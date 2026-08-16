# PriorityFrame のフィールドを pub(crate) に変更する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-privatize-priority-frame
- Polished: {YYYY-MM-DD}

## 目的

`PriorityFrame` の全フィールドを `pub` から `pub(crate)` に変更し、意図しない外部構築を防止する。

## 現状

`PriorityFrame`（`src/frame.rs` の `PriorityFrame` 型）はコメントで「公開コンストラクタは提供しない」と明記されているが、全フィールドが `pub` である。この構造体は `decoder.rs` でのみ構築され、`encoder.rs` でのみ消費される。外部から構築されることは意図されていない。

RFC 9113 で PRIORITY フレームは非推奨であり、本実装でも受信のみを処理する。外部クレートが `PriorityFrame` を構築するユースケースは存在しない。

## 設計方針

- `PriorityFrame` の全フィールドを `pub` から `pub(crate)` に変更する
- 必要に応じて getter を追加する（現在は外部から読み取りも行われていないため不要の可能性が高い）

## 完了条件

- `PriorityFrame` のフィールドが `pub(crate)` に変更されていること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
