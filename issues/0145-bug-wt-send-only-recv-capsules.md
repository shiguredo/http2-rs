# 送信専用ストリームへの WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED が受理される

- Created: 2026-09-10
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-send-only-recv-capsules
- Polished: {YYYY-MM-DD}

## 目的

ローカル開始 uni ストリーム (送信専用) 宛にピアから WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED capsule を受信した際、それらが受理されてイベントが送出される問題を修正する。両 capsule はいずれも送信側が送るものであり、送信側を持たない送信専用ストリームに対しては valid でないため、draft-ietf-webtrans-http2-15 Section 6.2 / Section 6.9 が要求する WT_STREAM_STATE_ERROR を返す必要がある。

## 現状

`src/webtransport.rs` の `WtSession::handle_capsule` は、WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED の検証に `WtStream::can_recv()` を使う。`can_recv()` は `RecvState` のみを見てストリームの方向を見ないため、送信専用であるローカル開始 uni ストリームでも `WtStream::new` が `recv_state: RecvState::Recv` を設定するため true を返す。

- WT_RESET_STREAM は `reliable_size` が `recv_offset` (0) と一致すれば `stream.can_recv()` が true のため受理され、`WtEvent::StreamReset` が送出される
- WT_STREAM_DATA_BLOCKED も `stream.can_recv()` が true のため受理される

WT_STREAM 側はローカル開始 uni を `WtSession::handle_stream_data` で個別に拒否するようになったが、これらの中継 capsule は方向を見ていない。根本原因は `WtStream` がストリームに受信パートがあるかどうかを表現していないことにある。

## 設計方針

- `WtStream` に受信パートの有無 (双方向またはピア開始) を判定する手段を設け、送信専用ストリームへの受信系 capsule を `WtError::stream_state_error` で拒否する
- WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED の検証で `can_recv()` だけでなく方向も確認する
- ローカル開始 bidi ストリームとピア開始ストリームは従来どおり受理する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- ローカル開始 uni ストリーム ID への WT_RESET_STREAM 受信が `stream_state_error` を返すこと
- ローカル開始 uni ストリーム ID への WT_STREAM_DATA_BLOCKED 受信が `stream_state_error` を返すこと
- ローカル開始 bidi ストリームおよびピア開始ストリームへの同 capsule は従来どおり動作すること
- テストが追加され、`cargo test --all` が通過すること
