# `#[allow(...)]` を `#[expect(...)]` に置き換える

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fmt-replace-allow-with-expect
- Polished: {YYYY-MM-DD}

## 目的

AGENTS.md の規約「`#[allow(...)]` を使わないこと（例外なし）」に違反している 3 箇所を `#[expect(...)]` に置き換える。

## 現状

以下の 3 箇所で `#[allow(...)]` が使用されている:

1. `crates/nghttp2-sys/src/lib.rs` — `#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types, dead_code, clippy::all)]`（crate-level 属性、手書き）
2. `crates/shiguredo_nghttp2/src/types.rs` — `#[allow(deprecated)]`（`from_u8` 関数）
3. `crates/tokio-http2/tests/interop.rs` — `#[allow(clippy::single_match, clippy::collapsible_if)]`（テストコード）

## 設計方針

- `#![allow(...)]` → `#![expect(...)]` に置き換える
- `#[allow(...)]` → `#[expect(...)]` に置き換える
- `expect` の reason には、なぜその lint を許可するのかの理由を日本語で明記する
- `nghttp2-sys` の `bindings.rs` は自動生成ファイルのため、`bindings.rs` 内の `#[allow(...)]` は対象外とする

## 完了条件

- 上記 3 箇所が `#[expect(...)]` に置き換わっていること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
- `cargo test --workspace` が全件通過すること
