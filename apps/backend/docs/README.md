# Backend ドキュメント

バックエンドの設計・要件ドキュメント。

| ドキュメント | 内容 | ステータス |
|-------------|------|-----------|
| [tenant-project-authz.md](./tenant-project-authz.md) | テナント / プロジェクト認可（`tenant_members`、公開規則、不変条件） | 実装済み |
| [personal-access-tokens-authz.md](./personal-access-tokens-authz.md) | PAT 認証・認可（2 層モデル、テナント/プロジェクト束縛） | 未実装 |
| [redis-trust-boundary.md](./redis-trust-boundary.md) | Redis 信頼境界（セッション・WebAuthn チャレンジ） | 運用ガイド |

## 開発環境セットアップ

### リンカ依存関係（mold）

ビルド時間短縮のため、Linux 環境では [mold](https://github.com/rui314/mold) リンカを使用する（`.cargo/config.toml` で設定済み）。

#### Linux

| ディストリビューション | インストールコマンド |
|----------------------|-------------------|
| Ubuntu / Debian | `sudo apt install mold` |
| Fedora / RHEL | `sudo dnf install mold` |
| Arch Linux | `sudo pacman -S mold` |

`mold` が見つからない場合は [GitHub Releases](https://github.com/rui314/mold/releases) からバイナリを取得するか、ソースからビルドする。

#### macOS

`.cargo/config.toml` の mold 設定は Linux ターゲット（`x86_64-unknown-linux-gnu` / `aarch64-unknown-linux-gnu`）にのみ適用されるため、**macOS では追加のリンカインストールは不要**。デフォルトの Apple ld が使用される。

#### Windows

mold は Windows をサポートしていない。`x86_64-pc-windows-msvc` ターゲットでは MSVC リンカ（Visual Studio Build Tools に同梱）がデフォルトで使用される。

**必要なもの**: [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) — 「C++ によるデスクトップ開発」ワークロードを選択してインストールする。

lld を使いたい場合は `rustup component add llvm-tools` を実行し、`.cargo/config.toml` に以下を追加する（実験的サポート）:

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "linker=rust-lld"]
```
