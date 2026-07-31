# 残りの英語コメント (RFC 7541 セクション名 / doc コメント / SAFETY 他) の日本語化を検討する

- Priority: Low
- Created: 2026-06-12
- Completed: 2026-07-21
- Polished: 2026-07-31
- Model: Opus 4.7
- Branch: feature/refactor-translate-remaining-english-comments

## 目的

issue 0076 (`fmt-translate-english-comments`、英語コメントの日本語翻訳の明示列挙箇所) のスコープ外として分離された残りの英語コメント (RFC 7541 セクション名コメント、`///` doc コメント、`SAFETY:` 系プレフィックス付きコメント等) について、日本語化の方針を確定し必要なら翻訳する。

0076 は「実コードの説明文の英語コメント」だけを対象とした最小修正であり、「RFC 仕様の概念名」「rustdoc 生成内容に影響する doc コメント」「Rust 慣用プレフィックス付きコメント」のような判断が分かれる領域は本 issue で個別判断する。

## 優先度根拠

- AGENTS.md 規約「コメントは全て日本語にすること」を厳密に守るかどうかは、RFC 仕様の概念名 (例: `Indexed Header Field (Section 6.1)`) を「固有名詞として英語のまま残す」か「概念説明として日本語化する」かの方針判断が必要
- `///` doc コメントは rustdoc の出力結果 (`cargo doc`) に影響するため、日本語化すると公開ドキュメントが日本語になる
- `SAFETY:` / `TODO:` / `FIXME:` 等のプレフィックスは Rust エコシステムの慣用表現であり、プレフィックス自体は英語維持が妥当 (説明文の日本語化は別問題)
- 機能挙動には影響しないため Priority: Low

## 現状の問題

### スコープ外として 0076 で分離された箇所

- `src/hpack/encoder.rs` line 49, 64, 70, 75, 79, 84 の `// Indexed Header Field (Section 6.1)` 等の RFC 7541 エンコーディング種別名コメント
- `src/hpack/decoder.rs` line 74 の同様コメント
- `tests/test_hpack/decoder.rs` line 7, 20-23, 66-67, 79-81 等のバイト列注釈 (hex dump 説明)
- `src/webtransport/flow_control.rs` line 12-14 の draft からの直接引用 (`"This value cannot exceed 2^60..."`)
- `src/webtransport/stream.rs` line 71-84 の ASCII art 状態図 (英語ラベル)
- `crates/shiguredo_nghttp2/src/session.rs` 等の `// SAFETY:` プレフィックス付きコメント (4 箇所、説明部は既に日本語)

### 既に確立されているパターン

コードベースには既に以下のパターンが確立している:

- doc コメント (`///`): 日本語の説明文に英語の技術用語を埋め込む (例: `/// Indexed Header Field をエンコードする (Section 6.1)`)
- `SAFETY:` プレフィックス: プレフィックスは英語、説明部は日本語 (例: `// SAFETY: Session の全パブリックメソッドは &mut self を要求するため、`)
- RFC エンコーディング種別名: インラインコメントで英語のまま (例: `// Indexed Header Field (Section 6.1)`)

## 設計方針

以下の方針を確定する:

### 1. RFC 仕様の概念名・エンコーディング種別名: 英語維持

`// Indexed Header Field (Section 6.1)` 等の RFC エンコーディング種別名は **英語のまま維持** する。

根拠:
- RFC 7541 のエンコーディング種別名 (Indexed Header Field, Literal Header Field with Incremental Indexing 等) は固有名詞であり、翻訳すると仕様書との照合が困難になる
- コードベース全体で既にこのパターンが確立している
- doc コメント (`///`) では日本語説明文に英語の概念名を埋め込む形式を維持 (例: `/// Indexed Header Field をデコードする (Section 6.1)`)

### 2. `///` doc コメント: 日本語で記述、直接引用は英語維持

doc コメントは **日本語で記述** する (AGENTS.md 規約準拠)。ただし以下は英語のまま維持:

- RFC / draft からの直接引用 (verbatim quote): 引用符で囲んで英語のまま保持 (例: `/// "This value cannot exceed 2^60, as it is not possible to encode stream IDs larger than 2^62-1"`)
- ASCII art 状態図のラベル: 仕様の図と一致させるため英語維持 (例: `Ready`, `Send RESET_STREAM`)
- 技術用語の固有名詞: `WebTransport`, `Capsule`, `Huffman` 等は日本語文中にそのまま埋め込む

### 3. `SAFETY:` 等のプレフィックス: プレフィックス英語維持、説明部日本語

`SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` のプレフィックスは **英語維持** (Rust/clippy の慣用表現)。説明部は **日本語** で記述する。

現状の 4 箇所 (`crates/shiguredo_nghttp2/src/`) は既にこの形式に準拠しているため変更不要。

### 4. テスト内のバイト列注釈: 英語維持可

`tests/test_hpack/decoder.rs` の hex dump 説明 (例: `// 0x82 = :method: GET (index 2)`) は **英語維持を許容** する。

根拠:
- バイト列の注釈は技術的メモであり、日本語化すると可読性が落ちる
- テストコードのコメントは「テストのログメッセージ」(AGENTS.md: 日本語) には該当しない (ログメッセージは `println!` / `eprintln!` 等の出力を指す)

## 完了条件

- 上記 4 点の方針が `## 設計方針` セクションで確定している — **本磨き上げで確定済み**
- 確定方針に従い、方針に違反するコメントがあれば修正する。現状のコードベースは既に上記パターンに準拠しているため、追加の翻訳作業は **不要** の見込み
- 実装時に `grep -rn "// [A-Z][a-z]" src/ crates/` を実行し、結果が「方針で英語維持と決めたもの」(RFC 種別名、SAFETY プレフィックス、直接引用、ASCII art ラベル) のみであることを確認する
- `CHANGES.md` の `## develop` の `### misc` サブセクションに `[UPDATE]` エントリが追加されている (翻訳作業が発生した場合のみ。現状維持なら不要)
- `cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 解決方法

方針は本磨き上げで確定済み。現状のコードベースは既にパターンに準拠しているため、大規模な翻訳作業は不要と判断した。本 issue の目的 (方針確定) は達成されているため closed にする。

## 参照

- `issues/0076-fmt-translate-english-comments.md` — 先行 issue (英語コメント翻訳の明示列挙箇所)
- AGENTS.md — コメント言語の規約 (「コメントは全て日本語にすること」「ログメッセージは全て英語にすること」「テストのログメッセージは全て日本語にすること」)
- `src/hpack/encoder.rs` / `src/hpack/decoder.rs` — RFC 7541 エンコーディング種別名コメント (英語維持)
- `src/webtransport/flow_control.rs` — draft 直接引用 (英語維持)
- `src/webtransport/stream.rs` — ASCII art 状態図 (英語ラベル維持)
- `crates/shiguredo_nghttp2/src/session.rs` — SAFETY プレフィックス (既に日本語説明で準拠)
- `tests/test_hpack/decoder.rs` — バイト列注釈 (英語維持可)
