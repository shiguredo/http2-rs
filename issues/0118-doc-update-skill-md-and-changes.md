# SKILL.md と CHANGES.md のドキュメントを更新する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/doc-update-skill-and-changes
- Polished: {YYYY-MM-DD}

## 目的

`skills/shiguredo-http2/SKILL.md` のバージョン番号と古い記述を修正し、`CHANGES.md` の draft-14 参照を最新化する。

## 現状

1. `skills/shiguredo-http2/SKILL.md` のバージョン表記が `2026.1.0-canary.8` だが、`Cargo.toml` の実際のバージョンは `2026.1.0-canary.12`
2. `skills/shiguredo-http2/SKILL.md` の nghttp2-sys のビルド方式が `cmake` と記載されているが、実際は `shiguredo_cmake` に切り替え済み（`CHANGES.md:240`）
3. `CHANGES.md` 内に draft-14 時代の参照が 3 箇所残っている:
   - `CHANGES.md:129` — `draft-ietf-webtrans-http2-14 対応のエコーサーバーサンプル`
   - `CHANGES.md:137` — `draft-ietf-webtrans-http2-14 §4.3 / §4.3.2`
   - `CHANGES.md:217` — `draft-ietf-webtrans-http2-14 §7`

## 設計方針

- SKILL.md のバージョン番号を `2026.1.0-canary.12` に更新する
- SKILL.md の nghttp2-sys ビルド方式の記述を `shiguredo_cmake` に修正する
- CHANGES.md の draft-14 参照は履歴エントリとして残す意図であれば注記を追加する。更新する場合は draft-15 に書き換える

## 完了条件

- SKILL.md のバージョン番号が `Cargo.toml` と一致していること
- SKILL.md のビルド方式の記述が実際の実装と一致していること
- CHANGES.md の draft 参照の扱いが決定され、対応されていること
