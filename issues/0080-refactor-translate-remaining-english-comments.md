# 残りの英語 `//` コメントを日本語化し、RFC セクション名・doc コメント・SAFETY 等の扱いを確定する

- Priority: Low
- Created: 2026-06-12
- Polished: 2026-06-16
- Model: Opus 4.7
- Branch: feature/refactor-translate-remaining-english-comments

注: 0076 (`fmt-translate-english-comments`) と本質は同じ「テキスト置換のみ」だが、本 issue は加えて「`///` doc コメント / `SAFETY:` プレフィックス等の扱い方針」も確定するため `refactor-` カテゴリを採用する。0076 を `fmt-` カテゴリと揃える案もあるが、方針決定を含むという観点で `refactor-` を維持する。ファイル名 `0080-refactor-translate-remaining-english-comments.md` と Branch 名は一致している。

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
- `tests/test_hpack/decoder.rs:21`: `// 0x41 = 01000001 (incremental indexing, index 1)`
- `tests/test_hpack/decoder.rs:22`: `// 0x0b = length 11`
- `tests/test_hpack/decoder.rs:79,97,105`: `Never Indexed ...` 系のテストコメント
- `tests/test_hpack/decoder.rs:80,106`: `pattern ...` 系のテストコメント
- `tests/test_hpack/decoder.rs:81`: `// 0x07 = length 7 (not Huffman)`
- `tests/test_hpack/decoder.rs:83`: `// 0x06 = length 6 (not Huffman)`
- `tests/test_hpack/decoder.rs:107`: `// 0x05 = length 5 (not Huffman)`

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

- RFC・仕様の固有名詞・略号 (`Never Indexed`、`incremental indexing`、`DATA`、`HEADERS` 等) は **大文字小文字を問わず** 英語のまま維持する (`Never Indexed` / `never indexed` どちらも英語維持)
- 16 進数値・パターン表現 (`0x41`、`pattern 00010000` 等) は英語のまま維持する
- 上記以外の説明文 (動詞句・名詞句・解説) は日本語に翻訳する
- CLAUDE.md 規約「全角と半角の間には半角スペースを入れること」に従う (`24 ビット` のように半角識別子と全角文字の境界に半角スペース)

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
| `// Stream ID (31 bits)` | `// Stream ID (31 ビット)` (ラベル単独形) |
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
7. `CHANGES.md` の `## develop` の `### misc` サブセクション内、既存 `[UPDATE]` ブロック末尾 (種別順 CHANGE→ADD→UPDATE→FIX を保つ位置) に以下のエントリと担当者行を追加する (`shiguredo-issues` 規約により issue 番号は含めない)。`### misc` は本 issue 着手時点で既に存在しているため新規作成不要:

   ```markdown
   - [UPDATE] 英語 `//` コメントを日本語化し、RFC セクション名・doc コメント・SAFETY 等の扱いを確定する
     - @voluntas
   ```

8. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過することを確認する (test は build を兼ねる)

## 完了条件

- `src/webtransport/init.rs:60` のコメントが日本語化されている
- `src/hpack/encoder.rs:95,98,264` のコメントが日本語化されている
- `src/hpack/table.rs:178-487` の `Index N` / `Index N (unused)` マーカーが日本語化されている
- `src/frame/encoder.rs:65,69,71,73,239,245,247,262,295,301` のラベルコメントが日本語化されている
- `src/frame/decoder.rs:133,136,139,142,527,538` のラベルコメントが日本語化されている
- `tests/test_hpack/decoder.rs:21,22,79,80,81,83,97,105,106,107` のコメントが日本語化されている
- `src/hpack/encoder.rs` の RFC 7541 セクション名コメントは変更されていない
- `///` / `//!` doc コメントは変更されていない
- `SAFETY:` / `TODO:` / `FIXME:` / `NOTE:` 等のプレフィックス付きコメントは変更されていない (必要に応じて説明文のみ日本語化済み)
- 対応手順 6 の `grep -rn "// [A-Z][a-z]" src/ crates/` 結果が、RFC/draft セクション名・固有名詞・パターン表現・プレフィックス・doc コメントのみであることを確認済み
- `CHANGES.md` の `## develop` の `### misc` サブセクションに `[UPDATE]` エントリが追加されている (issue 番号なし)
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 他 issue との関係

- **0076** (`fmt-translate-english-comments`): 先行 issue。本 issue は 0076 マージ後に着手する。0076 が `src/hpack/encoder.rs` の L261/273/276/285/288 を担当し、本 issue が L264 を担当する分割になっており、同一テスト関数内のコメントを別 PR で扱うことになる。これは実装者・レビュアーの追跡負荷を上げるため、0076 と本 issue を統合して 1 PR で処理する選択肢があり、ユーザー判断とする
- **0075 / 0079**: それぞれ `CHANGES.md` の `### misc` サブセクションに `[UPDATE]` エントリを追加するため、マージ順序によって既存 `[UPDATE]` ブロック末尾の位置が変わる。マージ時点での最新位置を再確認する

## 参照

- `issues/0076-fmt-translate-english-comments.md` — 先行 issue (英語コメント翻訳の明示列挙箇所)
- `shiguredo-issues` スキル — CHANGES.md に issue 番号を含めない規約
- `shiguredo-changelog` スキル — `### misc` サブセクションの扱い
- `shiguredo-rust` スキル — コメント方針
