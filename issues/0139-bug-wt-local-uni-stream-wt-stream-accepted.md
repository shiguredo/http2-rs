# ローカル開始 uni ストリームへのピア WT_STREAM が受理される

- Created: 2026-09-09
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-local-uni-stream-wt-stream
- Polished: 2026-09-10

## 目的

WebTransport セッションで、ローカルが開始した単方向ストリーム (送信専用) の ID 宛にピアから WT_STREAM capsule を受信した際、それが受理されて `StreamData` イベントが送出される問題を修正する。RFC 9000 Section 2.1 では単方向ストリームは開始側のみが送信でき、draft-ietf-webtrans-http2-15 Section 6.4 は valid でない状態のストリームへの WT_STREAM に WT_STREAM_STATE_ERROR を要求している。

## 現状

`src/webtransport.rs` の `WtSession::handle_stream_data` は、`is_new_stream = !self.streams.contains_key(&stream_id)` が true の場合にのみ `is_peer_initiated` を検証してローカル開始 ID を拒否する。ローカル開始 uni ストリームは `WtSession::open_uni_stream` で `streams` に登録されるため、ピアがその ID へ WT_STREAM を送ると `is_new_stream` は false になり、`is_peer_initiated` 検証を素通りする。

`WtStream::new` は全ストリームで `recv_state: RecvState::Recv` を初期化するため、送信専用であるローカル開始 uni ストリームでも `WtStream::recv_data` が成功し、`StreamData` イベントが送出される。ピアはローカル開始 uni の ID (例: サーバー開始なら 3, 7, ...) を推測できるため到達可能である。

## 設計方針

- `handle_stream_data` で、既存ストリームに対しても受信可能な方向かを検証する。ローカル開始 uni ストリーム (送信専用) への WT_STREAM は `WtError::stream_state_error` で拒否する
- ローカル開始 bidi ストリームは受信可能なので従来どおり受理する
- ピア開始ストリームは従来どおり受理する
- テストを追加し、`cargo test --all` が通過することを確認する

## 完了条件

- ローカル開始 uni ストリーム ID への WT_STREAM 受信が `stream_state_error` を返すこと
- ローカル開始 bidi ストリーム ID への WT_STREAM 受信は従来どおり `StreamData` を生成すること
- ピア開始ストリームへの WT_STREAM 受信は従来どおり動作すること
- テストが追加され、`cargo test --all` が通過すること
