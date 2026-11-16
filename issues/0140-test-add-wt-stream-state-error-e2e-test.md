# tokio ドライバで WT_STREAM_STATE_ERROR が CONNECT の RST_STREAM になることの E2E テストを追加する

- Created: 2026-09-09
- Completed: {YYYY-MM-DD}
- Branch: feature/add-wt-stream-state-error-e2e-test
- Polished: 2026-11-16

## 目的

`WtSession` が `WtErrorKind::StreamStateError` を返した際に、tokio ドライバが CONNECT ストリームへ `RST_STREAM(WT_STREAM_STATE_ERROR)` を送る経路の end-to-end テストが無い。マッピングの回帰を検出できるようにする。

draft-ietf-webtrans-http2-16 Section 6.4 は、無効な状態のストリームへの WT_STREAM 受信を
`WT_STREAM_STATE_ERROR` のセッションエラーとすることを MUST とし、Section 3.4 は
セッションエラーでセッションを終了させ、CONNECT ストリームの RST_STREAM に HTTP/2
エラーコードを載せて伝える。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::handle_event` は `wt_session.process()` のエラーを `abort_session_with_wt_error` に渡す。`wt_http2_error_code` が `WtErrorKind::StreamStateError` を `ErrorCode::WtStreamStateError` に対応付け、`abort_session_with_wt_error` が CONNECT ストリームを RST_STREAM する。しかし `crates/tokio-http2/tests/test_webtransport.rs` に `StreamStateError` の RST_STREAM 経路を検証するテストが無い。`FlowControlError` の対応は `test_wt_max_data_non_increase_aborts_session` が固定しているが、`StreamStateError` の対応は固定できていない。

## 設計方針

- `crates/tokio-http2/tests/test_webtransport.rs` に E2E テストを追加する
- クライアントは `WtSession::client` を生成して `initiate` し、`open_uni_stream` でローカル開始 uni ストリームを開き、`send_stream_data(stream_id, data, true)` で FIN を送る。`poll_output` の WT_STREAM capsule を `Client::send_data(connect_stream, out, false)` で送る。この FIN は WT_STREAM capsule の FIN ビットであり、HTTP/2 の END_STREAM は使わない。END_STREAM を使うと CONNECT ストリーム自体が閉じ、サーバーのドライバが `Error::ConnectionClosed` で終了して RST_STREAM 経路に入らない
- サーバーが FIN を処理して `WtSession::closed_stream_send_max` にストリーム ID を記録した後に 2 回目の WT_STREAM が届くよう、FIN を含む WT_STREAM capsule と 2 回目の WT_STREAM capsule は別の DATA フレームで送る (`Client::send_data` を 2 回呼ぶ)
- クライアント側 `WtSession::send_stream_data` は FIN 送信でローカル開始 uni ストリームを `remove_if_closed` により削除済みのため、同じ ID へ再送できない。2 回目は `CapsuleEncoder` で `Capsule::WtStream { stream_id, data: Vec::new(), fin: false }` を手動エンコードし、`Client::send_data` で CONNECT ストリームへ直接送る
- サーバー側は `WtServerRequest::accept` の後 `WtServerSession::into_parts()` の `driver` を await し、`Error::WebTransport` の `WtErrorKind::StreamStateError` で終了することを確認する
- クライアント側は `Event::StreamReset` を `tokio::time::timeout` 付きで待ち、`stream_id` が CONNECT ストリームと一致し、`error_code` が `ErrorCode::WtStreamStateError` であることを確認する。wire 値 0x101 は本実装の暫定値なので、リテラルではなく enum variant で比較する
- 既存の `test_wt_max_data_non_increase_aborts_session` の RST_STREAM 待機とドライバ終了確認、`test_wt_command_flush_error_not_masked_as_connection_closed` と同じハーネス (`Server::bind` / `await_connect_headers` / `WtServerRequest::from_connection` / `Client::connect_insecure` / `perform_connect`) を使う
- `cargo test --workspace` が通過することを確認する

## 完了条件

- `crates/tokio-http2/tests/test_webtransport.rs` に、クライアントが WT ストリームを FIN で閉じた後に同じ ID へ WT_STREAM を送り、サーバーのドライバが `WtErrorKind::StreamStateError` で終了し、クライアントが CONNECT ストリームの `Event::StreamReset` で `ErrorCode::WtStreamStateError` (本実装の暫定 wire 値 0x101) を受信することを検証するテストが追加されていること
- `cargo test --workspace` が通過すること
