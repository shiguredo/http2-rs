# `HeaderField::from_validated_parts` の cfg 二定義を解消する

Created: 2026-05-23
Completed: 2026-05-24
Model: Opus 4.7
Branch: feature/refactor-dedupe-from-validated-parts-cfg

## 内容

`src/hpack/table.rs` の `HeaderField::from_validated_parts` が **本体同一・可視性のみ異なる `#[cfg]` 排他 2 定義** (L93-L111) になっている (`#[cfg(feature = "__test_helpers")] #[doc(hidden)] pub` と `#[cfg(not(...))] pub(crate)`)。本 issue ではこれを `pub(crate)` 単一定義に統一し、テスト向けの公開層を既存の `src/__test_helpers.rs` モジュールへ移譲する。これにより `table.rs` から cfg 分岐 1 箇所が消え、公開 API 表面 (`HeaderField::from_validated_parts` の `pub` 露出) も `__test_helpers` モジュールに一本化される。

本 issue は issue 0024 の /review-diff-code 指摘「片方を直したらもう片方を直し忘れる潜在リスク」への対応だが、実体は 4 行の構造体リテラルへの cfg 切替であり「重複」より「責務分離」の観点で扱う。

なお、当初 issue 名にあった「`mod bytes` 改名」は 0035 で `HeaderBytes` 自体が `Cow<'static, [u8]>` 置換により削除され、0034 で `check_*_const` 群が `src/syntax.rs` に移管されるため、本 issue 完了後の 0034 → 0035 の自然な経路で `src/hpack/bytes.rs` 自体が消滅する。よって本 issue では改名作業を行わない。

## 設計方針

### `table.rs` 側

- L93-L111 の cfg 排他 2 定義を削除し、`pub(crate) fn from_validated_parts(name: Vec<u8>, value: Vec<u8>, sensitive: bool) -> Self` の単一定義に統合する。本体ロジック (`HeaderBytes::Owned(name)` / `HeaderBytes::Owned(value)` / `sensitive`) は変更しない。
- 0024 の解決方法 (`issues/closed/0024-...md` L18) では `#[doc(hidden)] pub` を採用したが、本 issue で公開層を `__test_helpers` モジュールへ移譲することで `table.rs` 側を `pub(crate)` に下げる根拠が成立する。`#[doc(hidden)]` は不要 (`pub(crate)` は rustdoc に出ない)。

### `__test_helpers.rs` 側

- 以下のラッパを追加する:

```rust
/// HPACK decoder 経路の `HeaderField::from_validated_parts` を PBT/fuzz から呼べるよう公開する
///
/// 検査をバイパスして `HeaderField` を構築するため、型不変条件を破壊しうる。
/// 本番利用禁止。
pub fn header_field_from_validated_parts(
    name: Vec<u8>,
    value: Vec<u8>,
    sensitive: bool,
) -> crate::hpack::HeaderField {
    crate::hpack::HeaderField::from_validated_parts(name, value, sensitive)
}
```

- 命名規則: 既存 `check_field_name_const_result` 群は「panic-catch して `Result<(), String>` を返す」セマンティクスを示すため接尾辞 `_result` を付けている。本ラッパは単純再エクスポートで panic-catch しないため接尾辞は付けない。型名 + メソッド名のスネークケースで `header_field_from_validated_parts` を採用する。
- `__test_helpers.rs` モジュール先頭 doc コメント (L3-L11) に、再エクスポート系ラッパが含まれる旨を 1 行追記する。

### crate 外呼び出しの追従

`HeaderField::from_validated_parts` を直接呼んでいる以下の crate 外コードを書き換える。

| ファイル | 行 | 件数 |
|---|---|---|
| `pbt/tests/prop_validation.rs` | 226, 502, 523, 545, 587, 607, 626 | 7 |
| `fuzz/fuzz_targets/fuzz_validation.rs` | 27 | 1 |
| `fuzz/fuzz_targets/fuzz_hpack_roundtrip.rs` | 29 | 1 |

書き換え例 (`fuzz/fuzz_targets/fuzz_validation.rs`):

```rust
// before
use shiguredo_http2::{HeaderField, validation::{validate_request_headers, ...}};
// ...
let headers: Vec<HeaderField> = input.headers.iter()
    .map(|h| HeaderField::from_validated_parts(h.name.clone(), h.value.clone(), false))
    .collect();

// after
use shiguredo_http2::{HeaderField, __test_helpers::header_field_from_validated_parts,
    validation::{validate_request_headers, ...}};
// ...
let headers: Vec<HeaderField> = input.headers.iter()
    .map(|h| header_field_from_validated_parts(h.name.clone(), h.value.clone(), false))
    .collect();
```

`pbt/tests/prop_validation.rs` も同様に `use shiguredo_http2::__test_helpers::header_field_from_validated_parts;` を追加し、各呼び出しを自由関数呼び出しに変える。

### crate 内呼び出しの追従

crate 内 (decoder / dynamic_table / validation / connection / table.rs の `#[cfg(test)] mod tests`) は `pub(crate)` のままで呼べるため **書き換え不要**。`src/hpack/table.rs:899` の `header_field_from_validated_parts_skips_check` テストも `#[cfg(test)] mod tests` 配下なので無変更。

