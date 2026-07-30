# webtransport モジュール内の is_tchar 重複を解消する

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/refactor-dedupe-is-tchar
- Polished: 2026-07-30

## 目的

`src/webtransport/init.rs` と `src/webtransport/protocols.rs` に完全に同一の `is_tchar` 関数が重複している問題を解消する。

## 現状

`src/webtransport/init.rs` と `src/webtransport/protocols.rs` に、RFC 9110 Section 5.6.2 の tchar 判定を行う `is_tchar(b: u8) -> bool` 関数がバイト単位で同一の実装として存在する。さらに `src/syntax.rs` にも tchar 判定の別実装が 2 つ（`is_token_char_lower` と `is_token_char_case_insensitive`）存在する。

## 完了条件

- `is_tchar` の実装が 1 箇所に集約されていること
- 全テストが通過すること

## 解決方法

`src/webtransport/` 内に共通のサブモジュール（例: `src/webtransport/syntax.rs`）を作成し、`is_tchar` を集約する。`init.rs` と `protocols.rs` から共通モジュールを参照するように変更する。`src/syntax.rs` の tchar 判定との統合も検討する。
