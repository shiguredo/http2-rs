# `HeaderBytes` を `Cow<'static, [u8]>` に置換する

Created: 2026-05-23
Model: Opus 4.7

## 内容

issue 0024 で導入した `enum HeaderBytes { Static(&'static [u8]), Owned(Vec<u8>) }` を、標準ライブラリの `std::borrow::Cow<'static, [u8]>` に置換することを検討する。

## 背景

- 0024 で `from_static` の `const fn` 化のため自前 enum `HeaderBytes` を導入した。手動で `PartialEq` / `Eq` / `Hash` を実装し、`as_slice()` / `len()` メソッドを提供している。
- `Cow<'static, [u8]>` は標準ライブラリで同等の表現を提供し、`PartialEq` / `Eq` / `Hash` / `Deref<Target=[u8]>` が自動で正しく実装される。
- `Cow::Borrowed(slice)` は const 評価可能なので `from_static` の const fn 化を維持できる。

## 設計方針

- `HeaderField` の内部表現を `HeaderBytes` から `Cow<'static, [u8]>` に置換する。
- 手動実装の `PartialEq` / `Eq` / `Hash` は不要になる。
- `HeaderBytes::Static(s)` → `Cow::Borrowed(s)`、`HeaderBytes::Owned(v)` → `Cow::Owned(v)` に機械的に置換。
- 0033 と関連: `mod bytes` 削除と同時に対応すれば良い。

## トレードオフ

- メリット: 標準型なので将来の保守コストが下がる、`PartialEq` 自動実装で同値性破綻リスクが減る、`Cow` の API 群が利用可能。
- デメリット: `Cow` は `enum` であり性能特性は同等、特に問題なし。`std::borrow::Cow` への依存は標準ライブラリ範囲内で 0 依存ポリシーに違反しない。

## 完了条件

- [ ] `HeaderField` 内部表現が `Cow<'static, [u8]>` になっている
- [ ] `src/hpack/bytes.rs` の `HeaderBytes` enum と手動実装が削除されている
- [ ] 既存の全テスト・PBT・fuzz が通る
- [ ] CHANGES.md `### misc` に変更を追記

## 依存

- [[0033-refactor-test-helpers-module-and-bytes-mod-name]] (mod 名整理と同時対応推奨)
