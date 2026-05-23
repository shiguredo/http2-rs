# PBT ファイル命名規約違反を解消する

Created: 2026-05-23
Model: Opus 4.7

## 内容

CLAUDE.md「テストについて」の規約「PBT のファイル名は `pbt/tests/prop_<module>.rs` とし、`src/<module>.rs` に対応させること」「`src/<module>/` のようにディレクトリモジュールの場合は `pbt/tests/prop_<module>/main.rs` にサブモジュール対応で分割すること」に違反する PBT ファイルを是正する。

## 対象

- `pbt/tests/prop_header_field_syntax.rs` (issue 0024 で追加): `src/hpack/bytes.rs` と `src/hpack/table.rs` の 2 モジュール横断のため、対応する単一モジュールが存在しない。
- `pbt/tests/prop_hpack.rs` / `pbt/tests/prop_dynamic_table.rs`: `src/hpack/` ディレクトリモジュール配下なので、本来は `pbt/tests/prop_hpack/main.rs` のサブモジュール構造が望ましい。

## 設計方針

- issue 0034 (検査関数を syntax モジュールに集約) で `src/syntax.rs` が新設されれば、`pbt/tests/prop_syntax.rs` に rename することで命名規約に整合する。
- それまでは暫定的に `pbt/tests/prop_hpack_syntax.rs` に rename し、`pbt/tests/prop_hpack/` 配下サブモジュール化を検討。
- `prop_hpack.rs` / `prop_dynamic_table.rs` も `pbt/tests/prop_hpack/main.rs` 構造への移行を併せて検討。

## 完了条件

- [ ] CLAUDE.md 規約に従った PBT ファイル配置
- [ ] 既存テストカバレッジを落とさない
- [ ] CHANGES.md `### misc` に変更を追記

## 依存

- [[0034-refactor-consolidate-field-syntax-module]] (syntax モジュール集約と同時対応推奨)
