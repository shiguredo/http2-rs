# field syntax 検査関数を `src/syntax.rs` に集約する

Created: 2026-05-23
Completed: 2026-05-24
Model: Opus 4.7
Branch: feature/refactor-consolidate-field-syntax-module

## 内容

field-name / field-value / 疑似ヘッダーの構文検査関数を、現在の 2 箇所配置 (`src/hpack/bytes.rs` の const fn 版 / `src/hpack/table.rs` の runtime 版) から、crate ルート直下の新規モジュール `src/syntax.rs` に集約する。

本 issue は issue 0024 の /review-diff-code で指摘された以下 2 点を解決する。

1. **二重メンテリスク (D2)**: const fn 版と runtime 版が同じ規則を別ファイルで別実装している。物理集約 + M5 PBT 同値性で乖離リスクを下げる。
2. **責務分離 (D3 改題)**: field-name / field-value / 疑似ヘッダー構文検査は **HTTP/2 セマンティクス** (RFC 9113 §8) の責務であり、**HPACK ヘッダー圧縮** (RFC 7541) の責務ではない。`hpack::table` に置かれているのは `HeaderField` 型の都合に過ぎないため、HPACK 非依存モジュールに切り出す。当初「依存逆転」と表現したが、validation → hpack は上位→下位で正常な依存方向であり「逆転」ではない。本 issue で行うのは **モジュール凝集度の向上と層責務の明確化**。

## 設計方針

### モジュール配置と名称

新規モジュール `src/syntax.rs` を crate ルート直下に作成する。HPACK (`src/hpack/`) と validation (`src/validation.rs`) の両方から参照される共通検査層であり、HPACK 依存ではない (RFC 9113 §8 / RFC 9110 §5.6.2 / RFC 3986 §3.1 / RFC 8441 §4 のみに依拠)。

`src/lib.rs` のモジュール宣言並び (L28-L41) は現状すべて `pub mod` のみで構成されアルファベット順 (`connection` → `webtransport`)。`syntax` は内部公開 (`pub(crate)`) のため、`pub mod` ブロックには混ぜず、`pub mod` 群の直後 (現状の L41 `pub mod webtransport;` の次行) に `pub(crate) mod syntax;` の 1 行ブロックとして分離して置く。これにより「公開モジュール一覧」と「内部限定モジュール」の視認性を保つ (既存 `#[cfg(feature = "__test_helpers")] pub mod __test_helpers;` も L24-L26 に独立ブロックで配置済みで、本配置はその慣習に倣う)。

### 共通化方針

完全な実装共通化は不可能 (runtime 版 `HeaderFieldError` が `Vec<u8>` フィールドを持つため const 文脈で構築できない)。本 issue では **物理的集約のみ** を行い、const fn 版と runtime 版を `src/syntax.rs` の同一ファイルに **隣接配置** してレビューア / 修正者が差分を視認しやすくする。両者の同値性は既存 M5 PBT (`pbt/tests/prop_header_field_syntax.rs`) で引き続き担保する。ロジック共通化は本 issue のスコープ外とする (エラー詳細度の劣化を伴うため別 issue で長期的に検討)。

### 集約対象

`src/hpack/bytes.rs` の const fn 検査関数群 (`check_field_name_const`, `check_field_value_const`, `check_pseudo_header_const` と内部ヘルパすべて) と、`src/hpack/table.rs` の runtime 検査関数群 (`validate_field_name`, `validate_field_value`, `validate_pseudo_header` と内部ヘルパ `is_token_char_lower`, `is_token_char_case_insensitive`, `is_valid_token_case_insensitive`, `is_valid_scheme`) のすべてを `src/syntax.rs` に移動する。公開関数 6 つは `pub(crate)`、内部ヘルパは module-private。

### const fn の panic メッセージ

現状の `panic!("HeaderField::from_static: ...")` プレフィックスは呼び出し元が `HeaderField::from_static` のみであることを前提としている。本 issue 完了後も呼び出し元は同じく `from_static` のみで変わらないため、**プレフィックスは現状維持** する。将来他 const fn コンストラクタから呼ぶ拡張が発生したらその issue 内で汎用化する。

