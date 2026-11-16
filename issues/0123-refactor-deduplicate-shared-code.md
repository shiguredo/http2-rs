# コード重複を解消し、自明な Default 実装を derive に置き換える

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-deduplicate-shared-code
- Polished: 2026-11-16

## 目的

コードベース内の重複コードを解消し、手動 `Default` 実装を `#[derive(Default)]` に置き換えて保守性を向上させる。

## 現状

以下の重複・自明な実装が存在する:

1. `examples/http2_client/src/main.rs` と `examples/http2_server/src/main.rs` に同一実装の `find_header` ヘルパー関数が存在する（両者とも `headers.iter().find(...).map(...)` の 6 行で完全に同一）
2. `src/webtransport/init.rs` の `DictionaryParser` と `src/webtransport/protocols.rs` の `ListParser` で RFC 9651 パーサーの基本メソッド（`is_empty`, `peek`, `advance`, `consume_if_eq`, `skip_ows`, `skip_string`, `skip_boolean`, `skip_byte_sequence`, `skip_token`, `parse_number` / `skip_number`, `parse_key`, `skip_parameters`）がほぼ同一実装で重複している。`parse_key` の戻り値、`parse_number` / `skip_number` の桁数検証、`skip_parameters` が扱う bare item の範囲、エラーメッセージが異なる
3. `crates/tokio-http2/src/tls.rs` と `crates/tokio-nghttp2/src/tls.rs` に同一実装の private `InsecureVerifier` 構造体が存在する（`supported_verify_schemes()` の全署名スキーム列挙を含め完全に同一）
4. `src/stream/state.rs` の `StateMachine`、`src/frame.rs` の `SettingsFrame`、`src/frame/encoder.rs` の `FrameEncoder` の手動 `Default` 実装が `#[derive(Default)]` で代替可能（`StreamState` は `#[default] Idle`、`SettingsFrame` の `ack: bool` / `settings: Vec<Setting>` と `FrameEncoder` の `buf: Vec<u8>` はいずれも `Default` を実装している）

## 設計方針

- 1: `find_header` を共通の examples 用ユーティリティに抽出する。examples は独立クレートであり、6 行のヘルパーのために共通クレートを新設するのが過剰と判断する場合は、各 example に同一実装を残す理由をコメントで明記する（この場合は意図的な重複として扱う）
- 2: RFC 9651 パーサーの共通基盤を `src/webtransport/` 内に抽出する（例: `src/webtransport/rfc9651.rs`）。`parse_key` の戻り値、`parse_number` / `skip_number` の桁数検証、`skip_parameters` が扱う bare item の範囲、エラーメッセージの違いは共通基盤側で吸収する。`protocols.rs` の既知の制限（Date / Display String 非対応）を解消する場合は、モジュール doc の記載も更新する。open issue `issues/0189-bug-fix-wt-header-parsing.md` の設計方針にも RFC 9651 パーサーの共通化が含まれるため、0189 を先に実装する場合は 0189 の共通化結果を本項の完了として扱い、重複して共通化しない
- 3: `InsecureVerifier` を `tokio-http2` 側に集約し、`tokio-nghttp2` からは `tokio-http2` のものを使用する。`tokio-nghttp2` は現在 `tokio-http2` に依存していないため、`tokio-http2` への依存を追加し、`InsecureVerifier` を `pub` にする。依存追加を避ける場合は、両クレートから参照できる共通クレートに抽出する
- 4: `#[derive(Default)]` に置き換える。`new()` はそのまま維持する（`StateMachine::new()` は `const fn` のまま）

## 完了条件

- 1〜4 の各項目が次の状態になっていること:
  - 1: `find_header` が共通化されている、または各 example に残した理由がコメントで明記されている
  - 2: RFC 9651 パーサーの共通基盤が抽出され、`DictionaryParser` / `ListParser` の重複した基本メソッドが共通基盤を参照している（0189 の共通化結果を利用する場合を含む）
  - 3: `InsecureVerifier` の実装が 1 箇所に集約され、両クレートが同じ実装を参照している
  - 4: `StateMachine` / `SettingsFrame` / `FrameEncoder` の手動 `Default` 実装が `#[derive(Default)]` に置き換わっている
- `cargo fmt --all -- --check` が通過すること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
