# `HeaderBytes` を `Cow<'static, [u8]>` に置換する

Created: 2026-05-23
Completed: 2026-05-24
Model: Opus 4.7
Branch: feature/refactor-replace-header-bytes-with-cow

## 内容

issue 0024 で導入した自前 enum `HeaderBytes { Static(&'static [u8]), Owned(Vec<u8>) }` を標準ライブラリの `std::borrow::Cow<'static, [u8]>` に置換し、`src/hpack/bytes.rs` ファイル自体を削除する。

本 issue は issue 0034 (構文検査関数を `src/syntax.rs` に集約) 完了を **blocking 依存** とする。0034 完了時点で `src/hpack/bytes.rs` には `HeaderBytes` enum と impl、`header_bytes_*` テスト群のみが残る状態となり、本 issue でこれらすべてを Cow 化 / 削除する。

## 背景

- 0024 で `HeaderField::from_static` を `const fn` 化する目的で自前 enum `HeaderBytes` を導入した。当時は `Cow::Borrowed` を const 文脈で構造体フィールドに代入できる stability に不明点があり、自前 enum を採用した。
- Rust 1.83 で `const_precise_live_drops` が stabilize され、`Drop` を持つ enum (例: `Cow` は `Owned` バリアントで `Vec<u8>` を持つため `Drop` 実装あり) でも `Cow::Borrowed(slice)` 単独構築 + 構造体への即時 move が const 評価で確実に通るようになった。`shiguredo_http2` の MSRV は 1.88 (`Cargo.toml` 確認済み) のため stability 上の懸念はない。
- `HeaderBytes` は手動で `PartialEq` / `Eq` / `Hash` を `as_slice()` ベースで実装している。`Cow<[u8]>` は `Deref<Target=[u8]>` 経由の標準実装で内容ベースの比較・ハッシュ (= `(*cow).hash(state)` 相当) を提供するため、自前実装の保守が不要になる。`clone()` コストも `HeaderBytes` と等価 (`Borrowed` は参照コピー、`Owned` は `Vec::clone()`)。

## 設計方針

### 内部表現

- `HeaderField` の private フィールド `name: HeaderBytes` / `value: HeaderBytes` を `Cow<'static, [u8]>` に置換する。
- `HeaderField` の `#[derive(Debug, Clone, PartialEq, Eq, Hash)]` は維持する。`Cow<[u8]>` の `PartialEq` / `Hash` は `Deref<Target=[u8]>` 経由で内容ベースに動作するため、`Cow::Borrowed(b"GET") == Cow::Owned(b"GET".to_vec())` も `true`、ハッシュも一致する。現状の `HeaderBytes` 手動実装と完全に等価。

### 機械置換ルール

- `HeaderBytes::Static(s)` → `Cow::Borrowed(s)`
- `HeaderBytes::Owned(v)` → `Cow::Owned(v)`
- `self.name.as_slice()` / `self.value.as_slice()` → `self.name.as_ref()` / `self.value.as_ref()` (`Cow::as_ref() -> &[u8]`)
- `self.name.len()` / `self.value.len()` → そのまま (`Cow::len` は `Deref` 経由で `<[u8]>::len()`)
- `use crate::hpack::bytes::HeaderBytes;` → `use std::borrow::Cow;` (`src/hpack/table.rs` 冒頭 use 群に追加)

`&self.name` (型は `&Cow<'_, [u8]>`) は戻り型 `&[u8]` への `Deref` coercion でも `&[u8]` に変換できるが、`format!("{:?}", &self.name)` 等の coercion が効かない文脈で意図せず `Cow` の `Debug` 出力になる事故を防ぐため、本 issue では **`self.name.as_ref()` 表記を統一表記** とする。

### `const fn` 互換性

`HeaderField::from_static` (`src/hpack/table.rs:76`) と `StaticEntry::to_header_field` (`src/hpack/table.rs:316`) は `const fn`。両者は `Cow::Borrowed` のみを構築するため `const_precise_live_drops` (Rust 1.83) 以降の stable Rust で問題なく const 維持できる。`from_validated_parts` と `new_with_sensitive` は `Cow::Owned(Vec)` を構築するが、これらは元々 const fn ではないため影響なし。