### コメント追記

`src/syntax.rs` の各検査関数 doc コメントに、const / runtime の対応相手と同値性 PBT への参照を書く。例:

```rust
/// const fn 版 field-name 検査 (RFC 9113 §8.2.1, RFC 9110 §5.6.2)
///
/// runtime 版は [`validate_field_name`] (同ファイル内)。両者は同じ規則を別実装で持つ。
/// 同値性は `pbt/tests/prop_header_field_syntax.rs` の M5 PBT で検証する。
#[allow(clippy::missing_panics_doc)]
pub(crate) const fn check_field_name_const(name: &[u8]) {
    ...
}
```

const / runtime のエラー対応関係 (例: `EmptyFieldName` ↔ `"field-name must not be empty"` 等) は移動先の doc コメントに記述する。本 issue 本文には記載しない。

### `HeaderFieldError` の配置

`HeaderFieldError` は `src/hpack/error.rs` に **維持する**。理由:

- public API として `shiguredo_http2::HeaderFieldError` で re-export されており、crate 内移動は API 互換性を保つ場合でも追加の `pub use` を要するなど書き換え範囲が広がる。
- `HeaderFieldError` 自体の責務 (HPACK エラーよりむしろ HTTP/2 フィールド構文エラー) は名前と乖離しており、本来は crate root への昇格が望ましい。ただしこれは別 issue で扱うべき長期的な API 整理であり、本 issue のスコープを膨らませない。
- `syntax` モジュール内の runtime 検査関数は `crate::hpack::error::HeaderFieldError` を `use` して使う。`syntax → hpack::error` の単方向参照のみで循環参照は発生しない。

### `src/hpack/bytes.rs::#[cfg(test)] mod tests` の扱い

現行 `mod tests` (L279-L336) は 2 種類のテストを含む。

- `header_bytes_*` 群 (L286-L317): `HeaderBytes` の `as_slice` / `len` / `PartialEq` / `Hash` を検証する。`HeaderBytes` 型は本 issue 完了後も `bytes.rs` に残るため、これらのテストも **`bytes.rs` に残す**。
- `const_check_*` 群 (L319-L335): `check_field_name_const` / `check_pseudo_header_const` / `check_field_value_const` を `const _: () = ...` の compile-time 評価で呼ぶ。検査関数の移動に合わせて **`src/syntax.rs` 側の `#[cfg(test)] mod tests` に移動する**。

### 呼び出し側の追従

| ファイル | 変更内容 |
|---|---|
| `src/hpack/table.rs` | `use crate::hpack::bytes::{HeaderBytes, check_*_const}` を `use crate::hpack::bytes::HeaderBytes;` と `use crate::syntax::{check_*_const, validate_*};` に整理。runtime `validate_*` 群と内部ヘルパ (L140-L297) を削除。L142 の `// const fn 版は crate::hpack::bytes 側` doc コメントを削除 (関数が消えるので注記不要) |
| `src/hpack/bytes.rs` | const fn 検査関数群 (L54-L262) と `const_check_*` テスト (L319-L335) を削除。モジュール doc コメント (L1-L7) を `HeaderBytes` 型定義のみに簡略化。残るのは `HeaderBytes` enum + impl + `header_bytes_*` テスト群 |
| `src/validation.rs` | L176 の `use crate::hpack::table::{validate_*};` を `use crate::syntax::{validate_*};` に書き換え。モジュール doc コメント (L1-L11) の参照先を `crate::syntax::validate_*` に修正 |
| `src/__test_helpers.rs` | `crate::hpack::bytes::check_*_const` と `crate::hpack::table::validate_*` のすべての参照を `crate::syntax::` に書き換え |
| `pbt/tests/prop_header_field_syntax.rs` | 無変更 (`__test_helpers` 経由のため path 変更が伝播しない)。rename は issue 0039 で対応 |

PBT は `__test_helpers` 経由で呼ぶため crate path 変更の影響を受けない。

## 完了条件

