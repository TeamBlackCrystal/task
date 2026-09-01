# task CLI

`task` コマンド。Rust の単一バイナリで、実行にランタイムの用意は要らない。

主なユースケースは AI レビュワーと CI からの `task review submit` / `task review summary`
（マージ前ゲート）。コマンドの仕様は `docs/features/review-findings.md` §6 が正。

## ビルド

`apps/backend` の Cargo ワークスペースのメンバーなので、`cargo fmt --all` /
`cargo check --workspace` / `cargo clippy --workspace` / `cargo test --workspace`
の対象に入る。

```bash
cargo build --release -p task-cli   # apps/backend/target/release/task
```

## 設定

`~/.config/task/config.yaml`（トークンを含むので `0600` で保存する）。環境変数が優先される。

| 設定 | 環境変数 |
|---|---|
| `api_url` | `TASK_API_URL` |
| `token` | `TASK_TOKEN` |
| `tenant_id` | `TASK_TENANT` |

```bash
task config set api_url https://task.example.com
task config set tenant_id <tenant-uuid>
task auth token < token.txt        # 引数を省くと標準入力から読む
task auth whoami
```

## 終了コード

ゲートとして使えるよう、失敗の理由を終了コードで分ける。

| コード | 意味 |
|---|---|
| 0 | 成功 |
| 1 | ゲート不成立（`review summary`）、またはその他の失敗 |
| 2 | 引数・投入 JSON の検証エラー、設定不足 |
| 3 | 認証失敗（401） |
| 4 | 権限不足（403） |
| 5 | 対象が見つからない（404） |

## API の型

リクエスト / レスポンスの型は backend の `payload` クレートをそのまま使う。手書きの
型定義を挟まないので、API 表面が変わったのに CLI が追従していない状態はコンパイル
エラーになる（#647 で手書き `paths.ts` が実際の応答とずれていた）。
