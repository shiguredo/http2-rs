# interop ディレクトリを新設し tokio-nghttp2 との疎通確認テストを追加する

- Created: 2026-09-13
- Completed: 2026-09-13
- Branch: feature/add-nghttp2-interop-smoke-test
- Polished: 2026-09-13

## 目的

独立実装 (nghttp2 C ライブラリ) との疎通確認を、実装の組み合わせ単位で実行できる入口を `interop/` に作る。http3-rs は `interop/h3` / `interop/wt` に quinn / ngtcp2 / s2n-quic / quiche との組み合わせごとのテストを置き、`interop/browser` に実ブラウザ検証を置いている。http2-rs にはこれに相当する場所が無い。

本 issue では `interop/h2` を新設し、tokio-http2 と tokio-nghttp2 の双方向の疎通確認を置く。あわせて `interop/` を、今後追加する実装の組み合わせの検証 (実ブラウザなど) の置き場として使えるようにする。

### 既存テストとの重複について

`crates/tokio-http2/tests/interop.rs` (12288 行・`#[tokio::test]` 117 件) が、同じ組み合わせの接続・レスポンス・本文一致を既に検証している (`test_nghttp2_client_http2_server_basic` / `test_http2_client_nghttp2_server_basic` / `test_nghttp2_client_http2_server_response_body` / `test_http2_client_nghttp2_server_response_body`)。したがって本 issue は新しいカバレッジを追加しない。

それでも `interop/h2` を新設するのは、次の 2 点のためである。

- 実装の組み合わせが最小構成で疎通するかを、12288 行の回帰テスト全体を回さずに 1 コマンドで確認できる入口を作る
- 実装の組み合わせごとの検証を、両クレートの回帰テストから分離して置く場所を決める

重複を最小に抑えるため、`interop/h2` に置くテストは双方向の GET 1 往復の 2 件だけに限定し、既存の網羅的な回帰テストは変更しない。

## 現状

- 独立実装との相互運用検証は `crates/tokio-http2/tests/interop.rs` に集約されている。12288 行・`#[tokio::test]` 117 件で、`cargo test --workspace` のたびに全件が実行される
- http3-rs の `interop/` は、実装の組み合わせごとにテストファイルを分けた crate (`interop/h3` / `interop/wt`) と Node.js の `interop/browser` で構成されている。http2-rs は `interop/` を持たない
- http3-rs は interop crate を workspace のテストから `--exclude` し、`make interop-test` を macOS の専用ステップで実行している。理由は quiche (BoringSSL) と neqo (NSS) のシンボル衝突で Linux のリンクが失敗することと、ブラウザ導入のコストである
- `crates/tokio-nghttp2` は Extended CONNECT / WebTransport を提供しないため、相互運用の対象は通常の HTTP/2 リクエスト / レスポンスである (`skills/shiguredo-http2/SKILL.md` の「既知の未対応 / 制限」)
- ルートの `Cargo.toml` の `[workspace.package]` は `edition` と `rust-version` のみで `version` を持たない。追加する crate は `version` を明示する必要がある (`pbt/Cargo.toml` / `examples/wt_server/Cargo.toml` は `version = "0.0.0"`)

## 設計方針

### crate の構成

- `interop/h2/` を新設する。構成は http3-rs の `interop/wt` に倣い、`src/lib.rs` にヘルパー、`tests/` に組み合わせごとのテストを置く
  - `Cargo.toml`: `name = "interop_h2"` / `version = "0.0.0"` / `publish = false` / `edition.workspace = true` / `rust-version.workspace = true` / `[lib] path = "src/lib.rs"`。依存には用途をコメントで明記する (`shiguredo-rust` の「依存ライブラリには用途をコメントで明記すること」)
    - `rcgen` — 自己署名証明書の生成
    - `rustls-pki-types` — 証明書の DER 型
    - `tokio` — 非同期ランタイム
    - `tokio-http2` — 本実装
    - `tokio-nghttp2` — nghttp2 ベース実装
  - `src/lib.rs`: 自己署名証明書の生成、サーバーの起動、クライアントの接続、1 往復のヘルパーを置く
  - `tests/nghttp2_client_http2_server.rs`: tokio-nghttp2 の `Client` から tokio-http2 の `Server` へ接続する
  - `tests/http2_client_nghttp2_server.rs`: tokio-http2 の `Client` から tokio-nghttp2 の `Server` へ接続する
  - `README.md`: 実行方法と検証内容を書く
- ルートの `Cargo.toml` の `[workspace] members` に `interop/h2` を追加する

### 実行形態

