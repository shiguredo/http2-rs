# WtFlowControl の初期値に上限チェックを追加する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-flow-control-init-validation
- Polished: {YYYY-MM-DD}

## 目的

`WtFlowControl::new()` に `MAX_STREAMS_LIMIT`（2^60）の上限チェックを追加し、`WtConfig` 経由の直接構築時に不正な初期値が注入されるのを防ぐ。

## 現状

`WtFlowControl::new()`（`src/webtransport/flow_control.rs` の `WtFlowControl` 型）は `max_streams_bidi_local` 等のストリーム数引数を上限チェックなしで受け取っている。`update_max_streams()` は受信時の上限チェック（`MAX_STREAMS_LIMIT`）を行っているが、初期値は `WtConfig` の `u64` フィールドから直接渡されるため、`WtConfig` を手動構築した場合に 2^60 超過の値を注入できる。

`Settings` 経由の WT 系パラメータは `u32` で制約されているため現状は安全だが、`WtConfig` 経由の直接構築では防御的検証がない。

## 設計方針

- `WtFlowControl::new()` に `MAX_STREAMS_LIMIT`（2^60）の上限チェックを追加する
- 上限超過時は `WtError`（`WtErrorKind::FlowControlError` 等）を返す
- `update_max_streams()` と同様の上限チェックを初期化時にも適用する

## 完了条件

- `WtFlowControl::new()` で上限超過時にエラーが返ること
- 既存のテストが全件通過すること
- 上限チェックの単体テストが追加されていること
