# `__test_helpers` モジュール隔離と `mod bytes` の改名

Created: 2026-05-23
Model: Opus 4.7

## 内容

issue 0024 の /review-diff-code で指摘された設計大物のうち、以下 2 点を本 issue で対応する。

1. `HeaderField::from_validated_parts` の `#[cfg(feature = "__test_helpers")] pub` と `#[cfg(not(...))] pub(crate)` の **2 定義による本体重複** を解消する
2. `src/hpack/bytes.rs` (mod 名 `bytes`) が将来 issue 0013 で導入予定の `bytes` クレートと **import 衝突する命名** になっている

## 背景

- 0024 では `__test_helpers` cargo feature を新設し、PBT/fuzz から `from_validated_parts` を呼び出せるようにした。しかし関数本体を 2 箇所に重複定義する形になり、片方を直したらもう片方を直し忘れる潜在リスクがある。
- `src/hpack/bytes.rs` は `HeaderBytes` 型を含む crate 内部実装で、`pub(crate) mod bytes;` として参照されている。0013 で `bytes` クレートを導入すると `use crate::hpack::bytes` と `use ::bytes` の混乱が発生する。

## 設計方針

### `__test_helpers` の本体重複解消

- `from_validated_parts` の本体を private な `from_validated_parts_inner` に集約し、`#[cfg]` で可視性ラッパだけを切り替える。
- もしくは `#[cfg(feature = "__test_helpers")] pub mod __for_test { pub fn header_field_from_validated_parts(...) -> HeaderField {...} }` のようにモジュール単位で隔離する。

### `mod bytes` の改名

- `src/hpack/bytes.rs` を `src/hpack/header_bytes.rs` または `src/hpack/repr.rs` に改名する。
- 内部参照 (`src/hpack/mod.rs`, `src/hpack/table.rs`) を追従修正。

## 完了条件

- [ ] `HeaderField::from_validated_parts` の本体定義が 1 箇所に集約されている
- [ ] `src/hpack/bytes.rs` がリネームされ、`bytes` クレート (将来 issue 0013) と命名衝突しない
- [ ] 既存の全テスト・PBT・fuzz が通る
- [ ] CHANGES.md `### misc` に変更を追記

## 依存

- [[0013-refactor-bytes-payloads]] (pending、`bytes` クレート導入時に本 issue が前提)
