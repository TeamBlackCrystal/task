# テナント / プロジェクト認可

**関連コード**: `crates/service/src/access.rs`, `crates/handler/src/extractors.rs`, `crates/handler/src/auth_helpers.rs`, `crates/handler/src/handlers/tenant_members.rs`, `crates/handler/src/handlers/project_members.rs`, `crates/entity/src/tenant_members.rs`

## 概要

「誰がどのテナント・プロジェクトに入れるか」の判定を記述する（#568 / #572）。

権限の形は NTFS や NFSv4 の ACL に倣う。
プロジェクトは既定でテナントの所属を受け継ぐ。これを**継承**と呼ぶ。
その上に、プロジェクト単位の **明示 ACE**（`project_members` の行）が重なる。
明示は継承より強い。テナントに行が無い利用者でも、明示 ACE があるプロジェクトには入れる（project-only の客分）。
継承元を外しても（テナント除名）、明示 ACE は独立して残る（「継承と明示」）。

判定は「テナントに入れるか（継承）」、「プロジェクトに入れるか」の順に進む。
明示 ACE を見るのは、継承で入れない場合に限る。

PAT のテナントバインドは「どのテナントを触れるか」の制限であって所属の証明ではない。
セッションと PAT はどちらも同じ所属判定を通る。

## 継承と明示

ACL の概念と、この実装での表現を対応づける。

| ACL の概念 | ここでの表現 | 意味 |
|---|---|---|
| 所有者 | `tenants.owner_id` | そのテナントの全プロジェクトに無条件で入れる。設定の変更と削除もできる。`tenant_members` に行は持たない |
| 継承（inherited） | `tenant_members` の行 | テナントに入れ、配下のプロジェクトに既定で入れる |
| 明示（explicit ACE） | `project_members` の行（`ProjectRole` 付き） | そのプロジェクトに入れる。継承より強く、継承元と独立に残る |
| 継承の停止 | `project_members` が 1 件以上あること（暗黙） | 明示 ACE を持つ人だけに絞る。継承だけの人は入れない |

明示 ACE の持ち主がテナントに居ない状態が、project-only の**客分**（guest）である。
客分は明示 ACE のあるプロジェクトの中だけ入れ、テナント全体の口は開かない。

以前「所属の 3 層」と呼んでいた区分（オーナー、テナントメンバー、客分）は、この表の所有者、継承のみ、明示のみに対応する。

### 除名が消すもの

テナント除名（`DELETE /v1/tenants/{id}/members/{user_id}`）が消すのは、継承（`tenant_members` の行）だけである。
明示 ACE（`project_members` の行）は残る。
明示 ACE は管理者が手で置いたものであり、継承元の変化を理由に黙って消すわけにはいかない。
NTFS でグループから外してもファイルの explicit ACE が残るのと同じ理屈である。
したがって、除名された人は残った明示 ACE のプロジェクトに客分として入り続ける。

管理者が「除名したのにまだ入れる」と驚かないよう、`GET /v1/tenants/{id}/members/{user_id}/explicit-projects` でその人の明示 ACE（project_id、key、name、role）を名指しできる。
除名の前に確かめる口だが、対象がテナントに居るかは問わないので、除名の後に「まだ何が残っているか」を見るのにも使える。
呼び出し側が入れないプロジェクトは名前や key を出さず、`hidden_count` に件数だけを載せる（テナント Admin でもプロジェクトの閲覧は `list_projects` と同じ境界で絞られる）。
閉め出したい場合は、そこに挙がったプロジェクトのメンバーからも外す。
除名（`DELETE`）自体の応答は従来どおり 204 で、body は持たない。

この定めは旧版（#688 以前）のデータにも遡って効く。
旧版で除名された人の残存行も明示 ACE であり、デプロイ後はそのプロジェクトに客分として入れる。
migration で消さない。残存行は管理者が明示的に置いた ACE であり、継承の消滅を理由に無効にする根拠が無いからである（#688 レビューの P1 に対する裁定）。

### 暗黙の継承停止

