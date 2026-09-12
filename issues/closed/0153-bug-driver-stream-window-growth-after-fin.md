# FIN 受信後のストリームへ WT_MAX_STREAM_DATA が送信される

- Created: 2026-09-12
- Completed: 2026-09-12
- Branch: feature/fix-driver-stream-window-growth-after-fin
- Polished: 2026-09-12

## 目的

tokio ドライバが受信状態 `Recv` でないストリームへ WT_MAX_STREAM_DATA を送らないようにする。RFC 9000 Section 3.3 は「The receiver only sends MAX_STREAM_DATA frames in the "Recv" state.」とし、Section 19.10 も「A MAX_STREAM_DATA frame can be sent for streams in the "Recv" state; see Section 3.2.」とするため、FIN を受信済みのストリームへの送信はこの制約に反する。

あわせて、0149 (`issues/0149-bug-recv-state-operations-validation.md`) が `WtSession::grow_stream_recv_window` に追加する受信状態の検証で、ドライバが正常な通信中にセッションを異常終了させる回帰を先に塞ぐ。0149 はこの制御をスコープ外としており、本 issue がその前提になる。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` は `WtSession::stream` が返す `WtStream` の `recv_available` だけを見て、`recv_available < initial.div_ceil(2)` なら `WtSession::grow_stream_recv_window` を呼ぶ。呼び出し元は `DriverState::dispatch_wt_event` の `WtEvent::StreamData` 分岐で、`fin: true` のイベントも同じ経路を通る (`stop_sending_sent_streams` に記録済みのストリームは早期 return するため対象外)。

`WtSession::poll_event` は FIN 付きの `WtEvent::StreamData` を返す前に `WtStream::mark_data_read` を呼ぶため、ドライバがこのイベントを処理する時点の受信状態は `DataRecvd` ではなく `DataRead` である。したがって次の 2 つが起きる。

- 現行のライブラリは受信状態を検証しないため、`DataRead` のストリームへ WT_MAX_STREAM_DATA が送信される (RFC 9000 Section 3.3 違反)
- 0149 の検証が入ると `grow_stream_recv_window` が `stream_state_error` を返し、`DriverState::abort_session_with_wt_error` が CONNECT ストリームへ RST_STREAM を送ってセッションを終了させる。双方向ストリームは送信パートが終端するまで `streams` に残るため、この経路は実際に踏む

拡張の判定は capsule 単位で行われる。`WtSession::send_stream_data` の 1 回の呼び出しは 1 個の WT_STREAM capsule を生成し、`WtEvent::StreamData` も capsule 1 個につき 1 件であるため、HTTP/2 の DATA フレーム分割 (送信バッファの 65535 bytes) は判定の回数と位置に影響しない。

FIN のイベントで閾値を下回る例は、ローカル広告値が既定 (`WtConfig::default()` の `initial_max_stream_data_bidi_remote` = 262144、閾値は 131072) で、ピアが 1 個の FIN 付き WT_STREAM capsule で 172144 bytes を送る場合である。ピアの送信上限が 262144 なので 1 回で送ることができ、このイベントで `recv_available` が 90000 になる。ドライバでは `WtServerRequest::accept` が広告 SETTINGS でローカル `WtConfig` を上書きするため、テストでこの状態を作るには広告値を 172144 bytes 以上 344288 bytes 未満 (2 × 172144) にする必要がある。広告値が 344288 bytes 以上だと閾値が 172144 bytes 以上になり、FIN のイベントでも下回らない。テストヘルパーの 64 KiB 広告ではピアの送信上限が 65536 になるため、この例のままでは 172144 bytes を送れない (送信上限超過)。

ピア開始 uni ストリームは FIN の `poll_event` で削除されるため `WtSession::stream` が `None` を返し、既に拡張しない。問題は送信パートが終端していない双方向ストリームに限る。

## 設計方針

- `DriverState::maybe_grow_stream_window` で、`WtSession::stream` が返す `WtStream` から `can_recv()` も取り出し、偽なら何もせず `Ok(())` を返す。`can_recv()` は `Recv` と `SizeKnown` で真、`DataRecvd` / `DataRead` / `ResetRead` で偽になるため、FIN 受信後 (`DataRead`) の拡張を抑止できる
- FIN を受信した後にウィンドウを拡張しても、ピアはそのストリームへデータを送れないため意味がない。抑止しても正常な通信のフロー制御は損なわれない
- しきい値 (`initial.div_ceil(2)`) と、開始主体ごとに `initial` を選ぶ処理は変更しない
- 判定は `grow_stream_recv_window` の呼び出し前に行い、`recv_max` の更新や capsule のエンコードを行わない
- 0149 の検証が入っても、ドライバは `Recv` 状態のストリームにしか `grow_stream_recv_window` を呼ばないためセッションは終了しない。`can_recv()` が真を返す `SizeKnown` は現行実装では到達せず、0149 の検証 (`Recv` 以外を拒否) との差は実害がない (0149 の「到達しない状態」を参照)
- アプリからの `stop_sending` は `DriverCmd::StopSending` の ack でエラーが返るだけでセッションを終了させないため、本 issue では扱わない

## 完了条件

- FIN 付きの `WtEvent::StreamData` を処理しても `WtSession::grow_stream_recv_window` が呼ばれず、WT_MAX_STREAM_DATA がピアへ送信されないこと。ドライバ内部の呼び出しは外部から観測できないため、ピア側で当該ストリームの `send_available()` が増えないことで固定する
- ピアが双方向ストリームへ FIN 付きデータを送るテストで、セッションが終了しないこと。CONNECT ストリームへの RST_STREAM は 0149 の検証が入って初めて発生するため、この確認は 0149 実装後に判別力を持つ前方互換のガードであり、本 issue 単独の判別条件は WT_MAX_STREAM_DATA が送信されないことである
- 送信パートが終端していない双方向ストリームで、ピアが FIN 付きデータを送ってもアプリが FIN を受け取れること
- `Recv` 状態のストリームへの従来のウィンドウ拡張が非回帰であること (`test_wt_local_bidi_window_grows_with_asymmetric_limits` / `test_wt_stream_window_grows_with_initial_one`)
- テストが追加され、`cargo test --all` が通過すること

## 解決方法

- `crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` に、`WtStream::can_recv()` が偽なら何もせず `Ok(())` を返す判定を追加した。FIN 付きの `WtEvent::StreamData` を処理する時点の受信状態は `DataRead` であり、`can_recv()` が偽になるため、ピアがそれ以上データを送れないストリームへ WT_MAX_STREAM_DATA を送らなくなった。判定は `grow_stream_recv_window` の呼び出し前に行うため、`recv_max` の更新と capsule のエンコードも行われない
- 併せて `WtSession::stream` の結果をタプルで分解していた箇所を let-else に整理し、`is_bidirectional()` と `can_recv()` の取り違えがコンパイルで通る形を解消した
- `draft-ietf-webtrans-http2-15` Section 5.2 (WebTransport ストリームの状態は QUIC ストリームの状態を mirror する) と RFC 9000 Section 3.3 / Section 19.10 (MAX_STREAM_DATA を送れるのは `Recv` 状態に限られる) を根拠としてコメントに明記した。`can_recv()` は `SizeKnown` でも真になるが、`SizeKnown` は現行実装では到達しないため、受信パートが終端したストリームがここで除外される
- `crates/tokio-http2/tests/test_webtransport.rs` に `test_wt_stream_window_not_grown_after_fin` を追加した。64 KiB を広告するサーバーへクライアントが閾値 (32768) を下回る 40 KiB を FIN 付きの 1 個の capsule で送り、サーバーが応答を返した後にピアの `send_available()` が `before - PAYLOAD_SIZE` のままであること (WT_MAX_STREAM_DATA が送信されていないこと)、アプリがデータと FIN を受け取れること、セッションが終了しないことを固定する。サーバーは応答を `fin = false` で送り、クライアント側のストリームが削除されて `send_available()` を読めなくなることを避けている
- 判定に判別力があることは、ガードを外した状態でこのテストが `send_available()` 90112 (拡張後) と 24576 (期待値) の不一致で失敗することで確認した
- `CHANGES.md` の `## develop` に `[FIX]` のエントリを追加した
