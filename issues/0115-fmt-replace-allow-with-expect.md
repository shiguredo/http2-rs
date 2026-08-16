# `#[allow(...)]` を `#[expect(...)]` に置き換える（発火しない lint は削除する）

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-replace-allow-with-expect
- Polished: 2026-08-16

## 目的

shiguredo-rust 規約「`#[allow(...)]` を使わないこと（例外なし）。lint 警告を抑制する必要があるときは必ず `#[expect(...)]` を使うこと」に違反している箇所を修正する。`#[allow(...)]` では、その lint 項目がなくなったり、コードの修正によって不要になったときに気づけないため。

## 現状

以下の箇所で `#[allow(...)]` が使用されている（bindings.rs の自動生成分を除く計 11 個 = 以下の 7 箇所）:

1. `crates/nghttp2-sys/src/lib.rs` — crate-level `#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types, dead_code, clippy::all)]`（手書き）
2. `crates/shiguredo_nghttp2/src/types.rs` の `FrameType::from_u8` — `#[allow(deprecated)]`
3. `crates/tokio-http2/tests/interop.rs` — crate-level `#![allow(clippy::collapsible_match)]` / `#![allow(clippy::collapsible_if)]` / `#![allow(clippy::while_let_loop)]` / `#![allow(clippy::for_kv_map)]` / `#![allow(clippy::single_match)]` の 5 個
4. `crates/tokio-http2/tests/interop.rs` の `stress_tests` モジュール — `#[allow(clippy::single_match, clippy::collapsible_if)]`
5. `crates/tokio-http2/tests/client_server.rs` — crate-level `#![allow(clippy::collapsible_match)]`
6. `crates/tokio-http2/tests/test_webtransport.rs` — crate-level `#![allow(clippy::collapsible_match, clippy::collapsible_if)]`
7. `crates/tokio-nghttp2/tests/client_server.rs` — crate-level `#![allow(clippy::collapsible_match)]`

`crates/nghttp2-sys/src/bindings.rs` は rust-bindgen による自動生成ファイル（26 箇所の `#[allow(clippy::unnecessary_operation, clippy::identity_op)]` を含む）であり、直接編集しないため対象外とする。

## 設計方針

- `#[allow(...)]` → `#[expect(...)]` に置き換える。`expect` の reason には、なぜその lint を許可するのかの理由を日本語で明記する
- **`crates/nghttp2-sys/src/lib.rs` の crate-level 属性は一律の expect 化ができない**。bindgen 生成コードで実際に発火するのは `non_upper_case_globals` / `non_camel_case_types` の 2 つのみであり、`non_snake_case` / `dead_code` / `clippy::all` は expect 化すると `unfulfilled_lint_expectations` 警告になる。したがって:
  - `non_upper_case_globals` / `non_camel_case_types` は `#![expect(...)]` に置き換える
  - `non_snake_case` / `dead_code` / `clippy::all` は expect 化せずに削除する（bindings.rs のアイテムレベル allow に抑制された clippy lint も含め、allow を外した実測で警告が出ない lint の抑制は不要のため）
- 上記の発火状況は現行の bindings.rs に対する実測結果である。`--features overwrite` による bindgen 再生成で bindings.rs が変わった場合は再検証すること
- `crates/shiguredo_nghttp2/src/types.rs` の `FrameType::from_u8` は deprecated な `Priority` / `PushPromise` バリアントを参照するため、`#[expect(deprecated, reason = "...")]` に置き換え可能
- テストファイルの `#![allow(clippy::...)]` は、該当 lint が実際に発生する場合のみ `#![expect(clippy::..., reason = "...")]` に置き換える。発生しない lint は削除する。調査手順: allow を外して `cargo clippy --workspace --all-targets` を実行し、警告が出た lint は expect 化、出なかった lint は削除する
  - 実測では interop.rs の 5 lint はすべて発火（expect 化）、`tokio-http2/tests/client_server.rs` の `collapsible_match` は発火（expect 化）、`test_webtransport.rs` は `collapsible_if` のみ発火（`collapsible_match` は削除）、`tokio-nghttp2/tests/client_server.rs` の `collapsible_match` は発火しない（削除）。ただし、テストコードの修正により発火状況は変わりうるため、最終的には clippy 実行結果に従うこと
- `bindings.rs` は対象外

## 完了条件

- 上記 7 箇所・計 11 個の `#[allow(...)]` が `#[expect(...)]` への置き換え、または発火しない lint の削除によって解消されている
- bindings.rs を除く `crates/` 配下に `#[allow]` が残っていない（`grep -rn "#!\[allow\|#\[allow" crates/ --include="*.rs" | grep -v "src/bindings.rs"` で 0 件）
- `crates/nghttp2-sys` を `cargo clippy -p nghttp2-sys -- -D warnings` で検査したときに警告がないこと
- `CHANGES.md` の `## develop` の `### misc` サブセクションに `[UPDATE]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過すること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
- `cargo check --manifest-path fuzz/Cargo.toml` が通過すること