NTFS では「継承を無効にする」が明示の操作である。
ここでは明示 ACE を 1 件置いた時点で暗黙に継承が止まる（「プロジェクトの公開規則」）。
つまり、明示 ACE を「継承に足す」用途と「継承を止めて絞る」用途が、同じ行で表現されている。
`projects` に継承の旗（既定 true）を持たせて両者を分けるのは、別 PR で扱う。

### 客分（project-only guest）の定め

- 客分が入れるのは `project_members` に明示指定されたプロジェクトだけ。
  「メンバー未指定＝テナント全体に開放」の規則はテナントメンバー限りで、客分には開かない
- テナント全体の口のうち**プロジェクト一覧（`GET /v1/tenants/{id}/projects`）と
  My Tasks（`GET /v1/tenants/{id}/users/me/tasks`）だけは、客分には己が明示 member の
  project の分に絞って返す**（公開 project は含めない）。当初は「従来どおり 403」を
  採っていたが覆した（#688 レビュー）: frontend の project 解決（URL の projectKey →
  UUID）とテナント選択直後の着地（My Tasks）がこの 2 つの一覧に依存しており、
  UI 用の口を新設するより一覧を絞る方が判定を一箇所に保てるため。判定は
  `AuthUser::ensure_tenant_access_or_guest_scope` + `access::explicit_member_project_ids`
  に集約し、ハンドラーには散らさない
- その他のテナント全体の口（テナント取得・メンバー一覧・Drive・テナント設定など）は
  従来どおり 403。frontend は 403 を空表示・案内文で穏当に受ける（下の表）
- テナント一覧（`GET /v1/tenants`）には客分として関わるテナントが `membership: "Guest"` の
  印付きで出る（「テナント一覧の membership 印」）
- 客分は二つの経路で生まれる。テナントに居ない利用者をプロジェクトへ直接招く（`project_members::add_member`。
  GitHub の outside collaborator に相当）か、テナントメンバーへ明示 ACE を置いた後にテナントから外す
  （残った `project_members` の行が客分の名指しになる。`list_explicit_projects` がその行を名指しする）
- 招けるのはプロジェクト Admin とオーナーだけ。テナント側で客分の招待を締める旗は別 Issue で扱う
- 通知・メンションの宛先には入らない（`project_accessible_user_ids` はテナント在籍者に絞る）
- Drive のファイル配信（`drive_files::can_access_project`）は客分を通さない
- 客分もテナントの `require_2fa`（2FA 強制）の対象である
  （`login_session::user_in_require_2fa_tenant` が `access::guest_tenant_ids` で客分のテナントも見る）

### frontend が tenant-wide に叩く口と客分への扱い

| 口 | frontend の呼び元 | 客分への扱い |
|---|---|---|
| `GET /v1/tenants` | stores/tenant.ts（TenantSwitcher の一覧）・useResolvedTenantId（URL の display_id → UUID 解決） | 印付きで返す（membership=Guest。テナント設定の欄は null） |
| `GET /v1/tenants/{id}/projects` | api-vue-query の useProjectsQuery（AppSidebar の NavProjects・useResolvedProjectId の projectKey → UUID 解決）、ProjectCreateForm / DeleteProjectDialog / ProjectSettingsView（cache 更新） | **己の明示 member の project に絞って 200** |
| `GET /v1/tenants/{id}/users/me/tasks` | pages/@tenant/my-tasks（テナント選択直後の着地） | **己の project の分に絞って 200** |
| `GET /v1/tenants/{id}/members` | ProjectSettingsView の MembersSection | 403 のまま（MembersSection は 403 を「権限なし」表示で受け、画面は壊れない） |
| `GET /v1/tenants/{id}` | 呼び元なし（一覧で足りる） | 403 のまま |
| `GET /v1/tenants/{id}/users/me/personal-project` | 呼び元なし | 403 のまま |
| Drive 系（`/v1/tenants/{id}/drive...`） | tenant-wide では呼ばない | 403 のまま |

## `tenant_members`

| カラム | 型 | 説明 |
|---|---|---|
| `id` | `UUID` PK | |
| `tenant_id` | `UUID` NOT NULL | `tenants(id)` ON DELETE CASCADE |
| `user_id` | `UUID` NOT NULL | `users(id)` ON DELETE CASCADE |
| `role` | `VARCHAR` NOT NULL | `TenantRole`（`Admin` / `Member` / `Viewer`） |

