# WtConfig のフィールドを private 化する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-privatize-wt-config
- Polished: {YYYY-MM-DD}

## 目的

`WtConfig` の全フィールドを private 化し、`Settings` や `Limits` と同様に getter/builder パターンに統一する。

## 現状

`WtConfig`（`src/webtransport.rs` の `WtConfig` 型）は全 6 フィールド（`initial_max_data`, `initial_max_stream_data_bidi_local`, `initial_max_stream_data_bidi_remote`, `initial_max_stream_data_uni`, `initial_max_streams_bidi`, `initial_max_streams_uni`）が `pub` であり、構築後に個別フィールドを外部から変更可能。

`Settings` や `Limits` は既に private フィールド + getter に移行済み（`CHANGES.md` の issue 0043, 0028 参照）。`WtConfig` はこの移行から取り残されている。

## 設計方針

- 全フィールドを private 化する
- 各フィールドの getter を追加する（`initial_max_data()` 等）
- `Default` 実装は維持する
- `examples/wt_server` が `WtConfig::default()` のみを使用しているため、サンプルコードへの影響はない

## 完了条件

- `WtConfig` の全フィールドが private 化されていること
- getter メソッドが追加されていること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
