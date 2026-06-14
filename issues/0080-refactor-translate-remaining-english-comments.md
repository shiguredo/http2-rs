# 残りの英語 `//` コメントを日本語化し、RFC セクション名・doc コメント・SAFETY 等の扱いを確定する

- Priority: Low
- Created: 2026-06-12
- Polished: 2026-06-14
- Model: Opus 4.7
- Branch: feature/refactor-translate-remaining-english-comments

## 目的

issue 0076 (`fmt-translate-english-comments`、英語コメントの日本語翻訳の明示列挙箇所) のスコープ外として分離された残りの英語コメントを処理する。具体的には:

- RFC/draft セクション名コメント、`///` doc コメント、`SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` 等のプレフィックス付きコメントの扱いを確定する
- 上記以外で 0076 では対象外とされた英語 `//` コメントを日本語化する

## 優先度根拠

- CLAUDE.md / AGENTS.md 規約「コメントは全て日本語にすること」を厳密に守るため、残存する英語 `//` コメントを整理する必要がある
- RFC 仕様の概念名 (例: `Indexed Header Field (Section 6.1)`) や `///` doc コメントは判断が分かれるため、方針を確定してから実装する
- 機能挙動には影響しないため Priority: Low

## 現状の問題

### 0076 でスコープ外とされた英語 `//` コメント

以下の箇所は 0076 では「別 issue 0080 で対応」とされていたが、未翻訳のまま残っている:

- `src/webtransport/init.rs:60`: `// RFC 8941 Section 4.2.2 step 2.1: Parse a Key`
- `src/hpack/encoder.rs:264`: `// 0x41 = 01000001 (incremental indexing, index 1)`
- `src/hpack/encoder.rs:95`: `// Never Indexed with existing name (0x1x prefix)`
- `src/hpack/encoder.rs:98`: `// Never Indexed with new name (0x10 prefix)`
- `src/hpack/table.rs:178-487`: `// Index N` / `// Index N (unused)` マーカー (62 エントリ分)
- `src/frame/encoder.rs:65,69,71,73,239,245,247,262,295,301`: フレーム構成要素のラベルコメント (`Length (24 bits)` 等)
- `src/frame/decoder.rs:133,136,139,142,527,538`: フレーム構成要素のラベルコメント (`Length (24 bits)` 等)
- `tests/test_hpack/decoder.rs:79,97,105`: `Never Indexed ...` 系のテストコメント
- `tests/test_hpack/decoder.rs:80,106`: `pattern ...` 系のテストコメント

### 判断が分かれるカテゴリ

1. **RFC/draft セクション名コメント**: 例 `// Indexed Header Field (Section 6.1)`
2. **`///` doc コメント**: rustdoc 生成内容への影響があり、国際的なオープンソース利用の観点で要検討
3. **`SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` 等のプレフィックス付きコメント**: プレフィックスは Rust エコシステムの慣用表現

## 設計方針

### RFC/draft セクション名コメント

**英語のまま維持する**。0076 と同じく、RFC・draft・仕様の固有名詞・概念名は英語を維持する。セクション番号も英語のままとする。

例:

- `// Indexed Header Field (Section 6.1)` → 変更なし
- `// Literal Header Field with Incremental Indexing (Section 6.2.1)` → 変更なし
- `// RFC 8941 Section 4.2.2 step 2.1: Parse a Key` → RFC 参照部は変更なし、`Parse a Key` のみ日本語化

### `///` doc コメント

**本 issue では扱わない**。対象範囲が広く rustdoc 出力への影響も大きいため、別途 issue を起票して個別に方針決定・翻訳を行う。本 issue では `///` / `//!` は一切変更しない。

### `SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` 等のプレフィックス付きコメント

**プレフィックスは英語のまま、説明文は日本語**とする。既存コメントは原則としてこの形になっているため、変更は不要。新規追加時も同様とする。

例:

- `// SAFETY: user_data は Session へのポインタとして設定されている` → 変更なし

### 残りの英語 `//` コメントの翻訳ルール

0076 と同じルールを適用する:

- RFC・仕様の固有名詞・略号 (`Never Indexed`、`incremental indexing`、`DATA`、`HEADERS` 等) は英語のまま維持する
- 16 進数値・パターン表現 (`0x41`、`pattern 00010000` 等) は英語のまま維持する
- 上記以外の説明文 (動詞句・名詞句・解説) は日本語に翻訳する

翻訳例:

| 元 (英語) | 翻訳 (日本語) |
|---|---|
| `// RFC 8941 Section 4.2.2 step 2.1: Parse a Key` | `// RFC 8941 Section 4.2.2 step 2.1: Key をパースする` |
| `// 0x41 = 01000001 (incremental indexing, index 1)` | `// 0x41 = 01000001 (incremental indexing、インデックス 1)` |
| `// Never Indexed with existing name (0x1x prefix)` | `// 既存の名前で Never Indexed を符号化する (0x1x prefix)` |
| `// Never Indexed with new name (0x10 prefix)` | `// 新しい名前で Never Indexed を符号化する (0x10 prefix)` |
| `// Index 0 (unused)` | `// インデックス 0 (未使用)` |
| `// Index 1` | `// インデックス 1` |
| `// Length (24 bits)` | `// Length (24 ビット)` |
| `// Type (8 bits)` | `// Type (8 ビット)` |
| `// Flags (8 bits)` | `// Flags (8 ビット)` |
| `// Stream ID (31 bits, R bit is reserved)` | `// Stream ID (31 ビット、R bit は予約)` |
| `// Last-Stream-ID (31 bits, R bit is reserved)` | `// Last-Stream-ID (31 ビット、R bit は予約)` |
| `// Error Code` | `// Error Code` |
| `// Debug Data` | `// Debug Data` |
| `// Window Size Increment (31 bits, R bit is reserved)` | `// Window Size Increment (31 ビット、R bit は予約)` |
| `// Prioritized Element ID (31 bits, R bit is reserved)` | `// Prioritized Element ID (31 ビット、R bit は予約)` |
| `// Priority Field Value` | `// Priority Field Value` |
| `// Never Indexed with new name "x-token" and value "secret"` | `// 新しい名前 "x-token" と値 "secret" で Never Indexed をデコードする` |
| `// Never Indexed should not be added to dynamic table` | `// Never Indexed は動的テーブルに追加されない` |
| `// Never Indexed with name index 7 (:scheme) and value "https"` | `// 名前インデックス 7 (:scheme) と値 "https" で Never Indexed をデコードする` |
| `// 0x10 = pattern 00010000 (never indexed, new name)` | `// 0x10 = pattern 00010000 (never indexed、新しい名前)` |
| `// 0x17 = pattern 0001 + 0111 (never indexed, index 7)` | `// 0x17 = pattern 0001 + 0111 (never indexed、インデックス 7)` |

