# syntax=docker/dockerfile:1
# backend イメージ。用途ごとに最終ステージを使い分ける。
#   runtime    … 開発・動作確認用（debug ビルド）。docker-compose.yml が使う
#   production … 配布用（release ビルド）
# ステージを省略すると最後の production になるので、開発用は target を明示すること。
#
# ── 起動手順（配布用イメージでも同じ）─────────────────────────────
#   1. /app/run-migration.sh   … migration up + UNIQUE INDEX 4 件の DROP
#   2. /app/backend            … サーバー起動（既定の CMD）
#
# 手順 1 を飛ばして /app/migration up だけで済ませると backend は起動しない。
# backend は起動時に SeaORM の schema sync を無条件で実行する（main.rs）。sync は
# migration が作る UNIQUE INDEX を「制約」として DROP しようとするが、実体はインデックス
# なので Postgres が 42704（constraint ... does not exist）を返し、プロセスが落ちる。
# run-migration.sh はこの衝突を避けるため sync より先に該当インデックスを落とす。
# 同じ DROP は e2e/scripts/start-backend.sh と apps/backend/tests/common/mod.rs でも
# 行っており、環境を問わず必要な前処理になっている。
# ─────────────────────────────────────────────────
#
# Rust のバージョンはリポジトリルートの rust-toolchain.toml と合わせること。
# このイメージのビルドコンテキストは apps/backend なので rust-toolchain.toml はコンテキストに
# 含まれず、ここでの指定が唯一の版指定になる（rust-toolchain.toml を上げても自動追従しない）。

########## builder 共通 ##########
FROM rust:1.98-bookworm AS builder-base
# .cargo/config.toml が Linux ビルドで mold リンカ（-fuse-ld=mold）を要求する
RUN apt-get update \
    && apt-get install -y --no-install-recommends mold \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build

# ワークスペース一式（crates/ + migration/）をコピー
COPY . .

########## builder: debug ##########
# cargo のレジストリと target をキャッシュマウントに載せ、再ビルド時の依存クレート
# 再取得・再コンパイルを避ける。キャッシュマウントの中身は次のステージへ COPY できないため、
# 成果物のバイナリだけ /out に取り出す。
FROM builder-base AS builder-debug
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    mkdir -p /out \
    && cargo build --bin backend \
    && cp target/debug/backend /out/backend

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/migration/target \
    cargo build --manifest-path migration/Cargo.toml \
    && cp migration/target/debug/migration /out/migration

########## builder: release ##########
FROM builder-base AS builder-release
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    mkdir -p /out \
    && cargo build --release --bin backend \
    && cp target/release/backend /out/backend

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/migration/target \
    cargo build --release --manifest-path migration/Cargo.toml \
    && cp migration/target/release/migration /out/migration

########## runtime 共通 ##########
FROM debian:bookworm-slim AS runtime-base
# psql は run-migration.sh の DROP INDEX で使う。この DROP は開発専用の処理ではなく、
# backend が起動するための前提なので配布用にも同梱する（下の「起動手順」を参照）。
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# root では動かさない
RUN useradd --system --create-home --uid 10001 app
WORKDIR /app

COPY --chown=app:app --chmod=755 docker/run-migration.sh /app/run-migration.sh

########## runtime: 開発・動作確認用 ##########
FROM runtime-base AS runtime
COPY --from=builder-debug --chown=app:app /out/backend /app/backend
COPY --from=builder-debug --chown=app:app /out/migration /app/migration

USER app
EXPOSE 3400
CMD ["/app/backend"]

########## production: 配布用 ##########
FROM runtime-base AS production
COPY --from=builder-release --chown=app:app /out/backend /app/backend
COPY --from=builder-release --chown=app:app /out/migration /app/migration

USER app
EXPOSE 3400
CMD ["/app/backend"]
