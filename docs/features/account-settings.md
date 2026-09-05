---
title: アカウント設定・プロフィール編集
description: ログイン中ユーザーが自分のプロフィール・アクセストークンを管理する画面と API
icon: lucide:user-cog
---

# アカウント設定・プロフィール編集 仕様書

> ステータス: **Draft** / 作成日: 2026-08-19
> 依存: `users` テーブル（スキーマ変更なし）

---

## 1. 概要

ログイン中のユーザーが、自分のプロフィールを自分で編集できるようにする。

これまで `users` の内容を変更できる API は管理者専用の `PATCH /v1/admin/users/{id}` だけで、
本人がユーザー名や自己紹介を直す手段が無かった。本仕様で `PATCH /v1/auth/me` を追加し、
`/settings/profile` から編集できるようにする。

編集できるのは `users` テーブルに既にある列のうち、本人が持ち主である 3 つに限る。

| 項目 | 列 | 備考 |
|---|---|---|
| ユーザー名 | `users.username` | 一意制約は無い（同名を許す）。後述の影響あり |
| 自己紹介 | `users.bio` | |
| アバター | `users.avatar_url` | 画像の URL を指定する。ファイルのアップロードは未対応 |

メールアドレス・管理者フラグ・凍結フラグ・2FA の有効状態はこの API では変更できない。

### 同名を許すことの影響

`users.username` に一意制約は無く、この API も重複を確認しない。したがって、既存の利用者と
同じ `username` に変更できる。登録 API（`POST /v1/auth/register`）も同様に重複を許す。
本人によるプロフィール更新は監査ログに記録し、ユーザー名の変更前後を追跡できる。

これはメンションの宛先に影響する。コメント本文の `@ユーザー名` は
`service::task_activities` の `resolve_mentions` が `username` の完全一致で解決し、
プロジェクトに入っている該当者**全員**を通知先にする。同名の利用者が同じプロジェクトにいれば、
`@alice` は 2 人に届く。画面上でも、コメント・担当者・`AvatarGroup` の表示名が同じになる。

宛先を 1 人に確定させたくなった時点で、`users.username` に一意制約を入れる
（既存の重複行を先に解消する必要がある）か、メンションを ID 参照に変えるかの判断が要る。
本仕様ではどちらも行っていない。

---

## 2. 画面

### URL と入口

| 項目 | 値 |
|---|---|
| URL | `/settings/profile` |
| ページファイル | `apps/frontend/src/pages/settings/profile/+Page.vue` |
| 入口 | サイドバー下部のユーザーメニュー → **Account** |

テナントに紐づかない個人の設定なので、テナントスコープ（`/{tenant}/...`）の外に置く。

### 構成

左にアカウント設定のナビ、右にプロフィールのフォームを並べる。

ナビは 6 項目を表示するが、リンクとして機能するのは
**プロフィール** / **セキュリティ** / **アクセストークン**。
残りの 3 項目（環境設定 / 通知 / セッション）は、
対応する画面がまだ無いため `aria-disabled` の押せない項目として並べる。

> ナビ項目は `apps/frontend/src/components/settings/AccountSettingsNav.vue` の `items` に定義する。
> 画面を実装したら、その項目に `href` を足すだけで有効になる。

### フォームの挙動

- 初期値は `GET /v1/auth/me` の結果。
- アバターは URL 入力欄とプレビュー。**削除**ボタンは入力欄を空にする（この時点では保存しない）。
- 保存すると `PATCH /v1/auth/me` を送り、成功したら `/v1/auth/me` のクエリを無効化して
  サイドバーの表示名・アバターにも反映する。
- 入力チェックは送信前（`onSubmit`）と項目からフォーカスが外れたとき（`onBlur`）の両方で走る。
  条件は後述の API 側と同じものを持たせているので、正常な入力が 400 で跳ね返ることはない。

---

## 3. API

### `PATCH /v1/auth/me`

ログイン中ユーザー自身のプロフィールを更新する。

**リクエスト**（`UpdateProfileRequest`）

| フィールド | 型 | 制約 | 省略時 |
|---|---|---|---|
| `username` | `string?` | 3〜255 文字 | 変更しない |
| `bio` | `string?` | 1000 文字以内 | 変更しない |
| `avatar_url` | `string?` | 2048 文字以内、`https://` で始まる | 変更しない |
| `clear_avatar_url` | `bool` | | `false` |

