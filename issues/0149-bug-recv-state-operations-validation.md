# 受信終端状態のストリームへの受信系操作が受理される

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-recv-state-operations-validation
- Polished: {YYYY-MM-DD}

## 目的

受信パートが終端状態 (Reset Recvd / Reset Read / Data Recvd / Data Read) となったストリームに対して、受信側を前提とする操作が状態検証されず、RFC 9000 の送信状態制約に反する capsule を送信する問題を修正する。`WtSession::stop_sending` / `send_max_stream_data` / `grow_stream_recv_window` は 0147 で方向 (受信パートの有無) の検証を追加したが、受信状態は見ていない。

## 現状

`WtStream` の受信パートが終端状態でも次の操作が受理される。

- 双方向ストリームでピアから WT_RESET_STREAM を受信した後 (Reset Recvd / Reset Read) の `stop_sending` が WT_STOP_SENDING を送信できる
- 双方向ストリームで受信が完了した後 (Data Recvd / Data Read) や Reset Recvd / Reset Read の `send_max_stream_data` / `grow_stream_recv_window` が WT_MAX_STREAM_DATA を送信できる

RFC 9000 Section 3.3 は「The receiver only sends MAX_STREAM_DATA frames in the "Recv" state」「A receiver MAY send a STOP_SENDING frame in any state where it has not received a RESET_STREAM frame」とし、Section 19.10 は MAX_STREAM_DATA を "Recv" 状態のストリームに送れるとする。終端状態からの送信はこれらの制約に反する。

## 設計方針

- `stop_sending` は受信パートが Reset Recvd / Reset Read の場合に `stream_state_error` を返す。`send_max_stream_data` / `grow_stream_recv_window` は受信パートが Recv 状態でない場合に `stream_state_error` を返す (draft-ietf-webtrans-http2-15 Section 5.2)
- 拒否時は `recv_max` の更新や capsule のエンコードを行わない
- 受信パートが Recv 状態の双方向ストリームと受信専用ストリーム (ピア開始 uni) への従来どおりの操作は受理する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- Reset Recvd / Reset Read の双方向ストリームへの `stop_sending` が `stream_state_error` を返し、出力が生成されないこと
- Data Recvd / Data Read / Reset Recvd / Reset Read の双方向ストリームへの `send_max_stream_data` が `stream_state_error` を返し、出力が生成されないこと
- 同状態の `grow_stream_recv_window` が `stream_state_error` を返し、`recv_available` が変化しないこと
- 受信状態の双方向ストリームと受信専用ストリームへの同操作は従来どおり動作すること
- テストが追加され、`cargo test --all` が通過すること
