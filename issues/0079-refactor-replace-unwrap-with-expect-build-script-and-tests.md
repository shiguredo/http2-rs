# build.rs と tests/ の .unwrap() を .expect() に置換する

- Priority: Low
- Created: 2026-06-12
- Polished: 2026-06-16
- Model: Opus 4.7
- Branch: feature/refactor-replace-unwrap-with-expect-build-script-and-tests

## 目的

`crates/nghttp2-sys/build.rs` とリポジトリルートの `tests/` 配下 (integration test) の `.unwrap()` を `.expect("MESSAGE")` に置換し、`shiguredo-rust` 規約「`.unwrap()` ではなく `.expect("MESSAGE")` を使うこと」に準拠させる。

issue 0075 (`refactor-replace-unwrap-with-expect`、`examples/` の `.unwrap()` 整理) のスコープ外として明示的に分離された作業。0075 は `examples/` のみを対象とした最小修正に留め、`build.rs` と `tests/` の整理は本 issue で扱う。

## 優先度根拠

- `shiguredo-rust` 規約「`.unwrap()` ではなく `.expect("MESSAGE")` を使うこと」に違反している既存箇所の整理
- `build.rs` は build 失敗時のメッセージが欠落すると原因特定が困難になるため、`.expect("理由")` 化の価値は高い
- `tests/` 配下の integration test も同様で、CI 失敗時の原因特定が容易になる
- 機能挙動には影響しないため Priority: Low
- 修正コストは中〜大 (build.rs は 6 箇所、tests/ は 315 箇所。多数だが定型文の使い回しが可能で実装は比較的容易。`shiguredo-rust` 規約「panic 時の『想定外 / 仕様上絶対起きないか』を区別」の趣旨に従い、定型文を使う場合も「セットアップ失敗 / decode 失敗 / イベント取得失敗」等の文脈ごとに分類して書き分ける)

## 現状の問題

`shiguredo-rust` 規約: `.unwrap()` ではなく `.expect("MESSAGE")` を使うこと (理由: panic 時のメッセージで「想定外なのか / 仕様上絶対起きないのか」を区別できるようにするため)。

### `crates/nghttp2-sys/build.rs` の `.unwrap()`

build script 内に `.unwrap()` が 6 箇所存在 (`grep -n "\.unwrap()" crates/nghttp2-sys/build.rs` で確認):

- `crates/nghttp2-sys/build.rs:6`: `std::env::var("CARGO_MANIFEST_DIR").unwrap()`
- `crates/nghttp2-sys/build.rs:7`: `std::fs::read_to_string(...).unwrap()`
- `crates/nghttp2-sys/build.rs:8`: `shiguredo_toml::from_str(...).unwrap()`
- `crates/nghttp2-sys/build.rs:25`: `std::env::var("OUT_DIR").unwrap()`
- `crates/nghttp2-sys/build.rs:79`: `std::env::var("CARGO_MANIFEST_DIR").unwrap()` (`#[cfg(feature = "overwrite")]` 内)
- `crates/nghttp2-sys/build.rs:86`: `manifest_dir.join("src/wrapper.h").to_str().unwrap()` (`#[cfg(feature = "overwrite")]` 内)

これらは build 時の環境変数が設定されていなかったり、ファイルが読めなかったりすると panic するが、現状の `.unwrap()` ではエラーの文脈が失われる。

### `tests/` 配下の `.unwrap()`

`grep -rn "\.unwrap()" tests/ --include='*.rs'` で 315 件の `.unwrap()` が確認できる (2026-06-16 時点)。integration test 内の `.unwrap()` は失敗時にテスト失敗の文脈で情報が得られるとは言え、`.expect("理由")` 化することで CI ログから原因を即座に特定できる。

ファイルごとの内訳 (実装着手時には `grep -rc "\.unwrap()" tests/ --include='*.rs' | sort -t: -k2 -nr` で最新値を再確認すること):

