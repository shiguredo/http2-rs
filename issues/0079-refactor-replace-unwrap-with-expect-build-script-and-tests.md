# build.rs と tests/ の .unwrap() を .expect() に置換する

- Priority: Low
- Created: 2026-06-12
- Polished: {Polished}
- Model: Opus 4.7
- Branch: feature/refactor-replace-unwrap-with-expect-build-script-and-tests

## 目的

`crates/nghttp2-sys/build.rs` と `tests/` 配下 (integration test) の `.unwrap()` を `.expect("MESSAGE")` に置換し、`shiguredo-rust` 規約「`.unwrap()` ではなく `.expect("MESSAGE")` を使うこと」に準拠させる。

issue 0075 (`refactor-replace-unwrap-with-expect`、`examples/` の `.unwrap()` 整理) のスコープ外として明示的に分離された作業。0075 は `examples/` のみを対象とした最小修正に留め、`build.rs` と `tests/` の整理は本 issue で扱う。

## 優先度根拠

- `shiguredo-rust` 規約「`.unwrap()` ではなく `.expect("MESSAGE")` を使うこと」に違反している既存箇所の整理
- `build.rs` は build 失敗時のメッセージが欠落すると原因特定が困難になるため、`.expect("理由")` 化の価値は高い
- `tests/` 配下の integration test も同様で、CI 失敗時の原因特定が容易になる
- 機能挙動には影響しないため Priority: Low
- 修正コストは小〜中 (build.rs は 6 箇所、tests は多数だが機械的置換可能)

## 現状の問題

`shiguredo-rust` 規約: `.unwrap()` ではなく `.expect("MESSAGE")` を使うこと (理由: panic 時のメッセージで「想定外なのか / 仕様上絶対起きないのか」を区別できるようにするため)。

### `crates/nghttp2-sys/build.rs` の `.unwrap()`

build script 内に `.unwrap()` が 6 箇所存在 (`grep -n "\.unwrap()" crates/nghttp2-sys/build.rs` で確認):

- `std::env::var("CARGO_MANIFEST_DIR").unwrap()` (CARGO 環境変数読み込み)
- `read_to_string(...).unwrap()` (ファイル読み込み)
- `shiguredo_toml::from_str(...).unwrap()` (TOML パース)
- `std::env::var("OUT_DIR").unwrap()` (CARGO 環境変数読み込み)
- `to_str().unwrap()` (パス変換) 等

これらは build 時の環境変数が設定されていなかったり、ファイルが読めなかったりすると panic するが、現状の `.unwrap()` ではエラーの文脈が失われる。

### `tests/` 配下の `.unwrap()`

`grep -rn "\.unwrap()" tests/` で多数の `.unwrap()` が確認できる。integration test 内の `.unwrap()` は失敗時にテスト失敗の文脈で情報が得られるとは言え、`.expect("理由")` 化することで CI ログから原因を即座に特定できる。

## 設計方針

### `build.rs` の置換

各 `.unwrap()` を `.expect("具体的な失敗理由")` に置換する。メッセージ例:

- `std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR は Cargo が必ず設定する")`
- `read_to_string(path).expect("Cargo.toml の読み込みに失敗 (ファイル不在/権限不足)")`
- `shiguredo_toml::from_str(s).expect("Cargo.toml の TOML パースに失敗")`

### `tests/` の置換

機械的に `.unwrap()` を `.expect("理由")` に置換する。理由文は文脈に応じて記述するが、量が多いため簡潔な定型文 (例: `.expect("テスト前提条件: <内容>")`) で統一する選択肢もある。

詳細な方針は `/polish-issue` で磨き上げる際に確定する (テスト全体を一括置換するか、segment ごとに分割するか)。

## スコープ外

- `src/` / `crates/*/src/` 配下の `#[cfg(test)] mod tests` 内の `.unwrap()`: テストコード内の `.unwrap()` は失敗時にテスト失敗の文脈で十分情報が得られるため、本 issue の対象外 (テストの可読性とのバランスで許容)。0075 でも同様の扱い
- `pbt/` / `fuzz/` 配下: PBT / fuzz は失敗時にフレームワーク経由でメッセージが得られるため、本 issue の対象外
- `examples/` 配下: 0075 で既に対応済み

## 完了条件

- `crates/nghttp2-sys/build.rs` の `.unwrap()` 6 箇所がすべて `.expect("理由")` に置換されている
- `tests/` 配下の `.unwrap()` が `.expect("理由")` に置換されている (詳細は `/polish-issue` で確定)
- 各 expect メッセージは日本語で具体的な失敗理由を示している (CLAUDE.md 規約「コメントは全て日本語にすること」「テストのログメッセージは全て日本語にすること」)
- `grep -rn "\.unwrap()" crates/nghttp2-sys/build.rs tests/` で 0 件 (もしくは正当な理由があるもののみ)
- `CHANGES.md` の `## develop` の `### misc` サブセクションに `[UPDATE]` エントリが追加されている (機能影響なしのため `### misc` 配下)
- `cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 解決方法

issue 0075 マージ後に着手する。詳細な置換手順とテストの量に応じた分割可否は `/polish-issue` で磨き上げる。

## 参照

- `issues/closed/0075-fmt-replace-unwrap-with-expect.md` — 先行 issue (`examples/` の `.unwrap()` 整理)。本 issue のスコープ外として分離された経緯が書かれている
- `~/.claude/skills/shiguredo-rust/SKILL.md` — `.unwrap()` ではなく `.expect("MESSAGE")` を使う規約
- `~/.claude/skills/shiguredo-changelog/SKILL.md` — `### misc` サブセクションの扱い
- `crates/nghttp2-sys/build.rs` — `.unwrap()` 6 箇所の対象
- `tests/` — integration test の対象
