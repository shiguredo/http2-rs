# WtSession::streams から閉じたストリームを削除してメモリリークを修正する

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/fix-wt-stream-memory-leak
- Polished: 2026-07-30

## 目的

`WtSession::streams` の HashMap からストリームが一切削除されず、長寿命セッションでメモリが無限に増大するバグを修正する。

## 現状

`src/webtransport/mod.rs` の `WtSession` は `streams: HashMap<WtStreamId, WtStream>` にストリームを insert するが、remove するコードがどこにもない。`WtStream::is_closed()` が true になっても HashMap に残り続ける。

draft-ietf-webtrans-http2-15 Section 6.7 でストリーム数制限は累積（closed も含む）と規定されているが、これは ID 空間の管理であり、閉じたストリームの状態オブジェクトを保持し続ける必要はない。ID 空間の管理は `next_*_stream_id` と `flow_control` が担っている。

## 設計方針

draft-ietf-webtrans-http2-15 Section 5.2 は「QUIC が ACK 受信で状態遷移する箇所を、WebTransport は即座に遷移する」と規定する（HTTP/2 の順序配送により ACK が不要なため）。この規定に従い、終端状態への遷移を即座に行う:

- 送信側: `send_data(fin=true)` 呼び出し時点で `DataSent` → `DataRecvd` へ即座に遷移。`send_reset()` 呼び出し時点で `ResetSent` → `ResetRecvd` へ即座に遷移
- 受信側: `recv_data(fin=true)` 呼び出し時点で `SizeKnown` → `DataRecvd` へ即座に遷移（順序配送により全データ到着済み）。`DataRecvd` → `DataRead` は `poll_event()` で `StreamData { fin: true }` を pop した時点で遷移。`recv_reset()` 呼び出し時点で `ResetRecvd` → `ResetRead` へ即座に遷移

単方向ストリームでは片側の状態機械が動作しないため、`is_closed()` の定義を方向性に応じて変更する:

- 双方向: `send_state.is_terminal() && recv_state.is_terminal()`（既存）
- 送信専用単方向（ローカル開設 uni）: `send_state.is_terminal()` のみで閉じたとみなす
- 受信専用単方向（ピア開設 uni）: `recv_state.is_terminal()` のみで閉じたとみなす

## 完了条件

- 双方向・単方向の両方で、ストリームが完全に閉じた後に `streams` HashMap から削除されること
- 大量のストリームを開閉してもメモリが増大しないこと
- 削除後もフロー制御の累積カウントが正しく動作すること

## 解決方法

1. `WtStream` の送信側状態遷移を追加する: `send_data(fin=true)` で直接 `DataRecvd` へ、`send_reset()` で直接 `ResetRecvd` へ遷移させる（Section 5.2 の即座遷移）
2. `WtStream` の受信側状態遷移を追加する: `recv_data(fin=true)` で直接 `DataRecvd` へ遷移。`recv_reset()` で直接 `ResetRead` へ遷移
3. `poll_event()` で `StreamData { fin: true }` を pop した際に、対象ストリームの `RecvState` を `DataRecvd` → `DataRead` へ遷移させ、`is_closed()` なら `self.streams.remove(&stream_id)` を呼ぶ
4. `WtStream::is_closed()` を方向性に応じて変更する: 単方向ストリームは動作する側の終端のみで判定する
5. 削除トリガーを以下の 3 箇所に配置する:
   - `handle_capsule` の `WtStream` / `WtResetStream` / `WtStopSending` 処理後（リセット経由のクローズ）
   - `poll_event()` の `StreamData { fin: true }` 遷移後（FIN 経由の正常クローズ。step 3 と同一箇所）
   - `send_stream_data(fin=true)` / `reset_stream()` の呼び出し後（送信専用 uni の正常クローズ）