- `tests/test_connection.rs`: 98 件
- `tests/test_webtransport/integration.rs`: 41 件
- `tests/test_hpack/rfc7541.rs`: 26 件
- `tests/test_webtransport/root.rs`: 24 件
- `tests/test_webtransport/capsule.rs`: 22 件
- `tests/test_hpack/dynamic_table.rs`: 18 件
- `tests/test_hpack/decoder.rs`: 15 件
- `tests/test_webtransport/varint.rs`: 14 件
- `tests/test_hpack/table.rs`: 11 件
- `tests/test_webtransport/flow_control.rs`: 8 件
- `tests/test_webtransport/stream.rs`: 7 件
- `tests/test_stream_id.rs`: 7 件
- `tests/test_hpack/integer.rs`: 7 件
- `tests/test_hpack/huffman.rs`: 6 件
- `tests/test_flow_control.rs`: 6 件
- `tests/test_hpack/encoder.rs`: 4 件
- `tests/test_validation.rs`: 1 件

合計: 98+41+26+24+22+18+15+14+11+8+7+7+7+6+6+4+1 = **315 件**

## 設計方針

### `expect` メッセージの言語

- `crates/nghttp2-sys/build.rs`: `.expect()` は panic message として出力されるが、build script では panic 時に build ログとしてユーザーに表示されるため、AGENTS.md「ログメッセージは全て英語にすること」に従い**英語**とする
- `tests/` 配下: テストのログメッセージとして扱われるため、AGENTS.md「テストのログメッセージは全て日本語にすること」に従い**日本語**とする

### `build.rs` の置換

各 `.unwrap()` を `.expect("具体的な失敗理由")` に置換する。既存の英語 `.expect` メッセージ (`Failed to execute git clone` 等) とトーンを合わせ、句頭大文字・句読点なしの短い英文とする。実際の呼び出しは `PathBuf::from(...)` 等に包まれているため、`.expect()` は `.unwrap()` と同じ位置（`Result` を返す式の直後）に入れる:

- `PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by Cargo"))`
- `std::fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("Failed to read crates/nghttp2-sys/Cargo.toml")`
- `shiguredo_toml::from_str(&cargo_toml).expect("Failed to parse crates/nghttp2-sys/Cargo.toml")`
- `PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR must be set by Cargo"))`
- `manifest_dir.join("src/wrapper.h").to_str().expect("src/wrapper.h path must be valid UTF-8")`

### `tests/` の置換

- `.unwrap()` を `.expect("<文脈に応じた理由>")` に機械的に置換する
- メッセージは日本語とし、簡潔に失敗内容を示す。量が多いため、定型文を使ってもよい:
  - セットアップ処理: `.expect("テスト用ヘッダーの構築に失敗")`
  - decode 結果: `.expect("フレームのデコードに失敗")`
  - イベント取得: `.expect("イベントの取得に失敗")`
  - ストリームデータ: `.expect("ストリームデータの取得に失敗")`
- 可能な限り文脈に応じたメッセージを入れるが、大量のため同一ファイル内で同じパターンが続く場合は定型文で統一してもよい

## スコープ外

- `src/` / `crates/*/src/` 配下の `#[cfg(test)] mod tests` 内の `.unwrap()`: テストコード内の `.unwrap()` は失敗時にテスト失敗の文脈で十分情報が得られるため、本 issue の対象外 (テストの可読性とのバランスで許容)。0075 でも同様の扱い
- `crates/*/tests/` 配下 (crate 単位の integration test): 本 issue でいう `tests/` 配下はリポジトリルートの `tests/` ディレクトリを指す。`crates/shiguredo_nghttp2/tests/test_session.rs` 等は `.unwrap()` が存在せず対象外
- `pbt/` / `fuzz/` 配下: PBT / fuzz は失敗時にフレームワーク経由でメッセージが得られるため、本 issue の対象外
- `examples/` 配下: 0075 で既に対応済み
- `tests/test_error.rs` / `tests/test_frame.rs`: `.unwrap()` が存在しないため変更なし

