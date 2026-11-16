# WtSession の状態ガードを統一する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-session-state-guards
- Polished: {YYYY-MM-DD}

## 目的

`WtSession` の送信系 API の状態検査が非対称である。`send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` は `Closed` を検査せず、WT_CLOSE_SESSION の後ろに capsule を出力できる。また `process()` は `Initial` でも capsule を処理してストリームを生成する。draft-ietf-webtrans-http2-16 Section 3.2 / Section 6.12 に合わせて状態ガードを統一する。

## 現状

`src/webtransport.rs` の `send_stream_data` / `open_stream` / `send_datagram` / `send_max_streams` は `Active` / `Draining` を要求するが、`send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` はセッション状態を検査しない。`close()` で `Closed` にした後もストリームが残るため、これらの API が WT_MAX_DATA / WT_MAX_STREAM_DATA / WT_RESET_STREAM / WT_STOP_SENDING を出力できる。

`process()` は `Closed` のときだけ capsule を無視し、`Initial` では通常どおり `handle_capsule` を呼ぶ。サーバーは WebTransport セッションを accept するまで request stream の capsule を処理してはならない (Section 3.2)。

## 設計方針

- 全送信 API の先頭で共通の状態ガードを呼び、`Active` / `Draining` 以外では `session_state_error` を返す。
- `close()` は `Active` / `Draining` のみ許可する。
- `process()` / `feed()` は `Active` / `Draining` 以外で `session_state_error` を返す。Initial での処理を意図的に許す場合は、その契約と危険性を rustdoc に明記する。
- Closed 後の各 API、Initial からの `close`、Initial での `process` のテストを追加する。

## 完了条件

- `Closed` 後に `send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` がエラーになり、出力を生成しないこと。
- `Initial` での `process()` の扱いが実装と doc で一致していること。
- 各状態のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