- workspace メンバーとして追加し、`cargo fmt` / `cargo clippy --workspace --all-targets` / `cargo test --workspace` の対象に入れる。http3-rs が `--exclude` して専用ステップへ分離している理由 (リンカ衝突・ブラウザ導入コスト) は本リポジトリには無く、tokio-nghttp2 は既に全 OS の CI で動いている。専用ステップへ分離するより、CI の変更なしで 3 OS の CI で実行されるほうが得られる確認が多い
- Makefile に `interop-test` を追加し、実体は `cargo test -p interop_h2` にする。workspace 全体ではなく interop だけを回す入口であることを名前どおりにするためである (`.PHONY` にも追記する)。0160 が `interop-test-browser` を先に追加している場合は、`.PHONY` とターゲットを既存の定義に追記する
- prek に専用フックは追加しない (既存の `cargo-test` フックが workspace 経由で実行する)
- CI に専用ステップは追加しない (既存の `cargo test --workspace` が実行する)

### 検証内容

- 検証するのは「接続できること」「`:status = 200` のレスポンスが返ること」「本文が一致すること」の 3 点を双方向で 1 回ずつ。SETTINGS / フロー制御 / RST_STREAM などの網羅は既存の `crates/tokio-http2/tests/interop.rs` が担う
- 既存の `crates/tokio-http2/tests/interop.rs` と `crates/tokio-http2/Cargo.toml` は変更しない
- モック・スタブは使わない。両実装をそのまま起動し、TLS で接続する (AGENTS.md)
- 証明書は `rcgen` で生成する (`crates/tokio-http2/tests/interop.rs` の `generate_http2_test_cert` / `generate_nghttp2_test_cert` と同じ方法)。クライアント側は証明書検証をスキップする `TlsClientConfig::insecure` を使う
- テストのログメッセージは日本語にする (AGENTS.md)
- `CHANGES.md` は変更しない。`publish = false` のテスト専用 crate と Makefile の追加だけで、公開 API と配布物に影響しないためである (CODEBASE.md の「この指示がなくなるまでは変更履歴を `CHANGES.md` に残さないこと」にも従う)

## 完了条件

- `interop/h2` が workspace メンバーとして追加され、`cargo test -p interop_h2` が通ること
- tokio-nghttp2 クライアント ↔ tokio-http2 サーバー、tokio-http2 クライアント ↔ tokio-nghttp2 サーバーの両方向で、GET 1 往復の疎通確認テストが通ること
- `interop/h2` のテストが双方向の GET 1 往復の 2 件のみで、既存テストの写しを増やしていないこと
- `make interop-test` が `cargo test -p interop_h2` を実行し、interop のテストだけを回せること
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること
- `crates/tokio-http2/tests/interop.rs` と `crates/tokio-http2/Cargo.toml` に変更が無いこと
- モック・スタブを追加していないこと

## 解決方法

- `interop/h2` を新設した (`Cargo.toml` / `src/lib.rs` / `tests/nghttp2_client_http2_server.rs` / `tests/http2_client_nghttp2_server.rs` / `README.md`)。package 名は `interop_h2`、`publish = false`
- ルートの `Cargo.toml` の `[workspace] members` に `interop/h2` を追加し、`cargo fmt` / `cargo clippy --workspace --all-targets` / `cargo test --workspace` の対象にした
- Makefile に `interop-test` (`cargo test -p interop_h2`) を追加した。あわせて `.PHONY` を実ターゲットに合わせて整理し、`clippy` に `--all-targets` を追加した
- 検証は双方向の GET 1 往復の 2 件のみとした。`rcgen` の自己署名証明書と `TlsClientConfig::insecure` を使い、モック・スタブは使っていない
- 応答送信後はピアが接続を閉じるか `LINGER_TIMEOUT` (2 秒) まで `next_event` を回し続ける `drain_http2_connection` / `drain_nghttp2_connection` を追加した。未読データを残したまま接続を閉じると OS が RST を送り、送信済みの応答 DATA が失われていた (実測: 修正前は順方向 44/60 失敗、修正後は 0/60)
- 接続確立と accept に `IO_TIMEOUT` (5 秒) を適用し、エラー値とタイムアウトを区別して報告するようにした。`StreamReset` / `StreamClosed` / `GOAWAY` も明示的に失敗として扱う
- `crates/tokio-http2/tests/interop.rs` と `crates/tokio-http2/Cargo.toml` は変更していない
- `CHANGES.md` は変更していない (`publish = false` のテスト専用 crate と Makefile の追加のみで、公開 API と配布物に影響しないため)