`UNIQUE (tenant_id, user_id)` と `user_id` 単独の索引を張る。
前者の索引は先頭列が `tenant_id` なので、`user_id` だけで引く経路（ログインごとに通る 2FA 強制の判定、テナント一覧）には効かない。

### ロール

| ロール | 現在の意味 |
|---|---|
| `Admin` | テナントメンバーの追加・ロール変更・削除ができる |
| `Member` | テナントに入れる |
| `Viewer` | テナントに入れる（`Member` と同じ。読み取り専用の制限は未実装） |

ロールを見るのはメンバー管理 API だけで、それ以外の判定は「行があるか」しか見ない。

## プロジェクトの公開規則

| プロジェクトの状態 | 入れる人 |
|---|---|
| `project_members` が 0 件（継承のみ） | そのテナントに入れる人全員（客分は含まない） |
| `project_members` が 1 件以上（継承の停止 + 明示 ACE） | 明示 ACE を持つ人だけ（＋テナントオーナー）。その人がテナントに居なければ客分として入る |
| 個人プロジェクト（`is_personal = true`） | `personal_owner_id` の本人と、明示的に指定された人だけ（＋テナントオーナー） |

メンバーを 1 人も指定していないプロジェクトをテナント全体に開放するのは、
「参加したのにどのプロジェクトも見えない」状態を作らないため。

個人プロジェクト（Inbox）はこの規則から外す。
作成時は本人が `project_members` に入るが、その行は利用者の削除などで消えうる。
行の有無に頼ると 0 件になった時点でテナント全員に開いてしまうため、`is_personal` で明示的に閉じる。

ただしテナントオーナーは個人プロジェクトにも入れる。`has_tenant_access` がオーナーで短絡し、
公開規則の判定に進まないため（所属の 3 層の表のとおり、オーナーは全プロジェクトに無条件で入れる）。
一方で**通知の宛先には入らない** — `project_accessible_user_ids` は個人プロジェクトについて
`personal_owner_id` だけを返す。読めるが通知は来ない、という非対称は意図したもの。

## 判定の実装

### 入口は 1 つ

ハンドラーは `AuthUser::ensure_tenant_access` だけを呼ぶ。
これがセッション・PAT の双方を `has_tenant_access`（`extractors.rs`）に合流させる。
例外はテナント一覧系の 2 口（プロジェクト一覧・My Tasks）で、こちらは
`ensure_tenant_access_or_guest_scope` を呼ぶ — 通常判定に加えて客分を通し、
絞り込み用の project id 集合を返す。

```rust
auth.require_scope(Scope::ReadTask)?;
auth.ensure_tenant_access(&state, tenant_id, Some(project_id)).await?;
```

`has_tenant_access` の順序は次のとおり。

1. テナントを取得する（無ければ 404）
2. オーナーなら、プロジェクト指定があればテナント配下かだけ確認して通す
3. `tenant_members` に行が無ければ、プロジェクト名指しがあり `project_members` に
   明示指定がある場合（project-only の客分）に限りテナント配下確認の上で通す。
   それ以外は 403。存在探りを許さないため、明示指定の確認を実在確認より先に行う
4. プロジェクト指定があれば、そのプロジェクトがテナント配下かを確認する（違えば 404）
5. プロジェクトの公開規則で判定する

**同じ判定を呼び出し側で重ねない。**
`require_project_access`（`auth_helpers.rs`）は `ensure_tenant_access` の部分集合なので、
リクエスト元自身の認可に使うと同じクエリを二重に流すだけになる。
担当者の追加など**自分以外**を検証するときにだけ使う。

### `service::access` の 7 関数

認可（handler）と通知の宛先抽出（service）が同じ規則を見る必要があるため、実装をここに集約している。