なお、`src/frame/decoder.rs:142` や `src/frame/decoder.rs:527` のように、ラベル部分の後に日本語説明が続くコメントは、ラベル部分のみを日本語化する (`// Stream ID (31 ビット)`、`// Prioritized Element ID (31 ビット)`)。後続の日本語説明はそのまま維持する。

## スコープ外

- `///` / `//!` doc コメント: 本 issue では変更せず、別途 issue で扱う
- RFC/draft セクション名コメント: 英語維持として本 issue では変更しない
- `SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` 等のプレフィックス付きコメント: 既にプレフィックス英語・説明文日本語の形になっており、変更しない
- `src/` / `crates/*/src/` 配下の `#[cfg(test)] mod tests` 内の `.unwrap()` 等に関するコメント: 0075 / 0079 で対応
- `examples/` 配下: 0075 で対応済み
- `pbt/` / `fuzz/` 配下: PBT / fuzz は失敗時にフレームワーク経由でメッセージが得られるため対象外

## 対応手順

1. 作業ブランチ `feature/refactor-translate-remaining-english-comments` を作成する
2. 上記「現状の問題」の英語 `//` コメントを「設計方針」の翻訳例に従って日本語化する
3. `src/hpack/encoder.rs` の RFC 7541 セクション名コメントは変更しないことを確認する
4. `///` / `//!` doc コメントを一切変更していないことを確認する
5. `SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` 等のプレフィックス付きコメントを変更していないことを確認する
6. `grep -rn "// [A-Z][a-z]" src/ crates/` でヒットする残存英語コメントが、RFC/draft セクション名・固有名詞・パターン表現・プレフィックス・doc コメントのみであることを確認する
7. `CHANGES.md` の `## develop` セクションに `### misc` サブセクションがあればその末尾に、なければ新規作成して以下のエントリと担当者行を追加する (`shiguredo-issues` 規約により issue 番号は含めない):

   ```markdown
   ### misc

   - [UPDATE] 対象外としていた残りの英語 `//` コメントを日本語化し、RFC セクション名・doc コメント・SAFETY 等の扱いを確定する
     - @voluntas
   ```

8. `cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過することを確認する

## 完了条件

- `src/webtransport/init.rs:60` のコメントが日本語化されている
- `src/hpack/encoder.rs:95,98,264` のコメントが日本語化されている
- `src/hpack/table.rs:178-487` の `Index N` / `Index N (unused)` マーカーが日本語化されている
- `src/frame/encoder.rs:65,69,71,73,239,245,247,262,295,301` のラベルコメントが日本語化されている
- `src/frame/decoder.rs:133,136,139,142,527,538` のラベルコメントが日本語化されている
- `tests/test_hpack/decoder.rs:79,80,97,105,106` のコメントが日本語化されている
- `src/hpack/encoder.rs` の RFC 7541 セクション名コメントは変更されていない
- `///` / `//!` doc コメントは変更されていない
- `SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` 等のプレフィックス付きコメントは変更されていない (必要に応じて説明文のみ日本語化済み)
- `CHANGES.md` の `## develop` の `### misc` サブセクションに `[UPDATE]` エントリが追加されている (issue 番号なし)
- `cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 解決方法

issue 0076 マージ後、上記対応手順に従って残存する英語 `//` コメントを翻訳し、各カテゴリの扱いを確定する。

## 参照

- `issues/0076-fmt-translate-english-comments.md` — 先行 issue (英語コメント翻訳の明示列挙箇所)。本 issue のスコープ外として分離された経緯が書かれている
- **0075 (`refactor-replace-unwrap-with-expect`) / 0076 (`fmt-translate-english-comments`) / 0079 (`refactor-replace-unwrap-with-expect-build-script-and-tests`)**: それぞれ `CHANGES.md` の `### misc` サブセクションを新規作成する可能性がある。0075/0076/0079/0080 が並列にマージされる場合、`### misc` セクションが重複して生成されるため、マージ時に 1 つに統合する
- `shiguredo-issues` スキル — issue 番号を含めてはいけない場所 (CHANGES.md) の規約
- `shiguredo-changelog` スキル — `### misc` サブセクションの扱い
- `shiguredo-rust` スキル — コメント方針
- `src/webtransport/init.rs:60`
- `src/hpack/encoder.rs:95,98,264`
- `src/hpack/table.rs:178-487`
- `src/frame/encoder.rs:65,69,71,73,239,245,247,262,295,301`
- `src/frame/decoder.rs:133,136,139,142,527,538`
- `tests/test_hpack/decoder.rs:79,80,97,105,106`
