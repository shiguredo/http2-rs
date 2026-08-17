# SKILL.md と README のドキュメントを最新化し、CHANGES.md の draft-14 参照を履歴として保全する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/update-skill-md-and-readme
- Polished: 2026-08-17

## 目的

`skills/shiguredo-http2/SKILL.md` のバージョン番号・ビルド方式・モジュールパスの古い記述を現状に合わせて修正し、`README.md` と `crates/nghttp2-sys/README.md` のビルド方式の記述も合わせて修正する。`CHANGES.md` の draft-14 参照は履歴エントリとして保全し、変更しない。

## 現状

1. `skills/shiguredo-http2/SKILL.md` のバージョン表記が `2026.1.0-canary.8` だが、`Cargo.toml` の実際のバージョンは `2026.1.0-canary.12`
2. `skills/shiguredo-http2/SKILL.md` の nghttp2-sys のビルド方式が `cmake` と記載されているが、実際は `shiguredo_cmake` に切り替え済み（`crates/nghttp2-sys/Cargo.toml` の build-dependencies と `crates/nghttp2-sys/build.rs` で確認できる）
3. 同一の古い `cmake` 記述が `README.md` と `crates/nghttp2-sys/README.md` にも残っている（後者はビルド方式の説明に加えて「必要なツール」リストにも `- cmake` がある）
4. `skills/shiguredo-http2/SKILL.md` の「既知の未対応 / 制限」が存在しない `src/connection/mod.rs` を参照している（実際の構成は `src/connection.rs` と `src/connection/headers.rs`）
5. `CHANGES.md` 内に draft-14 時代の参照が 4 箇所残っている（draft-14 対応のエコーサーバーサンプルのエントリ、`WtInit` / `WtConfig::apply_init` のエントリ、TLS 1.3 未達拒否のエントリ、draft-14 由来の暫定性注記のエントリ）。これらは変更履歴の記録であり、書き換えずに履歴エントリとして保全する

## 設計方針

- SKILL.md のバージョン番号を `2026.1.0-canary.12` に更新する
- SKILL.md の nghttp2-sys ビルド方式の記述を `shiguredo_cmake` に修正する（`crates/nghttp2-sys` の実際の実装に合わせる）
- README.md と crates/nghttp2-sys/README.md の `cmake` 記述も `shiguredo_cmake` に修正する。ただし crates/nghttp2-sys/README.md の「必要なツール」リストの `- cmake` は、`shiguredo_cmake` がプリビルト CMake バイナリを自動ダウンロードするため不要になり、削除する
- SKILL.md の「既知の未対応 / 制限」の `src/connection/mod.rs` 参照を実際の構成に合わせて修正する
- CHANGES.md の draft-14 参照（4 箇所）は履歴エントリとして保全し、変更しない

## 完了条件

- SKILL.md のバージョン番号が `Cargo.toml` と一致していること
- SKILL.md のビルド方式の記述が実際の実装（`crates/nghttp2-sys` の `shiguredo_cmake` 使用）と一致していること
- README.md と crates/nghttp2-sys/README.md のビルド方式の記述が実際の実装と一致していること（crates/nghttp2-sys/README.md の「必要なツール」リストから `- cmake` が削除されていること）
- SKILL.md の「既知の未対応 / 制限」のモジュールパス参照が実際の構成と一致していること
- CHANGES.md の draft-14 参照（4 箇所）が履歴エントリとして保全され、変更されていないこと
- `cargo test --workspace` が全件通過すること