## 他 issue との関係

- **0075 (`refactor-replace-unwrap-with-expect`)**: `examples/` の `.unwrap()` を整理する先行 issue。本 issue は 0075 のスコープ外として分離された
- **0076 (`fmt-translate-english-comments`) / 0080 (`refactor-translate-remaining-english-comments`)**: それぞれ `CHANGES.md` の `### misc` サブセクションを新規作成する可能性がある。0075/0076/0079/0080 が並列にマージされる場合、`### misc` セクションが重複して生成されるため、マージ時に 1 つに統合する

## 対応手順

1. 作業ブランチ `feature/refactor-replace-unwrap-with-expect-build-script-and-tests` を作成する
2. `crates/nghttp2-sys/build.rs` の 6 箇所を上記「`build.rs` の置換」に従って `.expect("...")` に置換する (メッセージは英語)
3. `tests/` 配下の各ファイルを上記「`tests/` の置換」に従って `.unwrap()` を `.expect("...")` に置換する (メッセージは日本語)。ファイル数が多いため、`grep -rc "\.unwrap()" tests/ --include='*.rs' | sort -t: -k2 -nr` で件数順に並べ、件数の多いファイルから 1 ファイルずつ進めると差分が見やすい
4. `grep -n "\.unwrap()" crates/nghttp2-sys/build.rs` で 0 件、`grep -rn "\.unwrap()" tests/ --include='*.rs'` で 0 件になることを確認する (`tests/` 配下のスコープ外ファイル `tests/test_error.rs` / `tests/test_frame.rs` / `tests/test_send_error.rs` / `tests/test_settings.rs` / `tests/test_limits.rs` / `tests/test_validation.rs` を除く `tests/test_*.rs` / `tests/test_*/`*.rs` で 0 件、ただしこれらスコープ外ファイルにはそもそも `.unwrap()` が存在しないため単純な `grep -rn '\.unwrap()' tests/ --include='*.rs'` で 0 件確認できる)
5. `CHANGES.md` の `## develop` の `### misc` サブセクション内、既存 `[UPDATE]` ブロック末尾 (種別順 CHANGE→ADD→UPDATE→FIX を保つ位置) に以下のエントリと担当者行を追加する (`shiguredo-issues` 規約により issue 番号は含めない)。`### misc` 内に `[UPDATE]` ブロックが複数箇所にある場合は、より後ろ (下) に位置するブロックの末尾に追加する:

   ```markdown
   - [UPDATE] `crates/nghttp2-sys/build.rs` と `tests/` 配下の `.unwrap()` を `.expect("MESSAGE")` に置換し、`shiguredo-rust` 規約に準拠させる
     - @voluntas
   ```

6. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過することを確認する (test は build を兼ねる)

## 完了条件

- `crates/nghttp2-sys/build.rs` の `.unwrap()` 6 箇所がすべて `.expect("理由")` に置換され、メッセージが英語になっている
- リポジトリルートの `tests/` 配下の `.unwrap()` がすべて `.expect("理由")` に置換され、メッセージが日本語になっている
- `crates/nghttp2-sys/build.rs` / `tests/` 配下の `.rs` ファイルに `.unwrap()` が残っていない
- `CHANGES.md` の `## develop` の `### misc` サブセクションに `[UPDATE]` エントリが追加されている (issue 番号なし)
- `cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 参照

- `issues/0075-refactor-replace-unwrap-with-expect.md` — 先行 issue (`examples/` の `.unwrap()` 整理)。本 issue のスコープ外として分離された経緯が書かれている
- `shiguredo-rust` スキル — `.unwrap()` ではなく `.expect("MESSAGE")` を使う規約
- `shiguredo-changelog` スキル — `### misc` サブセクションの扱い
- `crates/nghttp2-sys/build.rs` — `.unwrap()` 6 箇所の対象
- `tests/` — リポジトリルートの integration test の対象