| 関数 | 用途 |
|---|---|
| `is_tenant_member` | テナントに行があるか（オーナーは含まない） |
| `is_project_member` | `project_members` に明示指定があるか（公開規則は見ない）。客分の名指し判定にも使う |
| `project_is_open_or_member` | 1 プロジェクトの公開規則。**テナントに入れることは呼び出し側で確認済みの前提** |
| `visible_project_ids` | 一覧系。候補をまとめて 3 クエリで解決する（件数分のクエリを避ける） |
| `guest_tenant_ids` | 客分として関わるテナントの id 集合。テナント一覧の印付けに使う |
| `explicit_member_project_ids` | テナント配下で明示指定されている project の id 集合。客分の一覧絞り込みに使う |
| `project_accessible_user_ids` | 通知・メンションの宛先。テナントに残っている人だけに絞る |

### Drive は単純な置き換えをしない

Drive にはファイル ID だけで引ける経路がある（`GET /v1/drive/files/{id}/content`）。
プロジェクト所属だけを見るとテナント境界を越えられるため、
`can_access_project` はファイル自身の `tenant_id` に対してテナント所属を先に確認してからプロジェクト判定に進む。
この経路は客分を通さない（客分の層はタスク系 API の名指し経路だけに効く）。

## API と権限

テナント系エンドポイントは PAT に `admin:tenant` スコープを要求する。

| 操作 | 許可 |
|---|---|
| テナント一覧（`GET /v1/tenants`） | 所属しているテナント＋客分として関わるテナントを `membership` の印付きで返す。PAT もバインド先に同じ判定を適用する |
| テナントの取得（`GET /v1/tenants/{id}`） | テナントに入れる人全員（客分は含まない） |
| プロジェクト一覧（`GET /v1/tenants/{id}/projects`） | owner: 全件。member: 公開規則どおり。客分: 明示 member の分だけ |
| My Tasks（`GET /v1/tenants/{id}/users/me/tasks`） | 所属者: 従来どおり。客分: 己の project の分に絞る |
| テナントの更新・削除 | オーナーのみ |
| メンバー一覧の閲覧 | テナントに入れる人全員（客分は含まない） |
| メンバーの追加・ロール変更・削除 | オーナー + テナント `Admin` |
| プロジェクトの作成 | オーナーのみ |
| プロジェクトメンバーの管理 | オーナー + プロジェクト `Admin` |

一覧と取得で条件を揃えているのは、一覧に出るのに開けないテナントを作らないため（#572）。
客分のテナントは一覧に出るが取得は開かない、という非対称だけは意図して許した —
`membership: "Guest"` の印がその見分けであり、クライアントは印で開ける口を判断する。

### テナント一覧の membership 印

`GET /v1/tenants` は `TenantListItemResponse`（`TenantResponse` の全欄 + `membership`）を返す。
`membership` は `TenantMembershipKind` で、`TenantRole` と同じ流儀の PascalCase 文字列
（`Owner` / `Member` / `Guest`）。取得・作成・更新の口は従来どおり `TenantResponse` を返す。
客分にはテナント設定の欄（`owner_id` / `drive_quota_bytes` / `require_2fa`）を返さない（null）。

メンバー系レスポンス（`TenantMemberResponse` / `ProjectMemberResponse`）には表示用の
`user`（`UserSummary`: id / username / avatar_url）を同梱する。メンバー管理 UI（#317）が
ID とは別にユーザー名・アバターを引けるようにするためで、メールアドレス等は含めない。

## 守っている不変条件

| 不変条件 | 実装 |
|---|---|
| 明示 ACE はテナント所属と独立に置ける | `project_members::add_member` はテナントに居るかを問わない（#568 の「プロジェクトメンバー ⊆ テナントメンバー」は #688 で外した）。継承だけがテナント所属に依る |
| 除名は継承を外すだけで、明示 ACE を消さない | `tenant_members::remove_member` は `project_members` の行を消さない。残る行は `list_explicit_projects` で名指しできる |
| 利用者を削除しても同じ（管理者による強制削除も明示 ACE を消さない） | `admin_users::delete_user_cascade` も `project_members` ではなく `tenant_members` の行を消す |
| テナントに居ない人の明示 ACE は、そのプロジェクトの中だけの客分アクセスに限られる | `has_tenant_access` の客分分岐。tenant-wide・Drive・通知には及ばない |
| テナントに居ない人に通知が飛ばない | `project_accessible_user_ids` がテナント在籍者との積集合を返す |
| テナントに居ない人がプロジェクトの Admin 枠を占有しない | `would_drop_last_admin` がテナントに残っている Admin だけを数える |
| プロジェクト側の操作が最後の在籍 Admin を落とさない | `would_drop_last_admin` が 409。読みと書きの間に割り込まれないよう、プロジェクトメンバーの更新・削除と**テナントメンバーの除名**が同じテナント行を `FOR UPDATE` で掴む（`project_members::lock_membership_changes`） |
| そのプロジェクトを管理できる人が常に居る | `require_project_admin` がテナントオーナーを無条件で通す |

