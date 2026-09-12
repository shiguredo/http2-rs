# 受信状態を問わず受信系操作が受理される

- Created: 2026-09-12
- Completed: 2026-09-12
- Branch: feature/fix-recv-state-operations-validation
- Polished: 2026-09-12

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

### 前提となるドライバ側の制御 (対応済み)

本 issue の検証だけを実装すると、tokio ドライバが正常な通信でセッションを異常終了させる回帰が入る。`crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` は `dispatch_wt_event` の `WtEvent::StreamData` 分岐から呼ばれ、`recv_available < initial.div_ceil(2)` のとき `WtSession::grow_stream_recv_window` を呼ぶ。失敗すると `abort_session_with_wt_error` が CONNECT ストリームへ `RST_STREAM` を送ってセッションを終了させる。

`WtSession::poll_event` は FIN 付きの `StreamData` を pop した時点で `WtStream::mark_data_read` を呼ぶため、ドライバが FIN のイベントを処理する時点の受信状態は `DataRecvd` ではなく `DataRead` である。双方向ストリームは送信パートが終端するまで `WtStream::is_closed()` が偽で `streams` に残るため、閾値を下回る FIN 付きデータを受信するとこの経路を踏む。

この制御は `issues/closed/0153-bug-driver-stream-window-growth-after-fin.md` で対応済みである。`DriverState::maybe_grow_stream_window` は `WtStream::can_recv()` が偽のストリームを拡張の対象から除外するため、受信状態が `Recv` でないストリームへ `grow_stream_recv_window` を呼ばない。したがって本 issue の検証を実装しても、この経路でセッションは終了しない。

ドライバが受信状態の影響を受ける API を呼ぶのは `maybe_grow_stream_window` の `grow_stream_recv_window` だけである。`maybe_grow_session_window` の `grow_recv_window` と `maybe_grow_max_streams` の `grow_max_streams` はストリームの受信状態を見ないため本 issue の検証対象外であり、アプリからの `stop_sending` は `DriverCmd::StopSending` が ack でエラーを返すだけでセッションを終了させない。

## 完了条件

- `ResetRead` の双方向ストリームへの `stop_sending` が `stream_state_error` を返し、出力が生成されないこと
- `DataRecvd` と `DataRead` の双方向ストリームへの `send_max_stream_data` が `stream_state_error` を返し、出力が生成されないこと
- `DataRecvd` / `DataRead` / `ResetRead` の双方向ストリームへの `grow_stream_recv_window` が `stream_state_error` を返し、`recv_available` が変化しないこと
- `DataRecvd` / `DataRead` の双方向ストリームへの `stop_sending` は、RFC 9000 Section 3.3 が許容するため従来どおり受理されること
- `Recv` 状態の双方向ストリームと受信専用ストリーム (ピア開始 uni) への同操作は従来どおり動作すること
- 送信専用ストリーム (ローカル開始 uni) への同操作が 0147 の非回帰として `stream_state_error` を返し続けること
- テストが追加され、`cargo test --all` が通過すること (受信状態の検証でドライバのテストが失敗しないことを含む)

## 解決方法

- `src/webtransport.rs` の `WtSession` に private ヘルパー `check_max_stream_data_recv_state` を追加し、受信状態が `Recv` でなければ `WtError::stream_state_error` を返すようにした。`WtStream::can_recv()` は `SizeKnown` も許容するため使わず、`WtStream::recv_state()` と `RecvState::Recv` の比較で判定する (RFC 9000 Section 3.3 / Section 19.10)
- `WtSession::send_max_stream_data` と `WtSession::grow_stream_recv_window` の両方でこのヘルパーを呼ぶようにした。`grow_stream_recv_window` では `recv_max` を更新する前に検証するため、拒否時にローカル状態が変化しない。既存の `has_recv_part()` (0147) と `stop_sending_sent()` (Section 6.6 の MUST NOT) の検証は置き換えず、そのまま維持している
- `WtSession::stop_sending` に、受信状態が `ResetRead` の場合の拒否を追加した。RFC 9000 Section 3.3 は STOP_SENDING を `Reset Recvd` / `Reset Read` 以外の状態で送れる (MAY) とするため、`DataRecvd` / `DataRead` では従来どおり受理する。`ResetRecvd` は現行実装では到達しないことをコメントに明記した
- 上記 3 API の doc コメントに拒否条件を追記した
- `tests/test_webtransport/integration.rs` に 9 テストを追加した。`DataRecvd` / `DataRead` / `ResetRead` の双方向ストリームへの `send_max_stream_data` と `grow_stream_recv_window` の拒否 (出力が生成されず、`grow_stream_recv_window` では `recv_available` も変化しない)、`ResetRead` への `stop_sending` の拒否 (送信済みフラグが立たない)、`DataRecvd` / `DataRead` への `stop_sending` の受理 (WT_STOP_SENDING がエンコードされる) を固定した。受信状態を作るヘルパー 3 個も併せて追加した
- `CHANGES.md` の `## develop` に `[FIX]` のエントリを追加した
