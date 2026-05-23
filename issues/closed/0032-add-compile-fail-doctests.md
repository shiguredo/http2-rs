# 構築時検査の compile-fail テストを rustdoc doctest で整備する

Created: 2026-05-23
Completed: 2026-05-24
Model: Opus 4.7
Branch: feature/add-compile-fail-doctests

## 概要

`HeaderField::from_static`, `WindowSize::from_static`, `ClientStreamId::from_static` 等の
`const fn` ベースの構築 API について、**不正リテラルがコンパイルエラーになることを
回帰テストで担保する**仕組みを導入する。

外部依存を増やさない方針 (依存は最小限) に従い、rustdoc 標準機能の `compile_fail`
doctest を使う。各 `from_static` API の doc コメントに `compile_fail` ブロックを
置き、不正リテラルが期待通りコンパイル失敗することを `cargo test --doc` で検証する。

## 背景

`const fn` での構築時検査は強力だが、以下のリグレッションが起きやすい:

- 検査ロジックを `const fn` から普通の `fn` にうっかり戻すと、コンパイル時検出が消える
- `panic!` の文言を変更すると、利用者向けエラーメッセージが劣化する
- `from_static` が「全ての不正リテラルでコンパイルエラーになる」状態を維持しているか、
  通常テストでは検証できない

`compile_fail` doctest は rustdoc 標準機能で、「コンパイルが失敗することが期待される」
コードブロックを doc コメント中に書ける。普通の `cargo test` (CI) で実行される。

## 根拠

- 「コンパイル時に弾ける」が本ライブラリの差別化要素 (issue 0024 参照) であり、
  この性質を CI で守らないとサイレントに失われる
- 当初の `trybuild` 案は外部 crate を必要とするが、本リポジトリは
  「依存は最小限にすること」(`CLAUDE.md`) の方針があり、外部依存を増やせない
- `compile_fail` doctest は標準ツールチェイン (rustdoc) 内で完結する
- `trybuild` の利点である `.stderr` 厳密比較は得られないが、「コンパイル失敗する」性質の
  リグレッション防止という目的には十分

## 設計

### 配置

各 `const fn from_static` API の doc コメント末尾に、`compile_fail` doctest ブロックを
追加する。複数の不正パターンがある API (例: `HeaderField::from_static` は大文字 /
CR/LF / 不正バイト等) では複数の `compile_fail` ブロックを並べる。

### 対象 API (Phase 1 で導入済みの全 `from_static`)

| 型 | from_static 不正パターン |
|---|---|
| `HeaderField::from_static` | uppercase field-name / CR/LF in value |
| `ClientStreamId::from_static` | id = 0 / id 偶数 |
| `ServerStreamId::from_static` | id = 0 / id 奇数 |
| `NonZeroStreamId::from_static` | id = 0 |
| `WindowSize::from_static` | size > 2^31 - 1 |
| `MaxFrameSize::from_static` | size < 2^14 / size > 2^24 - 1 |
| `WindowIncrement::from_static` | increment = 0 |
| `Weight::from_static` | weight = 0 / weight > 256 |
| `LastStreamId::from_static` | id > 2^31 - 1 |

Phase 2 以降で追加される `from_static` (Setting enum 化版や `LimitsBuilder::build_static`)
は、それぞれの実装 issue (0026 / 0028) 内で同様の `compile_fail` doctest を追加する。

### 書き方の例

```text
/// 構築時検査つきで生成する
///
/// 不正値は const eval 時に panic する:
///
/// ```compile_fail
/// const _BAD: shiguredo_http2::HeaderField =
///     shiguredo_http2::HeaderField::from_static(b"Host", b"example.com");
/// ```
pub const fn from_static(name: &'static [u8], value: &'static [u8]) -> Self { ... }
```

doctest は `cargo test` 実行時に自動でビルドされ、`compile_fail` ブロックは
「コンパイル失敗が期待される」テストとして扱われる。逆に **コンパイル成功してしまうと
テストが失敗** する。

### rust-toolchain.toml の安定性

rustdoc は `cargo test --doc` で自動的に走り、`compile_fail` ブロックの判定は
「ビルドが失敗するか否か」だけなので rustc のバージョン依存性は最小限。
`trybuild` のような `.stderr` 厳密比較は採用しないため、stable バージョンが
更新されても回帰検出は壊れない。

## 影響範囲

- `src/hpack/table.rs`: `HeaderField::from_static` の doc に `compile_fail` 追加
- `src/stream_id.rs`: `ClientStreamId::from_static` / `ServerStreamId::from_static`
  / `NonZeroStreamId::from_static` の doc に追加
- `src/settings.rs`: `WindowSize::from_static` / `MaxFrameSize::from_static` の doc に追加
- `src/frame/error.rs`: `WindowIncrement::from_static` / `Weight::from_static` /
  `LastStreamId::from_static` の doc に追加

## CHANGES.md エントリ

`### misc` に追記する:

```
- [ADD] 構築時検査の `*::from_static` API に `compile_fail` doctest を追加し、
  不正リテラル検出のリグレッションを CI で防止する (issue 0032)
  - @voluntas
```

## 受け入れ条件

- Phase 1 で導入済みの全 `const fn from_static` API について、少なくとも 1 件以上の
  `compile_fail` doctest が存在する
- `cargo test --doc -p shiguredo_http2` で全 `compile_fail` ブロックが期待通り
  「コンパイル失敗 (=テスト成功)」となる
- 既存の全テスト・PBT・fuzz が通る
- 外部依存 (`trybuild` 等) を追加していない

## 依存

- [[0024-change-header-field-construct-time-validation]]
- 関連: [[0026-change-setting-construct-time-validation]] (`Setting` enum 化版 `from_static` 追加時にケースを追加)
- 関連: [[0027-change-frame-construct-time-validation]] (フレーム関連の `from_static` 追加時にケースを追加)
- 関連: [[0028-change-limits-builder-result]] (`LimitsBuilder::build_static` 追加時にケースを追加)

## 解決方法

- 当初案の外部 crate `trybuild` 追加は本リポジトリの「依存は最小限にすること」(`CLAUDE.md`) 方針と衝突するため取り止め、rustdoc 標準の `compile_fail` doctest で代替
- issue ファイル名を `0032-add-compile-fail-doctests.md` にリネーム、本文を doctest 前提に書き換え
- Phase 1 で導入済みの全 `const fn from_static` API の doc に `compile_fail` ブロックを追加:
  - `src/hpack/table.rs`: `HeaderField::from_static` (uppercase / CR/LF)
  - `src/stream_id.rs`: `ClientStreamId::from_static` (0 / 偶数)、`ServerStreamId::from_static` (0 / 奇数)、`NonZeroStreamId::from_static` (0)
  - `src/settings.rs`: `WindowSize::from_static` (overflow)、`MaxFrameSize::from_static` (too small / too large)
  - `src/frame/error.rs`: `WindowIncrement::from_static` (0 / overflow)、`Weight::from_static` (256)、`LastStreamId::from_static` (overflow)
- 計 14 件の `compile_fail` doctest が `cargo test --doc -p shiguredo_http2` で「コンパイル失敗 = テスト成功」として通る
- `cargo fmt --check`、`cargo clippy --workspace --all-targets --features __test_helpers -- -D warnings`、`cargo test --workspace --features __test_helpers` の全てが通ることを確認
- 外部依存は一切追加していない (`Cargo.toml` / `Cargo.lock` の差分なし)
- Phase 2 で追加される `from_static` (Setting enum 化版・`LimitsBuilder::build_static`) は、それぞれの実装 issue (0026 / 0028) 内で同種の doctest を追加する
