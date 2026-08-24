# STOP_SENDING 送信後の在路データで WebTransport セッション全体が abort される

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-stop-sending-inflight-abort
- Polished: 2026-08-24

## 目的

アプリが `WtBidiStream::stop_sending` 等で WT_STOP_SENDING を送信した後、ピアからの在路 (inflight) データが届いた際に、WebTransport セッション全体が RST_STREAM で終了する問題を修正する。RFC 9000 Section 3.5 の STOP_SENDING セマンティクスでは、STOP_SENDING 後の受信データは破棄すべきものであり、セッション終了は過剰な対応である。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::dispatch_wt_event` は受信データ (`WtEvent::StreamData`) ごとに `maybe_grow_stream_window` を呼ぶ。`maybe_grow_stream_window` は `WtSession::grow_stream_recv_window` (`src/webtransport.rs`) を呼び、`grow_stream_recv_window` は `stream.stop_sending_sent()` が true だと `stream_state_error` を返す。

その結果、アプリが STOP_SENDING を送信済みのストリームに、`maybe_grow_stream_window` の受信ウィンドウ拡張しきい値 (`recv_available < initial/2`) を下回る量の在路データが届くと、`grow_stream_recv_window` が `stream_state_error` を返し、`dispatch_wt_event` は `abort_session_with_wt_error` を呼んで、CONNECT ストリームへの RST_STREAM 送信 + セッション終了となる。少量の在路データではしきい値に達せず abort は発生しないため、再現にはしきい値を越えるデータ量が必要である。

さらに `dispatch_wt_event` は STOP_SENDING 後も受信データをアプリのチャネル (`StreamPacket::Data`) に配送しており、停止要求後のデータがアプリに渡り続ける。

## 設計方針

- `dispatch_wt_event` の `StreamData` 分岐で、STOP_SENDING 送信済みストリームへの受信データはウィンドウ拡張せず破棄する
- `maybe_grow_stream_window` / `grow_stream_recv_window` の呼び出しを STOP_SENDING 済みストリームではスキップする (または破棄の経路を設ける)
- STOP_SENDING 後の受信データをアプリに配送しない

## 完了条件

- STOP_SENDING 送信後にピアから在路データ (受信ウィンドウ拡張しきい値を越える量) が届いても、セッションが継続すること
- STOP_SENDING 後の受信データがアプリのチャネルに配送されないこと
- テストが追加され、`cargo test --all` が通過すること
