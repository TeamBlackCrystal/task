# テナント / プロジェクト認可

**関連コード**: `crates/service/src/access.rs`, `crates/handler/src/extractors.rs`, `crates/handler/src/auth_helpers.rs`, `crates/handler/src/handlers/tenant_members.rs`, `crates/handler/src/handlers/project_members.rs`, `crates/entity/src/tenant_members.rs`

## 概要

「誰がどのテナント・プロジェクトに入れるか」の判定を記述する（#568 / #572）。

テナントの所属は `tenant_members` テーブルで表現する。プロジェクトメンバーはその**絞り込み**であって、所属そのものではない。
したがって判定は必ず「テナントに入れるか」→「プロジェクトに入れるか」の順に進む。

PAT のテナントバインドは「どのテナントを触れるか」の制限であって所属の証明ではない。
セッションと PAT はどちらも同じ所属判定を通る。

## 所属の 3 層

| 層 | 表現 | 意味 |
|---|---|---|
| テナントオーナー | `tenants.owner_id` | そのテナントの全プロジェクトに無条件で入れる。設定変更・削除もできる |
| テナントメンバー | `tenant_members` の行 | テナントに入れる。プロジェクト単位の絞り込みを受ける |
| プロジェクトメンバー | `project_members` の行 | プロジェクトを特定の人に絞り込むための指定 |

オーナーは `tenant_members` に行を持たない。オーナーであること自体が所属を意味する。

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
| `project_members` が 0 件 | そのテナントに入れる人全員 |
| `project_members` が 1 件以上 | 指定された人だけ（＋テナントオーナー） |
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

```rust
auth.require_scope(Scope::ReadTask)?;
auth.ensure_tenant_access(&state, tenant_id, Some(project_id)).await?;
```

`has_tenant_access` の順序は次のとおり。

1. テナントを取得する（無ければ 404）
2. オーナーなら、プロジェクト指定があればテナント配下かだけ確認して通す
3. `tenant_members` に行が無ければ 403
4. プロジェクト指定があれば、そのプロジェクトがテナント配下かを確認する（違えば 404）
5. プロジェクトの公開規則で判定する

**同じ判定を呼び出し側で重ねない。**
`require_project_access`（`auth_helpers.rs`）は `ensure_tenant_access` の部分集合なので、
リクエスト元自身の認可に使うと同じクエリを二重に流すだけになる。
担当者の追加など**自分以外**を検証するときにだけ使う。

### `service::access` の 4 関数

認可（handler）と通知の宛先抽出（service）が同じ規則を見る必要があるため、実装をここに集約している。

| 関数 | 用途 |
|---|---|
| `is_tenant_member` | テナントに行があるか（オーナーは含まない） |
| `project_is_open_or_member` | 1 プロジェクトの公開規則。**テナントに入れることは呼び出し側で確認済みの前提** |
| `visible_project_ids` | 一覧系。候補をまとめて 3 クエリで解決する（件数分のクエリを避ける） |
| `project_accessible_user_ids` | 通知・メンションの宛先。テナントに残っている人だけに絞る |

### Drive は単純な置き換えをしない

Drive にはファイル ID だけで引ける経路がある（`GET /v1/drive/files/{id}/content`）。
プロジェクト所属だけを見るとテナント境界を越えられるため、
`can_access_project` はファイル自身の `tenant_id` に対してテナント所属を先に確認してからプロジェクト判定に進む。

## API と権限

テナント系エンドポイントは PAT に `admin:tenant` スコープを要求する。

| 操作 | 許可 |
|---|---|
| テナント一覧（`GET /v1/tenants`） | 所属しているテナントだけを返す。PAT もバインド先に所属している場合だけ返す |
| テナントの取得（`GET /v1/tenants/{id}`） | テナントに入れる人全員 |
| テナントの更新・削除 | オーナーのみ |
| メンバー一覧の閲覧 | テナントに入れる人全員 |
| メンバーの追加・ロール変更・削除 | オーナー + テナント `Admin` |
| プロジェクトの作成 | オーナーのみ |
| プロジェクトメンバーの管理 | オーナー + プロジェクト `Admin` |

一覧と取得で条件を揃えているのは、一覧に出るのに開けないテナントを作らないため（#572）。

メンバー系レスポンス（`TenantMemberResponse` / `ProjectMemberResponse`）には表示用の
`user`（`UserSummary`: id / username / avatar_url）を同梱する。メンバー管理 UI（#317）が
ID とは別にユーザー名・アバターを引けるようにするためで、メールアドレス等は含めない。

## 守っている不変条件

| 不変条件 | 実装 |
|---|---|
| プロジェクトメンバー ⊆ テナントメンバー ∪ { オーナー } | `project_members::add_member` がテナント外の利用者を 400 で弾く |
| 除名しても、その人しか指定されていなかったプロジェクトは開かない | `tenant_members::remove_member` は `project_members` の行を消さない |
| 利用者を削除しても同じ（管理者による強制削除も開かない） | `admin_users::delete_user_cascade` も `project_members` ではなく `tenant_members` の行を消す |
| テナントに居ない人の残った行はアクセスを与えない | `has_tenant_access` が所属を先に見る |
| テナントに居ない人に通知が飛ばない | `project_accessible_user_ids` がテナント在籍者との積集合を返す |
| テナントに居ない人がプロジェクトの Admin 枠を占有しない | `would_drop_last_admin` がテナントに残っている Admin だけを数える |

`project_members` の行を残すのは、除名した人を戻したときに元の割り当てをそのまま復元するためでもある。

## HTTP ステータス

| 状況 | ステータス |
|---|---|
| 未認証 | 401 |
| テナントに入れない / プロジェクトに入れない / ロール不足 | 403 |
| テナントが存在しない / プロジェクトがそのテナントの配下にない | 404 |
| テナント外の利用者をプロジェクトメンバーに追加した | 400 |
| 同じ利用者を二重に追加した | 409 |
| 最後の Admin を削除・降格しようとした | 409 |

## 既知の制限

| 項目 | 現状 |
|---|---|
| 既存データの移行 | 行わない。`tenant_members` は空の状態から始まるため、非オーナーの利用者は API でメンバー登録するまでテナントに入れない |
| `Viewer` ロール | 「所属している」以上の意味を持たない。書き込みの制限は全ハンドラーに波及するため未着手 |
| テナント `Admin` の権限 | メンバー管理のみ。プロジェクトの作成とプロジェクトメンバーの管理はオーナー専用のまま |
| フォルダ共有 | テナントから外しても `drive_folder_shares` は失効しない。共有は所属とは独立した明示的な付与として扱っている |
| 管理 UI | frontend / CLI 未実装。テナントメンバーの操作は API 直叩き |
