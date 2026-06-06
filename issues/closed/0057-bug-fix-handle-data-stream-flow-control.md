# handle_data でストリームレベルのフロー制御違反が接続エラーとして伝播する問題を修正する

- Priority: Medium
- Created: 2026-06-06
- Completed: 2026-06-06
- Model: DeepSeek V4 Pro
- Branch: feature/fix-handle-data-stream-flow-control

## 目的

`src/connection/mod.rs:994` の `stream.flow_control_mut().consume_recv(flow_control_size)?` が、ストリームレベルのフロー制御違反を接続エラーとして伝播させていた。RFC 9113 §6.9 はストリームレベルのフロー制御違反に対して RST_STREAM での応答を要求しているため、修正が必要。

## 優先度根拠

- RFC 9113 §6.9: "For flow-control errors at the stream level, the endpoint sends a RST_STREAM frame."
- 1 ストリームのウィンドウ超過が接続全体の切断を引き起こすのは過剰
- 同一ファイル内の `handle_window_update` (L1257-1264) では既に RST_STREAM に変換するパターンが実装されており、コード内の不整合を解消する
- issue 0051 の `/review-diff-code` で指摘された

## 現状

`src/connection/mod.rs:994`:

```rust
stream.state_machine_mut().recv_data(frame.end_stream)?;
stream.flow_control_mut().consume_recv(flow_control_size)?;
```

`FlowControl::consume_recv` (`src/flow_control.rs:109-112`) は常に `Error::connection_error(FlowControlError)` を返す。そのため `?` で伝播すると接続テアダウンになる。

対照的に、`handle_window_update` (L1257-1264) では:

```rust
if stream.flow_control_mut().recv_window_update(increment_u32).is_err() {
    self.reset_stream(frame.stream_id, ErrorCode::FlowControlError)?;
    return Ok(());
}
```

`.is_err()` で捕捉し `reset_stream` で RST_STREAM に変換している。

## 設計方針

`handle_window_update` の既存パターンに統一する。`consume_recv` のエラーを `.is_err()` で捕捉し、`self.reset_stream()` で RST_STREAM(FLOW_CONTROL_ERROR) を送信する。

## 完了条件

- `handle_data` のストリームレベル `consume_recv` エラーが接続エラーではなく RST_STREAM に変換される
- `handle_window_update` と一貫したエラーハンドリングパターンになる
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する

## 解決方法

1. `src/connection/mod.rs:994` の `stream.flow_control_mut().consume_recv(flow_control_size)?` を `.is_err()` で捕捉し、`self.reset_stream()` で RST_STREAM(FLOW_CONTROL_ERROR) に変換するように変更した。
2. `handle_window_update` (L1257-1264) と一貫したエラーハンドリングパターンに統一した。
3. `CHANGES.md` の `[FIX]` セクションにエントリを追加した。
