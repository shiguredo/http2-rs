# 送信専用ストリームへの WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED が受理される

- Created: 2026-09-10
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-send-only-recv-capsules
- Polished: 2026-09-12

## 目的

ローカル開始 uni ストリーム (送信専用) 宛にピアから WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED capsule を受信した際、それらが受理される問題を修正する。WT_RESET_STREAM では `WtEvent::StreamReset` も誤って送出される。両 capsule はいずれも送信側が送るものであり、送信側を持たない送信専用ストリームに対しては valid でない (RFC 9000 Section 19.4 / Section 19.13)。draft-ietf-webtrans-http2-15 Section 6.2 / Section 6.9 は有効な状態にないストリームへの受信時に WT_STREAM_STATE_ERROR を返すことを要求している。

## 現状

`src/webtransport.rs` の `WtSession::handle_capsule` は、WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED の検証に `WtStream::can_recv()` を使う。`can_recv()` は `RecvState` のみを見てストリームの方向を見ないため、送信専用であるローカル開始 uni ストリームでも `WtStream::new` が `recv_state: RecvState::Recv` を設定するため true を返す。

- WT_RESET_STREAM は `reliable_size` が `recv_offset` (0) と一致すれば `stream.can_recv()` が true のため受理され、`WtEvent::StreamReset` が送出される
- WT_STREAM_DATA_BLOCKED も `stream.can_recv()` が true のため受理される (イベント送出はない)

WT_STREAM 側はローカル開始 uni を `WtSession::handle_stream_data` で個別に拒否するようになったが、これらの capsule の検証は方向を見ていない。根本原因は、`WtStream` が `bidirectional` / `locally_initiated` フィールドで方向を保持しているものの、受信パートの有無を判定する手段がなく、`can_recv()` が `RecvState` だけを見て方向を区別しないことにある。

## 設計方針

- `WtStream` に `bidirectional` / `locally_initiated` から受信パートの有無 (双方向またはピア開始) を判定するメソッドを追加し、WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED の検証で `can_recv()` だけでなく方向も確認して、送信専用ストリームへの受信を `WtError::stream_state_error` で拒否する
- ローカル開始 bidi ストリームとピア開始ストリームは従来どおり受理する
- WT_STOP_SENDING と WT_MAX_STREAM_DATA は送信専用ストリームでも受信が正当であるため、従来どおり受理する (今回の拒否対象に含めない)
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- ローカル開始 uni ストリーム ID への WT_RESET_STREAM 受信が `stream_state_error` を返し、`WtEvent::StreamReset` が送出されないこと
- ローカル開始 uni ストリーム ID への WT_STREAM_DATA_BLOCKED 受信が `stream_state_error` を返すこと
- ローカル開始 bidi ストリームおよびピア開始ストリームへの同 capsule は従来どおり動作すること
- ローカル開始 uni ストリームへの WT_STOP_SENDING と WT_MAX_STREAM_DATA は従来どおり受理され、送信停止要求・送信上限更新が機能すること
- テストが追加され、`cargo test --all` が通過すること