- [ ] `src/syntax.rs` が新設され、const fn 版 / runtime 版の検査関数群が同一ファイルに並んで配置されている
- [ ] `src/lib.rs` の `pub mod webtransport;` の次行に `pub(crate) mod syntax;` が独立ブロックとして追加されている
- [ ] `src/hpack/bytes.rs` から const fn 検査関数群と `const_check_*` テスト群が削除され、`HeaderBytes` 型と impl と `header_bytes_*` テストのみが残っている
- [ ] `src/hpack/bytes.rs` のモジュール doc コメントが `HeaderBytes` 型定義のみを記述するように簡略化されている
- [ ] `src/hpack/table.rs` から runtime `validate_*` 群と内部ヘルパが削除されている
- [ ] `src/hpack/table.rs::HeaderField::new_with_sensitive` / `from_static` の検査関数呼び出しが `crate::syntax::` 経由になっている
- [ ] `src/validation.rs::check_field` および module doc が `crate::syntax::` 経由を参照するように書き換わっている
- [ ] `src/__test_helpers.rs` の検査関数参照がすべて `crate::syntax::` に書き換わっている
- [ ] `HeaderFieldError` は `src/hpack/error.rs` から動かしておらず、`shiguredo_http2::HeaderFieldError` の re-export パスが不変
- [ ] `pbt/tests/prop_header_field_syntax.rs` の M5 同値性 PBT がファイル名・内容ともに無変更で引き続き通る
- [ ] `grep -rn 'hpack::bytes::check_' src/ pbt/ fuzz/` と `grep -rn 'hpack::table::validate_' src/ pbt/ fuzz/` が共に 0 件
- [ ] `cargo build` と `cargo build --features __test_helpers` の両方が通る
- [ ] `cargo test --workspace` と `cargo test --workspace --features __test_helpers` の両方が通る
- [ ] `cargo build --manifest-path fuzz/Cargo.toml` が通る
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --all -- --check` が通る
- [ ] CHANGES.md `### misc` に下記文面を追記

## CHANGES.md エントリ

`## develop` の `### misc` に追記する:

```
- [UPDATE] field-name / field-value / 疑似ヘッダーの構文検査関数を `src/syntax.rs` に集約し、HPACK 非依存のモジュールに分離する
  - @voluntas
```

## ブランチ命名

`feature/refactor-consolidate-field-syntax-module` を使用する。

## スコープ外

- `HeaderBytes` enum の Cow 化 / 削除、および `src/hpack/bytes.rs` の削除 → 0035 で対応
- PBT ファイル名 `prop_header_field_syntax.rs` → `prop_syntax.rs` の rename → 0039 で対応
- 検査ロジックの 1 関数共通化 (`Result<(), &'static str>` 退化を伴う) → エラー詳細度の劣化を許容しないため、本 issue では行わない
- `HeaderFieldError` の crate root 昇格 → 長期的な API 整理として別 issue 化

## テスト戦略

- 既存の M5 同値性 PBT (`pbt/tests/prop_header_field_syntax.rs`) が変更なしで引き続き通ることを確認する。本 issue は検査関数の物理位置を変えるだけでロジックは不変。
- `const_check_*` テスト群を `src/syntax.rs` の `#[cfg(test)] mod tests` に移し、compile-time 評価による **正常系のみ** のテストを維持する。失敗系は const 評価で panic = コンパイルエラーになるため、accept される入力に限定する (既存テスト同様)。
- 物理集約後も別実装が並走するため、M5 PBT は **継続して必須** (削除や無効化はしない)。
- 新規テスト追加なし。`tests/test_syntax.rs` も新設しない (CLAUDE.md「単体テストのファイル名は `tests/test_<module>.rs`」規約に照らすと候補になるが、検査関数の accept / reject 同値性は M5 PBT で全網羅されており、PBT で実現できる単体テストは書かないという CLAUDE.md 規約に従い不要)。
- カバレッジは `cargo llvm-cov` で移動前後の同等性を確認する。

## RFC 引用

集約する検査関数の根拠 RFC 節を `src/syntax.rs` モジュール doc コメントに列挙する。

