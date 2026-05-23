# `src/*/mod tests` を `tests/test_*.rs` に分離する

Created: 2026-05-23
Model: Opus 4.7

## 内容

CLAUDE.md「テストについて」の規約「単体テストのファイル名は `tests/test_<module>.rs` とし、`src/<module>.rs` に対応させること」に違反する `#[cfg(test)] mod tests` ブロックが複数ファイルに残っている。これらを順次 `tests/test_*.rs` に移管する。

## 対象 (確認済み主要箇所、他にもある可能性)

- `src/hpack/table.rs` 内 `#[cfg(test)] mod tests` (約 32 件のテスト)
- `src/hpack/dynamic_table.rs` 内 `#[cfg(test)] mod tests` (約 8 件)
- `src/validation.rs` 内 `#[cfg(test)] mod tests` (約 27 件)
- その他 `src/` 配下の `#[cfg(test)] mod tests` 全般

## 設計方針

- `tests/test_hpack_table.rs` / `tests/test_dynamic_table.rs` / `tests/test_validation.rs` に分離。
- 単体テストファイル内の `#[cfg(test)] mod` で更にカテゴリ分割可能。
- ただし `pub(crate)` 関数を直接叩いているテストは crate 外からアクセス不能なので、適切な内部公開設計 (もしくは `#[cfg(test)]` のまま残す) を検討。

## 完了条件

- [ ] CLAUDE.md 規約「`tests/test_<module>.rs`」に従ったファイル配置
- [ ] 既存テストカバレッジを落とさない
- [ ] CHANGES.md `### misc` に変更を追記

## 依存

なし
