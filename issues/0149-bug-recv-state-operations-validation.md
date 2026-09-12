# 受信状態を問わず受信系操作が受理される

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-recv-state-operations-validation
- Polished: {YYYY-MM-DD}

## 目的

受信パートが `Recv` 状態でないストリームに対して、受信側を前提とする操作が受信状態を検証されず、RFC 9000 に反する capsule を送信する問題を修正する。`WtSession::stop_sending` / `send_max_stream_data` / `grow_stream_recv_window` は 0147 で方向 (受信パートの有無) の検証を追加したが、受信状態は見ていない。

RFC 9000 Section 3.3 は「The receiver of a stream sends MAX_STREAM_DATA frames (Section 19.10) and STOP_SENDING frames (Section 19.5).」「The receiver only sends MAX_STREAM_DATA frames in the "Recv" state.」とし、Section 19.10 も「A MAX_STREAM_DATA frame can be sent for streams in the "Recv" state; see Section 3.2.」とする。すなわち MAX_STREAM_DATA は受信側が `Recv` 状態でのみ送れる。`Recv` 状態でないストリームからの送信はこの制約に反する。

## 現状

`WtStream` の受信状態 (`WtStream::recv_state`) が `Recv` でなくても次の操作が受理される。

- `stop_sending` は受信パートが `ResetRead` の場合も WT_STOP_SENDING を送信できる
- `send_max_stream_data` は受信パートが `DataRecvd` / `DataRead` / `ResetRead` の場合も WT_MAX_STREAM_DATA を送信できる
- `grow_stream_recv_window` は `DataRecvd` / `DataRead` / `ResetRead` でも `recv_max` を更新し WT_MAX_STREAM_DATA を送信できる

ピアから WT_RESET_STREAM を受信した後は `ResetRead`、FIN 付きの WT_STREAM を受信した後は `DataRecvd`、その FIN イベントをアプリが受け取った後は `DataRead` になる。現在の 3 API はいずれも `WtStream::has_recv_part()` と `WtStream::stop_sending_sent()` しか見ないため、これらの状態をすり抜ける。

## 設計方針

- `has_recv_part()` の検証は 0147 の非回帰として維持し、そのうえで受信状態の検証を追加する。置き換えない
- `send_max_stream_data` / `grow_stream_recv_window` は受信パートの状態が `Recv` でない場合に `stream_state_error` を返す (RFC 9000 Section 3.3 / Section 19.10)。`WtStream::can_recv()` は `Recv` と `SizeKnown` の両方を許容するため、この判定に `!can_recv()` を使うと `SizeKnown` を許容して仕様から外れる。`SizeKnown` は現行実装では到達しないが、判定は `Recv` 以外の拒否と明示する
- `stop_sending` は受信パートが `ResetRead` の場合に `stream_state_error` を返す。RFC 9000 Section 3.3 は「A receiver MAY send a STOP_SENDING frame in any state where it has not received a RESET_STREAM frame -- that is, states other than "Reset Recvd" or "Reset Read".」とし、`DataRecvd` / `DataRead` からの送信は許容するため拒否しない (Section 19.5 と Section 3.5 は `Recv` / `SizeKnown` に限定する記述もあるが、本 issue は Section 3.3 の MAY に従う)
- 拒否時は `recv_max` の更新や capsule のエンコードを行わない。`grow_stream_recv_window` は現在 `recv_max` を更新してから `send_max_stream_data` を呼ぶため、状態検証は `recv_max` の更新前に置く
- 受信パートが `Recv` 状態の双方向ストリーム、および受信パートが `Recv` 状態の受信専用ストリーム (ピア開始 uni) への従来どおりの操作は受理する。受信専用ストリームが `DataRead` / `ResetRead` に達すると `WtStream::is_closed()` が真になり `WtSession::remove_if_closed` で削除されるため、その後の API は `stream_state_error` ではなく `invalid_stream_id` を返す。非回帰の対象は `Recv` 状態の受信専用ストリームに限る

### 到達しない状態

`ResetRecvd` と `SizeKnown` は現行実装では到達しない。`WtStream::recv_reset` は `ResetRead` へ直接遷移し、`WtStream::recv_data` は FIN 付き受信で `DataRecvd` へ直接遷移する (`SizeKnown` を経由しない)。テストの対象は `Recv` / `DataRecvd` / `DataRead` / `ResetRead` の 4 状態とする。

### ドライバ側の制約 (本 issue のスコープ外)

**この検証だけを実装すると tokio ドライバが正常な通信でセッションを異常終了させる回帰が入る。** `crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` は `dispatch_wt_event` の `WtEvent::StreamData` 分岐から呼ばれ、`recv_available < initial.div_ceil(2)` のとき `WtSession::grow_stream_recv_window` を呼ぶ。失敗すると `abort_session_with_wt_error` が CONNECT ストリームへ `RST_STREAM` を送りセッション全体を abort する。

`WtSession::poll_event` は `StreamData { fin: true }` を pop した時点で `WtStream::mark_data_read` を呼ぶため、ドライバが FIN イベントを処理する時点の受信状態は `DataRecvd` ではなく `DataRead` である。双方向ストリームは送信パートが終端でない限り `WtStream::is_closed()` が偽で `streams` に残るため、この経路は実際に踏む。既定の `WtConfig::default()` でも、ピアが双方向ストリームへ 172144 bytes を FIN 付きで送ると 65535 + 65535 + 41074 に分割され、最終 chunk 受信後の `recv_available` は 90000 < 131072 となって拡張が走る。

したがって、本 issue の検証を入れる前に、または同時に、ドライバが受信状態 `Recv` でないストリームへ `grow_stream_recv_window` を呼ばないようにする制御が必要である。この制御は本 issue のスコープ外とし、別途 issue 化して対応する。本 issue の完了条件はライブラリ単体のテストで検証できる範囲に限る。

## 完了条件

- `ResetRead` の双方向ストリームへの `stop_sending` が `stream_state_error` を返し、出力が生成されないこと
- `DataRecvd` と `DataRead` の双方向ストリームへの `send_max_stream_data` が `stream_state_error` を返し、出力が生成されないこと
- `DataRecvd` / `DataRead` / `ResetRead` の双方向ストリームへの `grow_stream_recv_window` が `stream_state_error` を返し、`recv_available` が変化しないこと
- `DataRecvd` / `DataRead` の双方向ストリームへの `stop_sending` は、RFC 9000 Section 3.3 が許容するため従来どおり受理されること
- `Recv` 状態の双方向ストリームと受信専用ストリーム (ピア開始 uni) への同操作は従来どおり動作すること
- 送信専用ストリーム (ローカル開始 uni) への同操作が 0147 の非回帰として `stream_state_error` を返し続けること
- テストが追加され、`cargo test --all` が通過すること