- field-name = token: RFC 9113 §8.2.1 + RFC 9110 §5.6.2 (token = 1*tchar)
- field-name lowercase ASCII 必須 (MUST NOT 0x41-0x5a): RFC 9113 §8.2.1
- field-value NUL / CR / LF 禁止: RFC 9113 §8.2.1
- field-value 先頭末尾 SP / HTAB 禁止: RFC 9113 §8.2.1
- 疑似ヘッダー名集合 (`:method` / `:scheme` / `:authority` / `:path` / `:status` / `:protocol`): RFC 9113 §8.3.1, §8.3.2, RFC 8441 §4
- `:method` 値 token: RFC 9110 §9.1
- `:scheme` 値構文: RFC 3986 §3.1
- `:path` absolute-path / asterisk-form: RFC 9113 §8.3.1, RFC 9110 §4.1
- `:status` 3DIGIT: RFC 9110 §15
- `:protocol` 値 HTTP Upgrade Token: RFC 8441 §4 + RFC 9110 §7.8

「これらは HTTP/2 セマンティクスの責務 (RFC 9113 §8) であり、HPACK 圧縮 (RFC 7541) の責務ではない」という配置根拠もモジュール doc に明記する。

## 依存

- 本 issue を blocking 依存とする後続 issue: [[0035-refactor-replace-header-bytes-with-cow]] (本 issue 完了で `bytes.rs` 内の検査関数が消えた後に `HeaderBytes` Cow 化を行う必要があるため、0035 は 0034 完了が必須前提)、[[0039-fix-pbt-naming-convention]] (本 issue 完了後の `src/syntax.rs` 新設を受けて `prop_header_field_syntax.rs` → `prop_syntax.rs` に rename するため)
- 関連: [[0033-refactor-test-helpers-module-and-bytes-mod-name]] (本 issue で `__test_helpers.rs` 内の検査関数 crate path を `crate::syntax::` に更新する。`__test_helpers` モジュールの公開層整理自体は 0033 のスコープ)
- 本 issue 自体の前提: なし (独立着手可能)

## 解決方法

以下の手順で field syntax 検査関数を `src/syntax.rs` に集約した。

### 新規作成

- `src/syntax.rs`: const fn 版 (`check_field_name_const`, `check_field_value_const`, `check_pseudo_header_const`) と runtime 版 (`validate_field_name`, `validate_field_value`, `validate_pseudo_header`) の全検査関数を同一ファイルに配置。内部ヘルパ (`is_token_char_lower`, `is_token_char_case_insensitive`, `bytes_eq`, `check_token_nonempty_const`, `check_scheme_const`, `check_path_const`, `is_valid_token_case_insensitive`, `is_valid_scheme`) も集約。const fn 版と runtime 版で重複していたヘルパ関数 (`is_token_char_lower_const` / `is_token_char_lower`, `is_tchar_const` / `is_token_char_case_insensitive`) はそれぞれ 1 つに統一した。
- `src/syntax.rs` の `#[cfg(test)] mod tests` に `const_check_accepts_valid_pseudo` / `const_check_accepts_valid_regular` テストと `syntax_equivalence` PBT を配置。

### 削除

- `src/hpack/bytes.rs` から const fn 検査関数群 (L54-L277) と `const_check_*` テスト (L319-L335) を削除。`HeaderBytes` 型と impl と `header_bytes_*` テストのみ残存。
- `src/hpack/table.rs` から runtime 検査関数群 (L145-L304) と `syntax_equivalence` PBT (L909-L1062) を削除。

### 変更

- `src/lib.rs`: `pub mod webtransport;` の次行に `pub(crate) mod syntax;` を追加。
- `src/hpack/table.rs`: import を `crate::syntax::{check_*_const, validate_*}` に変更。モジュール doc コメントに `crate::syntax` への参照を追記。
- `src/validation.rs`: `check_field` 内の import を `crate::syntax::{validate_*}` に変更。モジュール doc コメントの参照先を `crate::syntax` に修正。

### テスト

- `cargo test --workspace` 全件通過
- `cargo clippy --workspace --all-targets -- -D warnings` 通過
- `cargo fmt --all -- --check` 通過
- `cargo build --manifest-path fuzz/Cargo.toml` 通過
