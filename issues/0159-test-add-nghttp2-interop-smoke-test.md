# interop ディレクトリを新設し tokio-nghttp2 との疎通確認テストを追加する

- Created: 2026-09-13
- Completed: {YYYY-MM-DD}
- Branch: feature/add-nghttp2-interop-smoke-test
- Polished: {YYYY-MM-DD}

## 目的

独立実装 (nghttp2 C ライブラリ) との相互運用の確認を、`interop/` ディレクトリに実装の組み合わせ単位で置けるようにする。

http3-rs は `interop/h3` / `interop/wt` に quinn / ngtcp2 / s2n-quic / quiche との組み合わせごとのテストを置き、`make interop-test` から実行している。http2-rs にはこれに相当する場所が無く、相互運用の確認は `crates/tokio-http2/tests/interop.rs` (12288 行・`#[tokio::test]` 117 件) に集約されている。

本 issue では `interop/h2` を新設し、tokio-http2 と tokio-nghttp2 の双方向の疎通確認 (最小構成の 1 往復) を、クレートの回帰テストとは別のターゲットとして実行できるようにする。今後 interop に追加する検証 (実ブラウザ、外部実装) の置き場を先に決めておくことも狙いとする。

## 現状

- 独立実装との相互運用検証は `crates/tokio-http2/tests/interop.rs` に集約されている。12288 行・`#[tokio::test]` 117 件で、`cargo test --workspace` のたびに全件が実行される
- http3-rs の `interop/` は、実装の組み合わせごとにテストファイルを分けた crate (`interop/h3` / `interop/wt`) と Node.js の `interop/browser` で構成されている。http2-rs は `interop/` を持たない
- `crates/tokio-nghttp2` は Extended CONNECT / WebTransport を提供しないため、相互運用の対象は通常の HTTP/2 リクエスト / レスポンスである (`skills/shiguredo-http2/SKILL.md` の「既知の未対応 / 制限」)

## 設計方針

- `interop/h2/` を新設する。構成は http3-rs の `interop/wt` に倣い、`src/lib.rs` にヘルパー、`tests/` に組み合わせごとのテストを置く
  - `Cargo.toml`: package 名は `interop_h2`、`publish = false`、`edition` / `rust-version` は `workspace.package` を継承する
  - `src/lib.rs`: 自己署名証明書の生成、サーバーの起動、クライアントの接続、1 往復のヘルパーを置く
  - `tests/nghttp2_client_http2_server.rs`: tokio-nghttp2 の `Client` から tokio-http2 の `Server` へ接続する
  - `tests/http2_client_nghttp2_server.rs`: tokio-http2 の `Client` から tokio-nghttp2 の `Server` へ接続する
  - `README.md`: 実行方法と検証内容を書く
- ルートの `Cargo.toml` の `[workspace] members` に `interop/h2` を追加する。これにより `cargo fmt` / `cargo clippy` / `cargo test --workspace` の対象に入り、CI の変更なしで実行される
- Makefile に `interop-test` を追加する (`cd interop/h2 && cargo test`)。`.PHONY` にも追記する。CI に専用ステップは追加しない (workspace メンバーとして既存の `cargo test --workspace` が実行する)
- prek に専用フックは追加しない (既存の `cargo-test` フックが workspace 経由で実行する)
- モック・スタブは使わない。両実装をそのまま起動し、TLS で接続する (AGENTS.md)
- 検証するのは「接続できること」「`:status = 200` のレスポンスが返ること」「本文が一致すること」の 3 点に絞る。SETTINGS / フロー制御 / RST_STREAM などの網羅は既存の `crates/tokio-http2/tests/interop.rs` が担う
- 既存の `crates/tokio-http2/tests/interop.rs` と `crates/tokio-http2/Cargo.toml` は変更しない
- 証明書は `rcgen` で生成する (`crates/tokio-http2/tests/interop.rs` の `generate_http2_test_cert` / `generate_nghttp2_test_cert` と同じ方法)。クライアント側は証明書検証をスキップする設定を使う
- テストのログメッセージは日本語にする (AGENTS.md)
- `CHANGES.md` は変更しない (CODEBASE.md の指示)

## 完了条件

- `interop/h2` が workspace メンバーとして追加され、`cargo test -p interop_h2` が通ること
- tokio-nghttp2 クライアント ↔ tokio-http2 サーバー、tokio-http2 クライアント ↔ tokio-nghttp2 サーバーの両方向で、GET 1 往復の疎通確認テストが通ること
- `make interop-test` で実行できること
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること
- `crates/tokio-http2/tests/interop.rs` と `crates/tokio-http2/Cargo.toml` に変更が無いこと
- モック・スタブを追加していないこと