文字数は UTF-16 コードユニットではなく Unicode コードポイント単位で数える。
`avatar_url` と `clear_avatar_url: true` は同時に指定できず、競合時は `400 Bad Request` を返す。

**レスポンス**: `200 OK` / `UserResponse`（更新後の値）

### 省略と消去の区別

送らなかったフィールドは変更しない。フィールドが 1 つも無い `{}` を送った場合も
`200 OK` を返し、値は変わらない（空の `UPDATE` は発行しない）。

`avatar_url` を消したいときだけ、値ではなくフラグで指定する。

```json
{ "clear_avatar_url": true }
```

空文字（`""`）では消せない。`avatar_url` は `https://` で始まることを要求するため、
空文字は入力チェックで弾かれるからである。既存の `PATCH .../tasks/{id}`（`UpdateTaskRequest`）
が nullable な項目に `clear_*` を持たせているのと同じ形にした。

`bio` には `clear_bio` を用意しない。`bio` は空文字がそのまま有効な値で、
新規登録時も空文字が入る（`NULL` と `""` をアプリ側で区別していない）ため、
消去は `{"bio": ""}` で足りる。

### 認可

エクストラクタは `GET /v1/auth/me` と同じ `CurrentUser` を使う。したがって次の拒否をそのまま引き継ぐ。

| 状態 | 応答 |
|---|---|
| 未ログイン | `401` |
| `Authorization: Bearer` 付き（PAT） | `401` — アカウント API は PAT 非対応 |
| 2FA の途中（half-authed） | `403` |
| 凍結済みユーザー | `403` |
| 入力チェック違反 | `400` |

PAT で自分のプロフィールを書き換えられないことは、
[個人アクセストークンの認可](../../apps/backend/docs/personal-access-tokens-authz.md) の
「アカウント API は PAT 非対応」に沿う。

### `avatar_url` のスキーム制限

`avatar_url` はフロントエンドで `<img src>` に流し込む。`javascript:` や `data:` を保存できると
そのまま描画経路に載るため、`https://` 以外を保存段階で弾く
（`payload::users::validate_avatar_url`）。HTTP は mixed content で表示できないため、相対パスと同様に受け付けない。

同じ判定をフロントエンドの `ProfileForm.vue` にも置いている。**片方だけ変えると、
画面では通るのに保存で 400 になる**（またはその逆）ので、変更するときは両方を直すこと。
前後の空白は、画面側の判定・送信値のどちらでも取り除く。

なお `avatar_url` は本人以外の画面でも `<img src>` に載る（担当者一覧の `AvatarGroup.vue` など）。
指定された URL は閲覧者のブラウザが直接取りに行くため、その URL を用意した側は
アバターを見た人の IP アドレスと User-Agent を知り得る。URL を指定させる方式に
内在する性質で、ファイルのアップロードに対応する（§4）まで解消しない。

---

## 4. 未対応

モックアップにあるが、この仕様では実装していない項目と、その理由。

| 項目 | 未対応の理由 | 必要な作業 |
|---|---|---|
| 表示名（Display name） | `users` に列が無い | マイグレーションで列を追加し、`UserResponse` と `UpdateProfileRequest` に載せる |
| タイムゾーン | 同上 | 同上。加えて日時表示側の適用箇所を決める必要がある |
| 言語 | 同上 | 同上。i18n の仕組み自体がまだ無い |
| メールアドレスの変更 | 変更後の再確認フローが無い（`email_verification` は新規登録用） | 変更要求 → 新アドレスへ確認メール → 確認後に反映、の経路を新設する |
| アバターのファイルアップロード | アップロード先の API が無い | ドライブ（`drive_files`）に載せるか、専用の保存先を用意するかの判断から |
| 環境設定 / 通知 / セッション の各画面 | 本仕様の対象外 | 通知は `/v1/users/me/notification-settings/{project_id}` が既にある |
| 二要素認証・パスキーの管理 | 認証方法（§7）とは別タスク（TASK-188 / TASK-189） | セキュリティ画面にセクションを足す。API は既にある |
| トークンのプロジェクト絞り込み | アクセストークン画面（§6）の対象外 | API（`project_ids`）は既にある。フォームに選択 UI を足す |

