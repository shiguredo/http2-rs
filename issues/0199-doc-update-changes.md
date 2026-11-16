# CHANGES.md の draft-16 移行記載と CODEBASE.md の変更履歴禁止を整合させる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/update-changes
- Polished: {YYYY-MM-DD}

## 目的

`CHANGES.md` の `## develop` には draft-16 への移行 (`SETTINGS_WT_ENABLED` の 0x2b67 変更、`WT_MAX_*` の厳密増加規則の変更) を記載したエントリがない。既存の draft-15 参照エントリは、当時の仕様を記録した履歴として保全し、書き換えない (open 0118 の方針)。また `CODEBASE.md` は「この指示がなくなるまでは変更履歴を CHANGES.md に残さないこと」とする一方で、CHANGES には 31 エントリがある。変更履歴の運用と、未記載の draft-16 移行を整合させる。

## 現状

- `CHANGES.md` の `## develop` は draft-15 参照の履歴エントリのみで、draft-16 への wire 互換性変更を記載したエントリがない。既存の draft-15 参照は履歴として保全する。
- `### misc` が空のまま。
- `send_max_stream_data` のエントリは「現在の受信上限より小さい値は拒否」とだけ書き、実装の「同値も拒否」と一致しない。後方互換のない挙動変更として `[CHANGE]` に分けるか、記述を修正する必要がある。
- `CODEBASE.md` の変更履歴禁止指示と CHANGES の実データが矛盾している。

## 設計方針

- CODEBASE.md の指示を削除・更新するか、「既存記載は維持し、新規変更は残さない」などの意図を明文化する。
- draft-15 → draft-16 の wire 互換性変更を、新しい `[CHANGE]` / `[FIX]` エントリとして `## develop` に追加する。既存の draft-15 参照エントリは書き換えない。
- `send_max_stream_data` の記述を「増加させない値」に修正し、同値拒否を明記する。
- 空の `### misc` を削除するか、必要なエントリを追加する。

## 完了条件

- `CODEBASE.md` の変更履歴指示と `CHANGES.md` の運用が矛盾しないこと。
- draft-15 → draft-16 の wire 互換性変更を記載した新しいエントリが `## develop` にあること。
- 既存の draft-15 参照エントリが履歴として変更されていないこと。
- `send_max_stream_data` の記述が実装と一致していること。
- 空の `### misc` が整理されていること。
- `cargo fmt --all -- --check` / `cargo test --workspace` はコード変更がないため不要だが、issue ファイルの整合性を確認すること。
