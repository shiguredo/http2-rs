# SendError 型を削除する

- Priority: Medium
- Created: 2026-06-11
- Polished: {Polished}
- Model: deepseek-v4-pro
- Branch: feature/refactor-remove-send-error

## 目的

未統合のまま公開 API としてエクスポートされている `SendError` 型を削除する。

## 現状の問題

`src/send_error.rs:6-7`:

> 現時点では `Connection` の送信 API は従来の `Error` 型を使用しており、
> この型は未統合。統合は送信 API のリファクタリング時に行う。

`src/lib.rs:54`:

```rust
pub use send_error::SendError;
```

`Connection` の送信系 API (`send_data`, `send_response`, `reset_stream` 等) は全て `Error` 型を返しており、`SendError` はコードベース内のどこからも使われていない（`tests/test_send_error.rs` は存在するが、製品コードでは未使用）。

未完成の型を公開 API に置くことは利用者を混乱させ、将来の互換性保証の負債になる。

## 完了条件

- `src/send_error.rs` が削除されていること
- `src/lib.rs` から `pub use send_error::SendError;` が削除されていること
- `tests/test_send_error.rs` が削除されていること（当該テストファイルが存在する場合）
- `cargo build --workspace` が成功すること
- `cargo test --workspace` が成功すること
- CHANGES.md `## develop` に `[CHANGE]` エントリを追加すること

## 解決方法

1. `src/send_error.rs` を削除
2. `src/lib.rs` の `pub use send_error::SendError;` を削除
3. `tests/test_send_error.rs` を削除（存在する場合）

削除後に `cargo build --workspace` と `cargo test --workspace` を実行し、SendError への依存が無いことを確認する。

将来的に送信エラー型の統合が必要になった場合は、新たに設計し直す。現状の `SendError` を温存する理由はない。

## 参照

- `src/send_error.rs:1-54` — 削除対象ファイル
- `src/lib.rs:54` — `pub use send_error::SendError;`
- `issues/closed/0019-chore-remove-dead-code.md` — 過去の死にコード削除事例
- issue 0029 (エラー型分割) — `SendError` が導入された経緯
