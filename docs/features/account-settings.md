---
title: アカウント設定・プロフィール編集
description: ログイン中ユーザーが自分のプロフィールを編集する画面と API
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
| ユーザー名 | `users.username` | 一意制約は無い（同名を許す） |
| 自己紹介 | `users.bio` | |
| アバター | `users.avatar_url` | 画像の URL を指定する。ファイルのアップロードは未対応 |

メールアドレス・管理者フラグ・凍結フラグ・2FA の有効状態はこの API では変更できない。

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

ナビは 5 項目を表示するが、リンクとして機能するのは **プロフィール** のみ。
残りの 4 項目（環境設定 / 通知 / アクセストークン / セッション）は、
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
| `avatar_url` | `string?` | 2048 文字以内、`http://` または `https://` で始まる | 変更しない |
| `clear_avatar_url` | `bool` | | `false` |

**レスポンス**: `200 OK` / `UserResponse`（更新後の値）

### 省略と消去の区別

送らなかったフィールドは変更しない。フィールドが 1 つも無い `{}` を送った場合も
`200 OK` を返し、値は変わらない（空の `UPDATE` は発行しない）。

`avatar_url` を消したいときだけ、値ではなくフラグで指定する。

```json
{ "clear_avatar_url": true }
```

空文字（`""`）では消せない。`avatar_url` は `http://` / `https://` で始まることを要求するため、
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
そのまま描画経路に載るため、`http://` / `https://` 以外を保存段階で弾く
（`payload::users::validate_avatar_url`）。相対パスも受け付けない。

同じ判定をフロントエンドの `ProfileForm.vue` にも置いている。**片方だけ変えると、
画面では通るのに保存で 400 になる**（またはその逆）ので、変更するときは両方を直すこと。

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
| 環境設定 / 通知 / アクセストークン / セッション の各画面 | 本仕様の対象外 | アクセストークンは `/v1/personal_tokens`、通知は `/v1/users/me/notification-settings/{project_id}` が既にある |

画面上部のパンくず（`Account settings / Profile`）も未対応。
`+Layout.vue` のパンくずは全ページ共通の固定文字列で、ページごとに差し替える仕組みがまだ無い。

---

## 5. テスト

| 種別 | ファイル | 内容 |
|---|---|---|
| 統合 | `apps/backend/tests/profile_update_integration.rs` | 更新と永続化、省略項目を壊さないこと、`clear_avatar_url`、空 PATCH、スキーム拒否、長さ制限、未ログイン拒否、Bearer 拒否 |
| 単体 | `apps/frontend/src/components/settings/__tests__/ProfileForm.test.ts` | 空欄が `clear_avatar_url` になること、URL の送信、不正スキームとユーザー名長を送信前に止めること、失敗時の表示 |

拒否を確認するテストには、対照として通るケースも置いている（過剰に拒否していないことの確認）。