## 完了条件

- [ ] `src/hpack/table.rs` の cfg 排他 2 定義 (L93-L111) が消え、`pub(crate) fn from_validated_parts` の単一定義になっている
- [ ] `src/hpack/table.rs` L87-L92 の doc コメントが「crate 外からは `__test_helpers::header_field_from_validated_parts` ラッパ経由で呼ぶ」旨に更新されている
- [ ] `src/__test_helpers.rs` に `pub fn header_field_from_validated_parts` が追加されている
- [ ] `src/__test_helpers.rs` のモジュール doc に再エクスポート系ラッパが含まれる旨が追記されている
- [ ] `pbt/tests/prop_validation.rs` 7 箇所、`fuzz/fuzz_targets/fuzz_validation.rs` 1 箇所、`fuzz/fuzz_targets/fuzz_hpack_roundtrip.rs` 1 箇所がラッパ経由に書き換わっている
- [ ] `grep -rn 'HeaderField::from_validated_parts' pbt/ fuzz/` が 0 件を返す (crate 外直接呼び出しの残存検出)
- [ ] `cargo build` と `cargo build --features __test_helpers` の両方が通る
- [ ] `cargo test --workspace` と `cargo test --workspace --features __test_helpers` の両方が通る
- [ ] `cargo build --manifest-path fuzz/Cargo.toml` が通る
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` が通る
- [ ] `git mv issues/0033-refactor-test-helpers-module-and-bytes-mod-name.md issues/0033-refactor-dedupe-from-validated-parts-cfg.md` でファイル名を内容と整合させる
- [ ] `issues/0035-refactor-replace-header-bytes-with-cow.md` の依存セクションから `[[0033-...]] (mod 名整理と同時対応推奨)` 記述を削除し、`mod bytes` 削除を 0035 のスコープに含める旨を 1 行追記する
- [ ] CHANGES.md `### misc` に下記文面を追記

## CHANGES.md エントリ

`## develop` の `### misc` に追記する:

```
- [UPDATE] `HeaderField::from_validated_parts` の cfg 排他 2 定義を解消し、テスト向け公開層を `__test_helpers::header_field_from_validated_parts` に集約する
  - @voluntas
```

## ブランチ命名

`feature/refactor-dedupe-from-validated-parts-cfg` を使用する。CLAUDE.md にブランチ命名規則の明文記述はないが、リポジトリの既存実例は `feature/change-...` 系のみ。本 issue 以降の category `refactor` (0034, 0035, 0036, 0039) では `feature/refactor-...` プレフィックスを新規採用する宣言として本 issue で先行使用する。

## スコープ外

- `mod bytes` 改名 → 0034 + 0035 で `src/hpack/bytes.rs` の中身が消失するため自然解消
- `HeaderBytes` の Cow 化 → 0035 で対応
- `check_*_const` 群の syntax モジュール集約 → 0034 で対応

## テスト戦略

新規テスト追加なし。`cargo test --workspace --features __test_helpers` で既存 PBT (`prop_validation`, `prop_header_field_syntax`) と fuzz_targets がラッパ経由でも従来通り通ることを確認する。

## RFC 引用

本 issue は内部リファクタリングであり RFC 引用は不要。

## 依存

- なし
- 関連: [[0034-refactor-consolidate-field-syntax-module]], [[0035-refactor-replace-header-bytes-with-cow]]
- pending 連携: [[0013-refactor-bytes-payloads]] (`bytes` クレート導入時、`from_validated_parts` の `Vec<u8>` 引数を `Bytes` 化する対象)

## 解決方法

- `src/hpack/table.rs` の `HeaderField::from_validated_parts` を `pub(crate)` 単一定義に統一する。`#[cfg(feature = "__test_helpers")] #[doc(hidden)] pub` 版と `#[cfg(not(...))] pub(crate)` 版の cfg 排他 2 定義を削除し、本体ロジックは保持
- doc コメントを更新し、crate 外からは `__test_helpers::header_field_from_validated_parts` ラッパ経由で呼ぶ旨を明示
- `src/__test_helpers.rs` に `pub fn header_field_from_validated_parts(name: Vec<u8>, value: Vec<u8>, sensitive: bool) -> HeaderField` を追加し、`HeaderField::from_validated_parts` を pub(crate) のまま PBT / fuzz から呼べるよう公開層を一本化
- モジュール doc に再エクスポート系ラッパが含まれる旨を追記
- `pbt/tests/prop_validation.rs` の 7 箇所、`fuzz/fuzz_targets/fuzz_validation.rs` の 1 箇所、`fuzz/fuzz_targets/fuzz_hpack_roundtrip.rs` の 1 箇所をラッパ呼び出しに書き換え
- issue ファイル名を `0033-refactor-dedupe-from-validated-parts-cfg.md` にリネームし、`issues/0035-refactor-replace-header-bytes-with-cow.md` の依存参照を新 slug に更新
- `cargo build`、`cargo build --features __test_helpers`、`cargo test --workspace --features __test_helpers`、`cargo build --manifest-path fuzz/Cargo.toml`、`cargo clippy --workspace --all-targets --features __test_helpers -- -D warnings`、`cargo fmt --check` の全てが通ることを確認
