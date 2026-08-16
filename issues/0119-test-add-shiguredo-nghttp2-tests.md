# shiguredo_nghttp2 のテストを追加する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/test-add-shiguredo-nghttp2-tests
- Polished: {YYYY-MM-DD}

## 目的

`shiguredo_nghttp2` クレートのテストを拡充し、未テストの API をカバーする。

## 現状

`crates/shiguredo_nghttp2/tests/test_session.rs` には 4 件のテストしか存在せず、すべてクライアント側の正常系のみ。以下の API が未テスト:

- `submit_response()`（サーバー側）
- `submit_trailer()` / `submit_data_for_trailer()`
- `submit_headers()` / `submit_goaway()` / `submit_ping()` / `submit_window_update()`
- `submit_rst_stream()` / `submit_shutdown_notice()` / `terminate_session()`
- `consume()` / `consume_connection()` / `consume_stream()`
- 全フロー制御 API（`get_remote_window_size()` 等）
- `SessionOptions` を使用したセッション構築
- 不正なデータを `recv()` に渡した場合のエラーケース
- `submit_data()` のエラーケース

また `crates/shiguredo_nghttp2/src/validation.rs` の全関数（`check_header_name`, `check_header_value_rfc9113`, `check_method`, `check_path`, `check_authority`, `http2_strerror`, `is_fatal`）が未テスト。

## 設計方針

- `tests/test_session.rs` に各 API の正常系・異常系テストを追加する
- `tests/test_validation.rs` を新規作成し、`validation.rs` の全関数のテストを追加する
- テストは `cargo test -p shiguredo_nghttp2` で実行可能にする

## 完了条件

- 上記の未テスト API の正常系テストが追加されていること
- 主要なエラーケースのテストが追加されていること
- `validation.rs` の全関数のテストが追加されていること
- `cargo test -p shiguredo_nghttp2` が全件通過すること
