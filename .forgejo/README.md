# Forgejo Actions

GitHub と Forgejo のどちらでも CI が回るようにするための置き場。ここには
**検証系だけ**を置いている（backend / frontend / cli / OpenAPI 差分）。

Forgejo は `.forgejo/workflows` があればそちらだけを読み、`.github/workflows` は
見ない。GitHub は逆に `.github/workflows` しか見ない。したがって二重には走らず、
それぞれのホストで独立に動く。

## ワークフロー

| ファイル | 見るもの |
|---|---|
| `workflows/backend.yml` | `cargo fmt --check` / `clippy -D warnings` / `cargo test`（実 Postgres・Valkey）/ `openapi.json` の差分 |
| `workflows/frontend.yml` | `fmt:check` / `lint` / `vue-tsc --noEmit` / `test:unit` / `build` |
| `workflows/cli.yml` | 型検査（本体・契約テスト）/ 生成型の差分 / `pnpm test` |

## runner に必要なもの

- **ラベル `docker`**。ジョブはコンテナの中で走る前提で書いている。実イメージは
  runner の `config.yml`（`container.labels`）で決める。Debian/Ubuntu 系であること
  （`apt-get` で `mold` や `jq` を入れるため）
- **`services:` が使えること**（backend のテストが Postgres と Valkey を使う）。
  ジョブがコンテナで走るので、接続先は `localhost` ではなくサービス名
  （`postgres:5432` / `redis:6379`）。ワークフロー側でそう書いてある
- **キャッシュサーバ**（`actions/cache` の保存先）。無くても失敗はしないが、
  cargo と pnpm を毎回取り直すので遅くなる
- **github.com へ出られること**。`uses:` は全部 `https://github.com/...` の
  フル URL で、SHA も GitHub 側と同じものに固定している

## GitHub 側との意図的な違い

- ジョブを分割せず、アプリごとに 1 本にまとめている。runner が 1 台の構成では、
  ジョブごとにビルドや `pnpm install` をやり直す方が高くつくため
- `sccache` を使わない。GitHub Actions のキャッシュ API に乗る仕組みで、
  instance 側の対応が読めないため（cargo のキャッシュだけで足りる）
- `cargo nextest` ではなく `cargo test` を使う。追加のツール取得を増やさないため
  （`--test-threads=1` はどちらも同じ）
- `cargo build --release` は入れていない。デバッグビルドで clippy と test が通って
  おり、リリースビルドは GitHub 側の Backend Build と Docker Build が見る
- VRT / Argos / Chromatic / opencode / PR コメント系は移していない。外部 SaaS か
  GitHub 固有の API に依存していて、Forgejo では素直に動かないため

## 移していないもの（必要になったら足す）

`e2e.yml` / `docker-build.yml` / `api-path-param-gate.yml` / `publish-images.yml` /
`report-api-diff.yml` / `frontend-coverage.yml` / `frontend-bundle-diagnostics.*`。