画面上部のパンくず（`Account settings / Profile`）も未対応。
`+Layout.vue` のパンくずは全ページ共通の固定文字列で、ページごとに差し替える仕組みがまだ無い。

---

## 5. テスト

| 種別 | ファイル | 内容 |
|---|---|---|
| 統合 | `apps/backend/tests/profile_update_integration.rs` | 更新と永続化、省略項目を壊さないこと、`clear_avatar_url`、空 PATCH、スキーム拒否、長さ制限、未ログイン拒否、Bearer 拒否 |
| 統合 | `apps/backend/tests/personal_tokens_list_integration.rs` | 一覧が自分の有効なトークンだけを名前順で返すこと、平文・ハッシュを含まないこと、取り消しで一覧から消えること、空一覧、未ログイン拒否、Bearer 拒否 |
| 単体 | `apps/frontend/src/components/settings/__tests__/ProfileForm.test.ts` | 空欄が `clear_avatar_url` になること、URL の送信、前後の空白を取り除くこと、不正スキームとユーザー名長を送信前に止めること、失敗時（400 / それ以外）の表示、保存後に編集を再開すると保存済み表示が消えること |
| 単体 | `apps/frontend/src/components/settings/__tests__/AccessTokensSection.test.ts` | 一覧表示（伏せ字・スコープ数・期限・最終使用）、発行リクエストの内容（90 日既定・無期限）、名前とスコープの入力チェック、403 の表示、取り消しの確認ダイアログと失敗時の表示、オーナーのテナントが無い場合の無効化 |
| 単体 | `apps/frontend/src/lib/__tests__/personal-tokens.test.ts` | 有効期限プリセットの計算、伏せ字、期限・最終使用の表示文言（境界値込み） |
| 統合 | `apps/backend/tests/oauth_integration.rs` | `has_password` の反転、初回パスワード設定が一度きりであること、パスワードを得たあとに最後の連携を解除できること |
| 単体 | `apps/frontend/src/components/settings/__tests__/AuthMethodsSection.test.ts` | パスワードあり／なしの出し分け、初回設定に現在のパスワード欄を出さないこと、確認不一致で送信しないこと、現在のパスワード不一致の表示、変更後のサインイン画面への遷移、連携一覧と追加候補、self-hosted の URL 未入力での無効化、確認を挟む解除と `instance_url` の付与、最後の認証方法の拒否表示 |
| 単体 | `apps/frontend/src/lib/__tests__/auth-methods.test.ts` | 認証方法の数え方（パスキー込み）、パスワード入力チェック（8 文字の境界・現在と同値・確認不一致） |

拒否を確認するテストには、対照として通るケースも置いている（過剰に拒否していないことの確認）。

---

## 6. アクセストークン画面

パーソナルアクセストークン（PAT）を発行・取り消しする画面。PAT 自体の認可設計は
[個人アクセストークンの認可](../../apps/backend/docs/personal-access-tokens-authz.md) を正とする。

| 項目 | 値 |
|---|---|
| URL | `/settings/tokens` |
| ページファイル | `apps/frontend/src/pages/settings/tokens/+Page.vue` |
| 本体 | `apps/frontend/src/components/settings/AccessTokensSection.vue` |

### 画面の構成と挙動

- **一覧**: `GET /v1/personal_tokens`（この画面のために新設）。自分が発行した
  取り消し前のトークンを名前の昇順で返す。各行にトークン名、末尾 4 文字だけの伏せ字
  （`pat_••••••7f3a`）、スコープ数、有効期限、最終使用日時を表示する。
  スコープの内訳は「スコープを表示」のドロップダウンで開く。
- **発行**: 「トークンを発行」でフォームを開き、`POST /v1/personal_tokens` を送る。
  - トークン名（1〜100 文字）
  - テナント（自分が**オーナー**のテナントのみ。1 件ならフォームに出さず自動選択）
  - 有効期限プリセット: 30日 / 90日（既定）/ 1年 / 無期限
  - スコープ（backend の全 11 種。1 つ以上必須。説明付きチェックボックス）
  - プロジェクトの絞り込み（`project_ids`）は常に `null`（テナント内全プロジェクト）で送る
- **平文トークンは発行直後の応答でしか取得できない**。発行後にコピー付きで 1 度だけ表示し、
  再表示できないことを明記する。
