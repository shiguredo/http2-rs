# 受信専用のピア開始 uni ストリームへの送信系操作が方向検証されない

- Created: 2026-09-12
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-receive-only-stream-send-validation
- Polished: 2026-09-12

## 目的

ピア開始 uni ストリーム (ローカルから見て受信専用) に対して、送信側を前提とする操作と capsule が方向検証されず、仕様違反の送信・応答が行われる問題を修正する。RFC 9000 Section 2.1 では単方向ストリームは開始側のみが送信でき、受信専用ストリームへの STOP_SENDING / MAX_STREAM_DATA は Section 19.5 / Section 19.10 が STREAM_STATE_ERROR を要求している。

## 現状

`WtStream` には 0145 で `has_recv_part()` が追加されたが、送信パートの有無を判定する手段がなく、送信系の検証は `can_send()` (送信状態のみ) に依存している。そのためピア開始 uni ストリーム (受信専用。`WtStream::new` では `send_state: SendState::Ready` のため `can_send()` が true) に対して次の 4 経路が通ってしまう。

- `WtSession::send_stream_data` が `can_send()` のみで判定し、受信専用ストリームへ WT_STREAM を送信できる (ピアは WT_STREAM_STATE_ERROR で拒否する)
- `WtSession::reset_stream` が `can_send()` のみで判定し、受信専用ストリームへ WT_RESET_STREAM を送信できる
- `WtSession::handle_capsule` の WT_STOP_SENDING が `stop_sending_received()` のみを検証し、受信専用ストリームでも受理して `can_send()` が true のため WT_RESET_STREAM を自動応答する (RFC 9000 Section 19.5 違反)
- `WtSession::handle_capsule` の WT_MAX_STREAM_DATA が `stop_sending_sent()` のみを検証し、受信専用ストリームでも `update_send_max` で送信上限を更新する (RFC 9000 Section 19.10 違反)

## 設計方針

- `WtStream` に送信パートの有無 (双方向またはローカル開始) を判定する `has_send_part()` を追加する (0145 の `has_recv_part()` と対にする)
- `send_stream_data` / `reset_stream` で `has_send_part()` を検証し、受信専用ストリームへの送信系操作を `WtError::stream_state_error` で拒否する
- `handle_capsule` の WT_STOP_SENDING / WT_MAX_STREAM_DATA の検証で `has_send_part()` も確認し、受信専用ストリームへの受信を `WtError::stream_state_error` で拒否する。WT_STOP_SENDING を拒否する場合は WT_RESET_STREAM の自動応答も行わない
- `stop_sending` (WT_STOP_SENDING の送信) と `send_max_stream_data` / `grow_stream_recv_window` は受信パートを前提とする操作であり、受信専用ストリームでも正当なため従来どおり受理する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- ピア開始 uni ストリームへの `send_stream_data` が `stream_state_error` を返すこと
- ピア開始 uni ストリームへの `reset_stream` が `stream_state_error` を返すこと
- ピア開始 uni ストリームへの WT_STOP_SENDING 受信が `stream_state_error` を返し、WT_RESET_STREAM が応答されないこと
- ピア開始 uni ストリームへの WT_MAX_STREAM_DATA 受信が `stream_state_error` を返すこと
- ローカル開始 uni / ローカル開始 bidi / ピア開始 bidi への同操作は従来どおり動作すること
- ピア開始 uni ストリームへの `stop_sending` / `send_max_stream_data` / `grow_stream_recv_window` は従来どおり動作すること
- テストが追加され、`cargo test --all` が通過すること