明示 ACE を残すのは「継承と明示」の定めによる。
副次的に、除名した人を戻したときに元の割り当てがそのまま復元され、その人しか指定されていなかったプロジェクトが 0 件になって継承へ戻ることも防ぐ。
閉め出したい場合はプロジェクトメンバーからも外す（`list_explicit_projects` が対象を名指しする）。

「最後の Admin を残す」判定は数えてから書くので、掴まないと 2 通りの抜け方がある。
同じプロジェクトで互いを降格させ合う経路と、降格が「まだ居る」と読んだ後・書く前に
その相手が除名される経路である。後者はプロジェクト行を掴んでも守れない（判定が読むのは
`tenant_members` でもあるため）ので、ロックはテナント行 1 つに寄せている。
同じテナントの別プロジェクトのメンバー操作まで待たされるが、管理操作は頻度が低い。

### 「在籍 Admin が 1 人以上」は不変条件ではない

直列化しても「A を降格 →（別の操作として）B を除名」の順は両方とも正当なので、
在籍している Admin が 0 人になることはある。除名側で数え直して 409 にすれば揃うが、
対象が単独 Admin のプロジェクトを全部直すまでテナントから外せなくなり、退職者の
オフボーディングが止まる。`admin_users::delete_user_cascade`（管理者による強制削除）は
そもそも 409 を返せないので、止めても同じ状態には到達する。

この状態は行き止まりではない。テナントオーナーがそのプロジェクトのメンバーを直せる。
`would_drop_last_admin` が在籍者だけを数えるのは、まさにこの状態から復旧できるように
するため（行数で数えると、もう操作できない人が最後の枠を占有して詰む）。

## HTTP ステータス

| 状況 | ステータス |
|---|---|
| 未認証 | 401 |
| テナントに入れない / プロジェクトに入れない / ロール不足 | 403 |
| テナントが存在しない / プロジェクトがそのテナントの配下にない | 404 |
| 同じ利用者を二重に追加した | 409 |
| 最後の Admin を削除・降格しようとした | 409 |

## 既知の制限

| 項目 | 現状 |
|---|---|
| 既存データの移行 | 行わない。`tenant_members` は空の状態から始まるため、非オーナーの利用者は API でメンバー登録するまでテナントに入れない |
| 客分の招待 UI | frontend のプロジェクト設定（MembersSection）はテナントメンバーからしか選べない。テナント外の利用者を招くのは API 直叩き |
| 客分の招待を締める旗 | 無い。プロジェクト Admin なら誰でも客分を招ける。テナント設定で締めるのは別 Issue |
| 継承の旗 | 無い。明示 ACE を 1 件置くと暗黙に継承が止まる（「暗黙の継承停止」）。`projects` に旗を持たせるのは別 PR |
| 客分への通知 | 飛ばない。`project_accessible_user_ids` がテナント在籍者に絞るため（プロジェクトには入れるのに通知が来ない非対称は既知） |
| `Viewer` ロール | 「所属している」以上の意味を持たない。書き込みの制限は全ハンドラーに波及するため未着手 |
| テナント `Admin` の権限 | テナントメンバーの管理のみ。プロジェクトの作成はオーナー専用（プロジェクトメンバーの管理は、上の表のとおりオーナー + プロジェクト `Admin`） |
| フォルダ共有 | テナントから外しても `drive_folder_shares` は失効しない。共有は所属とは独立した明示的な付与として扱っている |
| 管理 UI | プロジェクトメンバーは frontend 実装済み（#317。プロジェクト設定のメンバー節）。テナントメンバーの操作と CLI は未実装で、API 直叩き |
