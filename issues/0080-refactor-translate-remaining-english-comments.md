# 残りの英語コメント (RFC 7541 セクション名 / doc コメント / SAFETY 他) の日本語化を検討する

- Priority: Low
- Created: 2026-06-12
- Polished: 2026-07-31
- Model: Opus 4.7
- Branch: feature/refactor-translate-remaining-english-comments

## 目的

issue 0076 (`refactor-translate-english-comments`、英語コメントの日本語翻訳の明示列挙箇所) のスコープ外として分離された残りの英語コメント (RFC 7541 セクション名コメント、`///` doc コメント、`SAFETY:` 系プレフィックス付きコメント等) について、日本語化の方針を確定し必要なら翻訳する。

0076 は「実コードの説明文の英語コメント」だけを対象とした最小修正であり、「RFC 仕様の概念名」「rustdoc 生成内容に影響する doc コメント」「Rust 慣用プレフィックス付きコメント」のような判断が分かれる領域は本 issue で個別判断する。

## 優先度根拠

- CLAUDE.md 規約「コメントは全て日本語にすること」を厳密に守るかどうかは、RFC 仕様の概念名 (例: `Indexed Header Field (Section 6.1)`) を「固有名詞として英語のまま残す」か「概念説明として日本語化する」かの方針判断が必要
- `///` doc コメントは rustdoc の出力結果 (`cargo doc`) に影響するため、日本語化すると公開ドキュメントが日本語になる (国際的なオープンソース利用の観点で要検討)
- `SAFETY:` / `TODO:` / `FIXME:` 等のプレフィックスは Rust エコシステムの慣用表現であり、プレフィックス自体は英語維持が妥当 (説明文の日本語化は別問題)
- 機能挙動には影響しないため Priority: Low
- 修正の前にまず「方針確定」が必要であり、本 issue は方針議論と実装をセットで扱う

## 現状の問題

### スコープ外として 0076 で分離された箇所

- `src/hpack/encoder.rs:49,64,70,75,79,84,95,98,209` の `// Indexed Header Field (Section 6.1)` 等の RFC 7541 セクション名コメント
- `src/hpack/decoder.rs:74,79,86,96,101` の同様コメント
- `tests/test_hpack/decoder.rs:79,97,105` の `// Never Indexed with new name "x-token" and value "secret"` 等
- 全体に存在する `///` doc コメントのうち英語で書かれているもの
- `SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` 等のプレフィックス付きコメント

### 判断が分かれるポイント

1. **RFC 仕様の概念名**: 例 `// Indexed Header Field (Section 6.1)` を以下のどちらにするか
   - 案 X: 英語維持 (RFC 7541 の章名引用なので原文尊重)
   - 案 Y: 日本語化 (例: `// インデックス型ヘッダーフィールド (Section 6.1)`)

2. **`///` doc コメント**: 公開クレートのドキュメントを日本語化するか
   - 案 X: 英語維持 (国際的なオープンソース利用の観点)
   - 案 Y: 日本語化 (CLAUDE.md 規約厳守)
   - 案 Z: 日英併記 (rustdoc のサポート範囲)

3. **`SAFETY:` 等のプレフィックス**: プレフィックスは英語維持として、説明部のみ日本語化
   - 例: `// SAFETY: ポインタは Session 構築時に登録されており有効` のように、プレフィックスは英語、説明は日本語

## 設計方針

`/polish-issue` で磨き上げる際に上記 3 点の方針を確定する。確定後、確定方針に従って機械的に翻訳または英語維持を選択する。

## 完了条件

- 上記 3 点 (RFC 仕様の概念名 / `///` doc コメント / プレフィックス付きコメント) の方針が `## 設計方針` セクションで確定している
- 確定方針に従って該当コメントが処理されている
- 処理後、`grep -rn "// [A-Z][a-z]" src/ crates/` の結果が「方針で英語維持と決めたもの」のみになる
- `CHANGES.md` の `## develop` の `### misc` サブセクションに `[UPDATE]` エントリが追加されている
- `cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する
- `cargo doc --no-deps` で生成される rustdoc が破綻していないこと (`///` を触る場合)

## 解決方法

issue 0076 マージ後に着手する。方針確定してから実装する。方針議論はチームでの合意を得る前提 (個別判断で勝手に翻訳しない)。

## 参照

- `issues/0076-fmt-translate-english-comments.md` — 先行 issue (英語コメント翻訳の明示列挙箇所)。本 issue のスコープ外として分離された経緯が書かれている
- CLAUDE.md — コメント言語の規約
- `src/hpack/encoder.rs` / `src/hpack/decoder.rs` — RFC 7541 セクション名コメントの対象
- `tests/test_hpack/decoder.rs` — `Never Indexed with new name` 等のテスト用コメントの対象
