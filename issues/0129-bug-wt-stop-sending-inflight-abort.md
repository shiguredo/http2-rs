# STOP_SENDING 送信後の在路データで WebTransport セッション全体が abort される

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-stop-sending-inflight-abort
- Polished: 2026-09-09

## 目的

アプリが `WtBidiStream::stop_sending` 等で WT_STOP_SENDING を送信した後、ピアからの在路 (inflight) データが届いた際に、WebTransport セッション全体が RST_STREAM で終了する問題を修正する。RFC 9000 Section 3.5 の STOP_SENDING セマンティクスでは、STOP_SENDING 後の受信データは破棄してよい (can be discarded upon receipt) ものであり、セッション終了は過剰な対応である。ただし同節は、破棄した場合でも当該データが connection / stream のフロー制御に引き続き計上されると定めている。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::dispatch_wt_event` は受信データ (`WtEvent::StreamData`) ごとに `maybe_grow_stream_window` を呼ぶ。`maybe_grow_stream_window` は `WtSession::grow_stream_recv_window` (`src/webtransport.rs`) を呼び、`grow_stream_recv_window` は `stream.stop_sending_sent()` が true だと `stream_state_error` を返す。

その結果、アプリが STOP_SENDING を送信済みのストリームに在路データが届き、`maybe_grow_stream_window` の受信ウィンドウ拡張しきい値 (`recv_available < initial/2`) を `recv_available` が下回ると、`grow_stream_recv_window` が `stream_state_error` を返し、`dispatch_wt_event` は `abort_session_with_wt_error` を呼んで、CONNECT ストリームへの RST_STREAM 送信 + セッション終了となる。STOP_SENDING 送信直後は `recv_available >= initial/2` であるため、abort の再現には `recv_available` を `initial/2` 未満まで減らす量 (目安として `initial/2` を超える量) の在路データが必要である。

さらに `dispatch_wt_event` は STOP_SENDING 後も受信データをアプリのチャネル (`StreamPacket::Data`) に配送しており、停止要求後のデータがアプリに渡り続ける。

## 設計方針

- `dispatch_wt_event` の `StreamData` 分岐で、STOP_SENDING 送信済みストリームへの受信データはアプリへ配送せず破棄し、`maybe_grow_stream_window` / `grow_stream_recv_window` を呼ばない (ストリームウィンドウ拡張をスキップする)
- 破棄するのはアプリへの配送とストリームウィンドウ拡張 (`WT_MAX_STREAM_DATA` 送信) だけにする。RFC 9000 Section 3.5 が破棄後もフロー制御への計上を求めるため、sans-io 層の `consume_recv` とドライバの `maybe_grow_session_window` (セッションウィンドウ拡張) は従来どおり維持する
- STOP_SENDING 済みかどうかは `WtSession::stream` への照会だけで判定しない。ピア開始 uni ストリームでは FIN 付き `StreamData` を `WtSession::poll_event` が `remove_if_closed` で削除してから `dispatch_wt_event` に届くため、照会は None になり破棄できない。ドライバ側で STOP_SENDING を送信したストリーム ID を記録し (例: `DriverState` に集合を持つ)、その記録で判定する。記録した ID は破棄判定後の FIN / リセット処理で削除し、長命セッションで無制限に増えないようにする
- FIN 付きデータを破棄する場合も、既存の `stream_channels.remove` と `account_peer_stream_closed` は維持し、`WT_MAX_STREAMS` の自動発行カウントがずれないようにする

## 完了条件

- STOP_SENDING 送信後にピアから在路データ (受信ウィンドウ拡張しきい値を下回るまで `recv_available` を減らす量) が届いても、セッションが継続すること
- STOP_SENDING 後の受信データがアプリのチャネルに配送されないこと (FIN 付きデータを含む)
- ピア開始 uni ストリームで FIN 付きデータを受信した場合もアプリのチャネルに配送されないこと
- テストが追加され、`cargo test --all` が通過すること
