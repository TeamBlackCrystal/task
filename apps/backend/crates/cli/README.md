# task CLI

`task` コマンド。Rust の単一バイナリで、実行にランタイムの用意は要らない。

主なユースケースは AI レビュワーと CI からの `task review submit` / `task review summary`
（マージ前ゲート）。コマンドの仕様は `docs/features/review-findings.md` §6 が正。

## 導入

`v*` タグを push すると GitHub Release にバイナリが添付される
（`.github/workflows/release-cli.yml`）。clone もビルドも要らない。

| 配布物 | 使う場面 |
|---|---|
| `task-<version>-x86_64-unknown-linux-musl.tar.gz` | Linux 全般。静的リンクなので alpine 等の CI コンテナでもそのまま動く |
| `task-<version>-x86_64-unknown-linux-gnu.tar.gz` | glibc のある Linux |
| `task-<version>-aarch64-apple-darwin.tar.gz` | Apple Silicon の macOS |
| `task-<version>-x86_64-pc-windows-msvc.zip` | Windows |

GitHub Actions から使う例。取り違えや破損に気づけるよう `.sha256` を突き合わせる。

```yaml
- name: Install the task CLI
  env:
    # gh は Actions 上でトークンを明示しないと動かない
    GH_TOKEN: ${{ github.token }}
    TASK_CLI_VERSION: v0.1.9
  run: |
    set -euo pipefail
    asset="task-${TASK_CLI_VERSION#v}-x86_64-unknown-linux-musl.tar.gz"
    gh release download "$TASK_CLI_VERSION" --repo koyori-app/task --pattern "$asset*"
    sha256sum --check "${asset}.sha256"

    # runner のユーザーは /usr/local/bin へ書けない。書ける場所へ展開して PATH へ通す
    mkdir -p "$RUNNER_TEMP/bin"
    tar -xzf "$asset" -C "$RUNNER_TEMP/bin"
    "$RUNNER_TEMP/bin/task" --version
    echo "$RUNNER_TEMP/bin" >> "$GITHUB_PATH"
```

`GITHUB_PATH` が効くのは次のステップからなので、同じステップの中では絶対パスで呼ぶ。

手元でも同じ手順で入る。

```bash
version=v0.1.9
asset="task-${version#v}-x86_64-unknown-linux-musl.tar.gz"
gh release download "$version" --repo koyori-app/task --pattern "$asset*"
sha256sum --check "${asset}.sha256"   # macOS は shasum -a 256 --check
mkdir -p ~/.local/bin && tar -xzf "$asset" -C ~/.local/bin
~/.local/bin/task --version
```

`--version` が名乗る版はタグと一致する（リリース時にタグから注入し、その場で
突き合わせている）。タグ以外でビルドしたバイナリはクレートの版を名乗る。

### 検め方

リリースには、四つの配布物の hash を束ねた `SHA256SUMS` と、それへの keyless 署名
`SHA256SUMS.cosign.bundle` も添えてある。`.sha256` の突き合わせで分かるのは
「壊れていないか」まで。署名の検証まで行うと「この repo の release workflow が
タグから作った物か」まで確かめられる。

[cosign](https://github.com/sigstore/cosign) の **v3 以上**が要る。

```bash
version=v0.1.9
gh release download "$version" --repo koyori-app/task --pattern "SHA256SUMS*"

# 署名を検める。--certificate-identity-regexp と --certificate-oidc-issuer を
# 省いてはならない。省くと「誰かが Sigstore で署名した」ことしか確かめておらず、
# 別人が作った SHA256SUMS でも通ってしまう
cosign verify-blob SHA256SUMS \
  --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '^https://github\.com/koyori-app/task/\.github/workflows/release-cli\.yml@refs/tags/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com

# 手元へ落とした配布物が SHA256SUMS と一致することを確かめる
sha256sum --check SHA256SUMS --ignore-missing   # macOS は shasum -a 256 --check --ignore-missing
```

### コンテナイメージの検め方

ghcr.io へ公開しているイメージ（`task-backend` / `task-frontend`、
`.github/workflows/publish-images.yml`）にも同じ流儀で keyless 署名が付いている。
署名は tag ではなく digest に付くが、tag を指して verify しても cosign が
digest へ解決して検めるので、そのまま使える。

こちらも [cosign](https://github.com/sigstore/cosign) の **v3 以上**が要る。

```bash
# 署名を検める。バイナリと同じく --certificate-identity-regexp と
# --certificate-oidc-issuer を省いてはならない
cosign verify ghcr.io/koyori-app/task-backend:0.2.0 \
  --certificate-identity-regexp '^https://github\.com/koyori-app/task/\.github/workflows/publish-images\.yml@refs/tags/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

frontend は image 名を `task-frontend` に読み替える。

## ビルド

`apps/backend` の Cargo ワークスペースのメンバーなので、`cargo fmt --all` /
`cargo check --workspace` / `cargo clippy --workspace` / `cargo test --workspace`
の対象に入る。

```bash
cargo build --release -p task-cli   # apps/backend/target/release/task
```

版を差し替えたいときだけ `TASK_CLI_VERSION` を渡す（`build.rs` が読む）。

## タスクを扱う

一覧・検索は既定で 50 件（検索は 20 件）ずつ返る。総件数と続きの有無は出力の最後に出る。

```bash
task tasks list --project TASK --page 2 --limit 100
task tasks list --project TASK --status Todo --label bug --sort deadline_asc
task tasks list --project TASK --archived
task tasks search --project TASK ページング
```

`--status` / `--label` / `--assignee` / `--milestone` / `--sprint` は名前で指せる。
綴りを外すと、そのプロジェクトで使える名前が並ぶ。

作成と更新は同じ綴りの項目を受ける。

```bash
task tasks create --project TASK --title 直す \
  --soft-deadline 2026-09-30 --estimate 90 --label bug --assignee yupix
task tasks update TASK-181 --progress 40 --sprint week-40 --add-label bug
task tasks update TASK-181 --clear-sprint --archive
```

- 期限は RFC 3339（`2026-09-30T12:00:00Z`）か日付だけ（`2026-09-30` = その日の終わり、UTC）
- ソフト期限はハード期限より**前**でなければならない。同時刻は API が 400 を返す
- `--label` は今のラベルを**置き換える**。残したまま増減するなら `--add-label` / `--remove-label`
- `--assignee` も今の担当者を**置き換える**。更新では足りない人を足し、外れた人を外す
  （既にいる人の役割は変えない）
- 渡さなかった項目は送らないので、既存の値は消えない。消すときは `--clear-*` を使う

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