- **取り消し**: 確認ダイアログを経て `DELETE /v1/personal_tokens/{id}`。取り消したトークンは
  一覧に出なくなる（行の削除ではなく `revoked` フラグ。API は冪等）。

### 認可まわりの注意

- PAT の発行はテナントオーナー限定（backend の `require_tenant_owner`）。オーナーの
  テナントが無いユーザーには発行ボタンを無効にし、その旨を表示する。403 が返った場合も
  オーナー限定であることを伝える。
- トークン管理 API はすべてセッション専用。PAT（Bearer）でこの画面の API は呼べない。

### `GET /v1/personal_tokens`

この仕様で新設した一覧 API。

- 認可: セッション必須（PAT は 403）
- 返すもの: 自分（`user_id`）のトークンのうち `revoked = false` のもの。
  名前の昇順（同名は `id` 順）。`PersonalTokenResponse` の配列で、平文トークン・ハッシュは含まない
- トークンを 1 件も持たない場合は空配列（404 にしない）

---

## 7. セキュリティ画面（認証方法）

パスワードと OAuth 連携を「サインインできる方法」としてまとめて管理する画面。
OAuth の認可フローと解除規則は [OAuth ログイン](/features/auth-oauth) を正とする。

| 項目 | 値 |
|---|---|
| URL | `/settings/security` |
| ページファイル | `apps/frontend/src/pages/settings/security/+Page.vue` |
| 本体 | `apps/frontend/src/components/settings/AuthMethodsSection.vue` |
| パスワード行 | `apps/frontend/src/components/settings/PasswordMethodRow.vue` |

1 枚のカードに「パスワード」「連携済み」「追加できる連携」を上から並べる。

### パスワード

`GET /v1/auth/me` の `has_password` で出し分ける。

| 状態 | ボタン | 送信先 | 入力 |
|---|---|---|---|
| 設定済み | パスワードを変更 | `POST /v1/auth/password/change` | 現在 + 新しい + 確認 |
| 未設定（OAuth のみ） | パスワードを設定 | `POST /v1/auth/password` | 新しい + 確認 |

変更はすべてのセッションと PAT を失効させるため、フォームを開いた時点でその旨を出す。
成功したら `/signin?password_changed=1` へフルページ遷移し、サインイン画面はこのパラメータを見て
なぜサインアウトされたのかを表示する。クライアントルーティングにしないのは、
失効済みのセッションで取ったキャッシュを持ち越さないため。

入力チェックは `lib/auth-methods.ts` の `validatePasswordForm`。8 文字以上、確認との一致、
変更のときだけ「現在のパスワードが空でない」「現在と同じ値でない」。強度表示は
サインアップと同じ `PasswordStrengthBar` を使う。

### OAuth 連携

- **連携済み**: `GET /v1/auth/oauth/connections`。プロバイダー名、`provider_email`、接続日時、
  self-hosted の `instance_url` を出す。解除は確認を挟んで
  `DELETE /v1/auth/oauth/connections/{provider}`（self-hosted は `instance_url` をクエリに添える）
- **追加できる連携**: `GET /v1/auth/oauth/providers` のうち未連携のもの。「連携する」で既存の
  OAuth 開始 URL へフルページ遷移する。`redirect_after` にこの画面を指定して戻し、
  `?linked=<provider>` で連携できた旨を出す。プロバイダー側のエラーは backend が
  `?oauth_error=` を付けて同じ画面に戻す

開始 URL の組み立てとプロバイダー表示名は、サインイン画面の `OAuthButtons` と共通の
`lib/oauth-providers.ts` に置く。片方だけ直すと画面ごとに名前や戻り先が食い違うため。

### 最後の認証方法

利用できる認証方法が 0 件にならないよう、backend は「その連携が最後の 1 件 かつ
パスワード無し かつ パスキー無し」のとき解除を `403 oauth-last-auth-method` で拒む。

画面はこの判定を先取りして注意書きを出すだけで、可否はサーバーの応答に従う
（画面が数えた後に別のタブで増減されうるため）。先取りの数え方（`countAuthMethods`）は
backend と揃えてパスキーも数えるので、この画面は件数を見る目的で `GET /v1/auth/passkeys` も呼ぶ。

### `UserResponse.has_password`

パスワードの有無で表示を切り替えるために `GET /v1/auth/me`（`UserResponse`）へ
`has_password: bool` を追加した。ハッシュそのものは返さない。