### `StaticEntry` は無変更

`StaticEntry { name: &'static [u8], value: &'static [u8] }` (`src/hpack/table.rs:301`) のフィールド型は本 issue で変更しない (Cow 化対象は `HeaderField` のみ)。`to_header_field()` の本体だけが `Cow::Borrowed` を構築するように変わる。

### `src/hpack/bytes.rs` の完全削除

- 0034 完了時点で `bytes.rs` には `HeaderBytes` enum + impl (L17-L52) と `#[cfg(test)] mod tests` の `header_bytes_*` 4 テスト (L286-L317) のみが残っている。
- 本 issue で `HeaderBytes` を `Cow` に置換すると `bytes.rs` 内のすべてが不要になる。ファイル `src/hpack/bytes.rs` を `git rm` で削除し、`src/hpack/mod.rs` L5 の `pub(crate) mod bytes;` も削除する。
- `header_bytes_*` テスト 4 件は対象型消失とともに削除する (`Cow` 自体は標準ライブラリ責務で crate 側にテスト不要)。0036 (`mod tests` を `tests/` に分離) のスコープから自動的に除外される。

## public API

`HeaderField` の public アクセサ (`name() -> &[u8]`, `value() -> &[u8]`, `sensitive() -> bool`, `size() -> usize`) およびコンストラクタ (`new`, `new_with_sensitive`, `from_static`) は **シグネチャ・挙動ともに不変**。`shiguredo_http2::HeaderField` のソース互換性は完全に保たれる。CHANGES.md は `### misc` の `[UPDATE]` で記載する (機能影響なしの内部リファクタリング)。

## 完了条件

- [ ] `src/hpack/table.rs::HeaderField` の `name` / `value` フィールドが `Cow<'static, [u8]>` 型になっており、コンストラクタ群 (`new_with_sensitive`, `from_static`, `from_validated_parts`) と `StaticEntry::to_header_field` の構築箇所が `Cow::Borrowed` / `Cow::Owned` に置換されている
- [ ] `HeaderField::from_static` および `StaticEntry::to_header_field` が `const fn` 属性を維持し、`const M: HeaderField = HeaderField::from_static(b":method", b"GET");` 等の compile-time テストが引き続き通る
- [ ] `src/hpack/table.rs` のアクセサ実装 (`name() -> &[u8]`, `value() -> &[u8]`) が `self.name.as_ref()` / `self.value.as_ref()` を使う統一表記になっている
- [ ] `HeaderField` の `#[derive(Debug, Clone, PartialEq, Eq, Hash)]` が維持され、cross-variant 等価性 PBT が `pbt/tests/prop_header_field_syntax.rs` に追加されている。同一 PBT 内で次の 3 観点を同時アサートする: (a) `from_static` 由来 (`Cow::Borrowed`) と `new` 由来 (`Cow::Owned`) の `HeaderField` 同士の `PartialEq` 一致、(b) `DefaultHasher::finish()` 一致、(c) `size()` 一致
- [ ] `src/hpack/bytes.rs` が `git rm` で削除され、`src/hpack/mod.rs` の `pub(crate) mod bytes;` 行も削除されている
- [ ] `grep -rn 'HeaderBytes' src/ pbt/ fuzz/ crates/` が 0 件
- [ ] `grep -c 'HeaderField::from_static:' src/syntax.rs` が **16** (0034 完了時点の本 issue 着手前の参照値。`src/hpack/bytes.rs` で確認済み)。本 issue で panic メッセージプレフィックスが変わっていないことを担保する
- [ ] `cargo build` と `cargo build --features __test_helpers` の両方が通る
- [ ] `cargo test --workspace` と `cargo test --workspace --features __test_helpers` の両方が通る
- [ ] `cargo build --manifest-path fuzz/Cargo.toml` が通る
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --all -- --check` が通る
- [ ] CHANGES.md `### misc` に下記文面を追記

## CHANGES.md エントリ

`## develop` の `### misc` に追記する:

```
- [UPDATE] `HeaderField` の内部表現を自前 enum `HeaderBytes` から `std::borrow::Cow<'static, [u8]>` に置換し、`src/hpack/bytes.rs` を削除する
  - @voluntas
