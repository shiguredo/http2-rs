# WT_STREAM_DATA_BLOCKED 受信時のストリーム状態チェックを仕様に合わせる

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/fix-wt-stream-data-blocked-state-check
- Polished: 2026-07-30

## 目的

`WtSession::handle_capsule` の `WtStreamDataBlocked` 処理が、仕様の MUST 要件より甘い状態チェックになっている問題を修正する。

## 現状

`src/webtransport/mod.rs` の `handle_capsule` で `Capsule::WtStreamDataBlocked` を受信した際、`!stream.can_recv() && !stream.can_send()`（送受信双方が終端）の場合のみエラーを返している。

draft-ietf-webtrans-http2-15 Section 6.9 は "A stream error of type WT_STREAM_STATE_ERROR MUST be sent if a WT_STREAM_DATA_BLOCKED capsule is received for a stream that is not in a valid state" と規定する。WT_STREAM_DATA_BLOCKED は送信側が送る capsule であり、受信側（このエンドポイント）の受信状態が終端ならエラーにすべき。

## 完了条件

- 受信側が終端状態のストリームに WT_STREAM_DATA_BLOCKED を受信した場合に `WT_STREAM_STATE_ERROR` が返ること
- 存在しないストリーム ID への WT_STREAM_DATA_BLOCKED で `WT_STREAM_STATE_ERROR` が返ること
- 単体テストが追加されていること

## 解決方法

`handle_capsule` の `WtStreamDataBlocked` 分岐で、`if let Some(stream)` による silent ignore を廃止する。ストリームが存在しない場合も `WT_STREAM_STATE_ERROR` を返す（存在しないストリームは "valid state" ではないため）。ストリームが存在する場合は `!stream.can_recv()` の場合にエラーを返すように条件を変更する。
