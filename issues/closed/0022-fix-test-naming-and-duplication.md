# テスト命名規則違反を修正する

- Priority: Low
- Created: 2026-05-14
- Model: deepseek-v4-pro
- Completed: 2026-05-26
- Branch: feature/fix-test-naming

## 目的

`tests/rfc7541.rs` が AGENTS.md の命名規則 (`tests/test_<module>.rs`) に違反している。`tests/test_hpack.rs` にリネームする。

## 優先度根拠

命名規則違反は 1 ファイルのみ。テストの正確性・CI に影響しない。

## 現状

`tests/rfc7541.rs` は `src/hpack/` モジュールに対応する RFC 7541 Appendix A のテストベクターに基づく統合テスト。AGENTS.md の「単体テストのファイル名は `tests/test_<module>.rs` とし、`src/<module>.rs` に対応させること」に違反している。

## 設計方針

`tests/rfc7541.rs` を `tests/test_hpack.rs` にリネームする（`git mv`）。内容は変更しない。

## 変更対象ファイル

- `tests/rfc7541.rs` → `tests/test_hpack.rs` (リネーム)

## 完了条件

- `tests/rfc7541.rs` が存在しない
- `tests/test_hpack.rs` が存在する
- `cargo test --workspace` が通る

## 備考: 既に解決済みの項目

本 issue は元々以下も含んでいたが、いずれも解決済みまたは別 issue で対応するため除外した:

- PBT と重複する単体テスト (src/ 内の #[cfg(test)]) → issue 0036 で tests/ に移管済み
- PBT ディレクトリモジュールのサブモジュール分割 → issue 0048 で扱ったが、規約のサブモジュール対応分割が `prop_frame` に原理的に適用できないため対応せず close した

## 解決方法

`tests/rfc7541.rs` を `tests/test_hpack/rfc7541.rs` に移動し、`tests/test_hpack/main.rs` に `mod rfc7541;` を追加した。

`tests/test_hpack.rs` へのリネームは既存の `tests/test_hpack/` ディレクトリモジュールと衝突するため、ディレクトリモジュールのサブモジュールとして統合する方式を採用した。AGENTS.md の「src/\<module\>/ のようにディレクトリモジュールの場合」の規約に準拠する。
