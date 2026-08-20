# CLAUDE.md

このプロジェクト固有の知見。

## プロジェクト構成

- `apps/backend` — Rust (axum + SeaORM + apalis)。Cargo ワークスペース
- `apps/frontend` — Vike + Vue 3 (Pinia + TanStack Query)。`openapi.json` から API 型を生成
- `apps/cli` — TypeScript CLI

### backend ワークスペース（依存は一方向・逆流禁止）

```
entity → common → payload → service → job → handler → backend(bin)
                              ↑
        auth-core / forge-* （外部リポジトリ。git 依存）
```

| クレート | 置くもの |
|---|---|
| `entity` | SeaORM エンティティ（sea-orm-cli 生成物。手で整形しない） |
| `common` | error / settings / db ヘルパー / 通知定数など最下層の横断コード |
| `payload` | リクエスト/レスポンス DTO。**依存は entity + common のみに閉じる** |
| `service` | ビジネスロジック・横断サービス（旧 `src/utils`） |
| `job` | apalis ジョブ。ワーカーは `AppState` ではなく `JobState` を受け取る |
| `handler` | axum ハンドラー / extractors / routes / openapi / middlewares / `AppState` |
| `backend` | `main` / `server` / `export_openapi` の glue のみ |

- 新しい DTO は payload、ロジックは service へ。ハンドラー間で共有したい処理も service に降ろす
- `backend::handlers` 等の再エクスポートは統合テスト互換のためのもの。新規コードは各クレートを直接 use する

#### 外部クレート（`github.com/koyori-app/auth-core`、git 依存で `rev` 固定）

OAuth 認証と Git ホスティング連携の土台。VRT ツールなど他プロジェクトと共有するため
別リポジトリに置いている。

| クレート | 置くもの |
|---|---|
| `auth-core` | OAuth 2.0 / OIDC のプロトコル層（PKCE / state / トークン暗号化 / SSRF ガード / `OAuthProvider` trait） |
| `auth-core-github` / `auth-core-gitlab` | 各ホストの OAuth ログインプロバイダー実装 |
| `forge-core` | ホスト中立の連携抽象（`TokenProvider` trait / `Repository` 型） |
| `forge-github` | GitHub App API クライアント。`Installation` 系の GitHub 固有語彙はこの中に閉じる |

- **アプリ固有のものを向こうへ入れない。** テナント/プロジェクトに紐づく state、
  「1 プロジェクト = 1 リポジトリ」前提のリポジトリ選定、環境変数の読み込みは
  `service::oauth` / `service::github` に置く
- 向こうを直したら `rev` を上げる必要がある。ワークスペースの `[workspace.dependencies]`
  に 5 クレート分まとめてあるので、そこを書き換える

## 検証（backend、コミット前に必ず）

```bash
cargo fmt                              # 忘れると fmt CI で落ちる
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo test --workspace --lib
```

- 統合テスト（`tests/`）は実 Postgres / Redis を使う（`.github/workflows/backend-test.yml` が正）。ローカルは docker さえ動いていればこれだけで動く:
  ```bash
  cargo test -- --test-threads=1   # CI は cargo nextest run --test-threads=1
  ```
  - ハーネス（`tests/common/mod.rs` の `ensure_test_env()`）が testcontainers で CI と同一イメージ（`postgres:17` / `valkey/valkey:8.1`）をランダムポートで起動する。手動の `docker run`・`apps/backend/.env` は不要。コンテナは各テストバイナリの終了時にハーネス（atexit）が自動削除する
  - `DATABASE_URL` / `REDIS_URL` が環境か `.env` に設定済みならそれを優先する（CI と同じ経路。CI はこの経路のためワークフロー変更不要）
  - SMTP・シークレット系の env はハーネスが CI と同じテスト用の値で補完する。GitHub App 系も設定不要（`load_github_test_env()` が自前注入）。SMTP は実サーバー不要
- API 表面を変えたら: `cd apps/frontend && pnpm openapi && node_modules/.bin/vp fmt`
  - CLI 型も再生成: `cd apps/cli && pnpm openapi:generate`。**忘れると `cli-test` の
    「Check generated CLI OpenAPI type drift」で落ちる**（#595 で実発生）。frontend だけ直して
    終わりにしない
  - `pnpm openapi` は **`pnpm install` 済みの作業ツリーで実行する**。未インストールだと
    `vp fmt` が無く、整形前の `openapi.json`（インデント差分だけで数千行）をコミットしかける
  - 整形は **`vp fmt`**（prettier は入っていない）。`api.d.ts` は gitignore 済み
  - API を変えていない PR では `openapi.json` の差分ゼロが検証項目になる

## 地雷（実際に踏まれた・発見されたもの）

新しい地雷を足すときに既存の項目を見直し、回帰ガード（CI・型・lint・固定テスト）で踏めなくなったものは消す。

- **SeaORM の生 SQL に `?` プレースホルダを書かない。** `Statement::from_sql_and_values` は SQL を無変換で sqlx に渡すため、Postgres では実行時構文エラーになる。`common::db` のヘルパー（`table_exists` / `column_exists` / `execute_bound` / `query_one_bool`）か `$N` 直書きを使う。この類のバグは過去に3箇所で見つかっている（#272 / #277）
- **`#[utoipa::path]` の path は nest 位置からの相対パス。** routes 側で同じパスを `.nest()` すると二重連結された URL に登録されて 404 になる（#277 で実発生）。既存ハンドラーの登録方法に合わせること
- **apalis のジョブペイロードは Postgres（apalis.jobs）に平文で永続化される。** トークン等の機微情報を載せない（Redis のみに保持する）。job クレートの「シリアライズ後キー集合」固定テストが回帰ガード。再送競合は `issued_at` 世代（Unix ミリ秒）を process 時に生成し、`email_verification::store_token` の世代比較（Lua）で後勝ち解決する
- **ワーカーに `AppState` を渡さない**（job → handler の循環になる）。必要な依存は `JobState` にフィールドを足す
- 増分ビルドの計測に `cargo build -p <crate>` を使わない。feature 解決がワークスペース全体と変わり依存を作り直すため、数字が実態と乖離する

## テスト・PR の流儀

- バグ修正 PR には**修正前の main で fail する回帰テスト**を付ける（バグの証明として機能させる）
- 統合テストは `tests/common` の `TestApp` を使う。拒否系（403/404）と対照の成功系（200/201、過剰拒否でないこと）をセットで書く
- 外部 API のモックが返す件数は、その API の境界（GitHub のページサイズ既定 30 件など）を**越えた値**にする。境界ちょうどにすると、ページングの欠落のような「境界で切れるバグ」をテストが隠す（#595 で実発生。30 件のモックが 31 件目以降の欠落を隠していた）
- エラーは握り潰さず `?` で伝播する（`unwrap_or(false)` / `let _ =` でのもみ消しが実バグを隠した前例あり）
- コミットは Conventional Commits + 日本語（例: `fix(backend): …` / `refactor(workspace): …`）。1 関心 = 1 PR
- PR 本文も日本語。「概要 / 変更内容 / 挙動の変化 / テスト」の構成
- テーブル・API・認可規則・ロールなど概念を増減したら、仕様書（`docs/` と `apps/backend/docs/`、索引は後者の `README.md`）を同じ PR で追従させる。実装の変更は日本語の説明文・権限表・末尾の決定事項ログに取り残されやすい（#575 で実発生）
- 作業用の git worktree（`.claude/worktrees/` 配下）は、PR マージなど作業が終わったら `git worktree remove <path>` で消す（上げっぱなし禁止。Docker コンテナと同じ扱い）