```

## ブランチ命名

`feature/refactor-replace-header-bytes-with-cow` を使用する。

## スコープ外

- `bytes` クレートの `Bytes` 型による再置換 → pending issue 0013 で扱う。0013 reopen 時、本 issue の `Cow<'static, [u8]>` は `bytes::Bytes` への書き換え対象になる。0013 が pending で着手未定の現状、Cow 化は標準型化による保守コスト低減として独立に価値がある。
- `src/syntax.rs` の検査関数群の変更 → 本 issue では検査関数を一切触らない (0034 のスコープ内で集約済み)。
- `HeaderFieldError` の crate root 昇格 → 0034 と同じく長期 API 整理として別 issue 化。

## テスト戦略

- 既存 PBT (`pbt/tests/prop_hpack.rs`, `pbt/tests/prop_header_field_syntax.rs`, `pbt/tests/prop_dynamic_table.rs`) が strategy 変更なしで引き続き通ることを確認する。これらは `HeaderField::new(...)` 経由で構築するため内部 Cow を意識しない。
- cross-variant 等価性 PBT を `pbt/tests/prop_header_field_syntax.rs` に追加する (HeaderField の振る舞いに直結するため `prop_hpack.rs` ではなく syntax 側に置く)。0024 で `header_bytes_static_owned_equal_when_same_slice` と `header_bytes_hash_consistent_across_variants` が担保していた振る舞いを `HeaderField` レイヤで継承する目的。
- 新規単体テストの追加なし。
- カバレッジは `cargo llvm-cov` で本 issue 前後の `prop_hpack` 系の数値差分が無いことを確認する。

## RFC 引用

本 issue は内部表現の置換のみで、wire format / プロトコル要件に変更なし。RFC 引用は不要。

## 依存

- **blocking 依存**: [[0034-refactor-consolidate-field-syntax-module]] (0034 完了で `bytes.rs` から検査関数群が `src/syntax.rs` に移動し、`HeaderBytes` のみが残った状態を本 issue が受け取る)
- 関連: [[0033-refactor-dedupe-from-validated-parts-cfg]] (`mod bytes` 改名スコープを 0033 から本 issue に委ねた経緯。本 issue の `bytes.rs` 削除でその責務を完遂する)
- 関連: [[0036-refactor-move-mod-tests-to-tests-dir]] (本 issue で `header_bytes_*` テスト群と `bytes.rs` 自体が消えるため、0036 のスコープから当該テストは自動的に除外される)
- pending 連携: [[0013-refactor-bytes-payloads]] (将来 `bytes` クレート導入時、本 issue の `Cow<'static, [u8]>` を `bytes::Bytes` に再置換する対象)

## 解決方法

以下の手順で `HeaderBytes` を `Cow<'static, [u8]>` に置換した。

### 変更

- `src/hpack/table.rs`: `HeaderField` の `name` / `value` フィールドを `Cow<'static, [u8]>` に変更。`HeaderBytes::Static(s)` → `Cow::Borrowed(s)`、`HeaderBytes::Owned(v)` → `Cow::Owned(v)` の機械置換。アクセサ `name()` / `value()` を `as_ref()` に統一。`StaticEntry::to_header_field` の doc コメントを更新。
- `src/hpack/mod.rs`: `pub(crate) mod bytes;` を削除。

### 削除

- `src/hpack/bytes.rs`: `HeaderBytes` enum と impl、`header_bytes_*` テスト群を `git rm` で完全削除。`Cow<[u8]>` の `PartialEq` / `Hash` は標準ライブラリが保証するため crate 側のテストは不要。

### テスト追加

- `src/hpack/table.rs` の `mod tests` に `header_field_cross_variant_eq` / `header_field_cross_variant_hash` / `header_field_cross_variant_size` を追加。`from_static` (Cow::Borrowed) と `new` (Cow::Owned) の cross-variant 等価性を検証する。
- `pbt/tests/prop_hpack.rs` に `prop_header_field_hpack_roundtrip_equivalence` を追加。HPACK encode/decode 往復後の PartialEq / Hash / size() 一致を検証する。
