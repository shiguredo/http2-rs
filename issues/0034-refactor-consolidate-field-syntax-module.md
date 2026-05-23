# 構築時検査関数を syntax モジュールに集約する

Created: 2026-05-23
Model: Opus 4.7

## 内容

issue 0024 の /review-diff-code で指摘された設計大物のうち、以下 2 点を本 issue で対応する。

1. `src/hpack/bytes.rs` の `check_*_const` と `src/hpack/table.rs` の `validate_*` が **同一の検査規則を別実装** で持っている (D2: 検査二重メンテ)
2. `src/validation.rs` が `src/hpack/table.rs::validate_*` を `pub(crate)` 公開で呼ぶことで **validation → hpack の依存逆転** が発生している (D3)

## 背景

- 0024 で `from_static` の `const fn` 化のため `bytes.rs` に const fn 検査を、`HeaderField::new` 用に `table.rs` に runtime 検査をそれぞれ実装した。両者は同じ規則 (field-name token、field-value SP/HTAB、:status 3DIGIT 等) を別書きしているため、片方の修正漏れで仕様乖離が発生する潜在リスクがある。M5 同値性 PBT がセーフティネットとして機能するが、根本的な解消ではない。
- field-name / field-value / 疑似ヘッダー検査は **HTTP/2 セマンティクス** の責務であり、HPACK (RFC 7541) の責務ではない。`hpack::table` モジュールに置かれているのは `HeaderField` 型の都合に過ぎず、論理的には独立モジュールが妥当。

## 設計方針

- 新規モジュール `src/syntax.rs` (もしくは `src/field_syntax.rs`) を作成し、以下を集約する:
  - const fn 版検査 (`check_field_name`, `check_field_value`, `check_pseudo_header`)
  - 同じロジックの runtime 版 (`validate_field_name`, `validate_field_value`, `validate_pseudo_header`)
- 可能なら const fn 1 つで両用に共通化する (`-> Result<(), &'static str>` 版を const fn で書き、`panic!` 版と Result 版で薄くラップ)。Rust 1.88 では const fn 内で `match` や `if let` が使えるため一定程度実現可能。
- `src/hpack/table.rs::HeaderField::new` と `src/validation.rs::check_field` の両方が `syntax` モジュールを呼ぶ依存方向に統一する。
- 結果として `validation` → `hpack` の依存逆転を解消する。

## 完了条件

- [ ] 検査関数が単一モジュール (`src/syntax.rs` 等) に集約されている
- [ ] `src/hpack/bytes.rs` から検査関数が消える (あるいは syntax モジュールに移動)
- [ ] `src/validation.rs` が `src/hpack/table.rs` の検査関数を直接呼ばない
- [ ] M5 同値性 PBT (`pbt/tests/prop_header_field_syntax.rs`) が引き続き通る
- [ ] 既存の全テスト・PBT・fuzz が通る
- [ ] CHANGES.md `### misc` に変更を追記

## 依存

なし
