# コード重複を解消する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-deduplicate-shared-code
- Polished: {YYYY-MM-DD}

## 目的

コードベース内の重複コードを解消し、保守性を向上させる。

## 現状

以下の重複が存在する:

1. `examples/http2_client/src/main.rs` と `examples/http2_server/src/main.rs` に同一実装の `find_header` ヘルパー関数が存在する
2. `src/webtransport/init.rs` の `DictionaryParser` と `src/webtransport/protocols.rs` の `ListParser` で RFC 8941 パーサーの基本メソッド（`skip_ows`, `skip_string`, `skip_boolean`, `skip_byte_sequence`, `skip_token`, `skip_number`, `parse_key`, `skip_parameters`）がほぼ同一実装で重複している
3. `crates/tokio-http2/src/tls.rs` と `crates/tokio-nghttp2/src/tls.rs` に同一実装の `InsecureVerifier` 構造体が存在する（`supported_verify_schemes()` の全署名スキーム列挙を含め完全に同一）
4. `src/stream/state.rs` の `StateMachine`、`src/frame.rs` の `SettingsFrame`、`src/frame/encoder.rs` の `FrameEncoder` の手動 `Default` 実装が `#[derive(Default)]` で代替可能

## 設計方針

- 1: `find_header` を共通の examples 用ユーティリティに抽出するか、それぞれの example 内に残す場合はコメントを追加する
- 2: RFC 8941 パーサーの共通基盤を `src/webtransport/` 内に抽出する（例: `src/webtransport/rfc8941.rs`）
- 3: `InsecureVerifier` を `tokio-http2` 側に集約し、`tokio-nghttp2` からは `tokio-http2` のものを使用する。または共通クレートに抽出する
- 4: `#[derive(Default)]` に置き換える

## 完了条件

- 上記の重複が解消されていること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
