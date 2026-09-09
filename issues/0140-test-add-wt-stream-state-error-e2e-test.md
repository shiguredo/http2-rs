# tokio ドライバで WT_STREAM_STATE_ERROR が CONNECT の RST_STREAM になることの E2E テストを追加する

- Created: 2026-09-09
- Completed: {YYYY-MM-DD}
- Branch: feature/add-wt-stream-state-error-e2e-test
- Polished: {YYYY-MM-DD}

## 目的

`WtSession` が WT_STREAM_STATE_ERROR を返した際に、tokio ドライバが CONNECT ストリームへ `RST_STREAM(WT_STREAM_STATE_ERROR)` を送る経路の end-to-end テストが無い。マッピングの回帰を検出できるようにする。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::handle_event` は `wt_session.process()` のエラーを `abort_session_with_wt_error` に渡す。`wt_http2_error_code` が `WtErrorKind::StreamStateError` を `ErrorCode::WtStreamStateError` に対応付けて CONNECT ストリームを RST_STREAM する。しかし `crates/tokio-http2/tests/test_webtransport.rs` にこの経路を検証するテストが無く、`WtErrorKind` と HTTP/2 エラーコードの対応を固定できていない。

## 設計方針

- クライアントがクローズ済みストリーム ID へ WT_STREAM を送り、サーバーのドライバが CONNECT ストリームを `RST_STREAM(WT_STREAM_STATE_ERROR)` で終了することを検証する E2E テストを追加する
- 既存の `test_wt_command_flush_error_not_masked_as_connection_closed` と同じハーネスを使う
- `cargo test --all` が通過することを確認する

## 完了条件

- クローズ済みストリームへの WT_STREAM で CONNECT ストリームが `WT_STREAM_STATE_ERROR (0x101)` の RST_STREAM で終了することを検証するテストが追加されていること
- `cargo test --all` が通過すること
