# examples/ の .unwrap() を .expect() に置換する

- Priority: Low
- Created: 2026-06-11
- Polished: 2026-06-12
- Model: deepseek-v4-pro
- Branch: feature/refactor-replace-unwrap-with-expect

## 目的

`examples/` 配下のコードで使用されている `.unwrap()` を `.expect("MESSAGE")` に置換し、`shiguredo-rust` 規約「`.unwrap()` ではなく `.expect("MESSAGE")` を使うこと」に準拠させる。

## 優先度根拠

- `shiguredo-rust` 規約「`.unwrap()` ではなく `.expect("MESSAGE")` を使うこと」に違反している既存箇所の整理。機能には影響しないが、規約準拠のために必要
- `examples/` は利用者がコピー&ペーストで参考にする可能性が高いため、規約違反のコードを残すと利用者の `.unwrap()` 使用を誘発する
- 修正コストは極小 (5 箇所のテキスト置換と日本語 expect メッセージの追加)
- Priority: Low の理由: 機能挙動に影響せず、未リリースのため緊急性も低い

## 現状の問題

`shiguredo-rust` 規約: `.unwrap()` ではなく `.expect("MESSAGE")` を使うこと (理由: panic 時のメッセージで「想定外なのか / 仕様上絶対起きないのか」を区別できるようにするため)。

`examples/` 配下を grep した結果、以下の 5 箇所で `.unwrap()` が使われている (`grep -rn "\.unwrap()" examples/` で確認):

### 1-2. `examples/http2_client/src/main.rs:56,58`

```rust
HeaderField::new(":path", path).unwrap(),
HeaderField::new(":authority", format!("{host}:{port}")).unwrap(),
```

`:path` や `:authority` が動的値のため `from_static` が使えず、`new` の `Result<_, HeaderFieldError>` を `.unwrap()` で処理している。HeaderField 構築時検査で失敗する条件 (`:path` に HTTP/2 で禁止された文字を含む等) は呼び出し元の責任。

### 3-4. `examples/http2_server/src/main.rs:194,196`

```rust
HeaderField::new(":status", status).unwrap(),
HeaderField::new("content-length", body.len().to_string()).unwrap(),
```

`body.len().to_string()` で得た数値文字列が `HeaderField::new` の検査を通らないケースはほぼないが、規約上 `.expect("...")` で意図を明示する。

### 5. `examples/wt_server/src/main.rs:272`

```rust
let listen: String = noargs::opt("listen")
    .short('l')
    .ty("ADDR")
    .doc("Listen address")
    .default(DEFAULT_LISTEN)
    .take(&mut args)
    .then(|o| Ok::<_, std::convert::Infallible>(o.value().to_string()))
    .unwrap();
```

`.then(...)` のクロージャは戻り値型を `Ok::<_, std::convert::Infallible>` で固定しているため `Result<String, Infallible>` を返す。`Infallible` は値を構築できない型のため `Err` 側が型レベルで排除されており panic することはない。規約準拠のために `.expect("infallible: then クロージャは Ok::<_, Infallible> を返す")` のような明示が必要。

## 設計方針

- 各 `.unwrap()` を `.expect("日本語メッセージ")` に置換する
- メッセージは「状況によっては発生する panic」と「仕様上絶対に発生しない panic (Infallible)」を区別できる内容にする
- 日本語メッセージとする (CLAUDE.md 規約「コメントは全て日本語にすること」)

## スコープ外

- `crates/nghttp2-sys/build.rs` の `.unwrap()` (6 箇所): build script 中のもので、`examples/` とは性質が異なる (build 時の環境変数読み込み等)。別 issue で対応する
- `src/` / `crates/*/src/` 配下の `#[cfg(test)] mod tests` 内の `.unwrap()`: テストコード内の `.unwrap()` は失敗時にテスト失敗の文脈で十分情報が得られるため、本 issue の対象外 (テストの可読性とのバランスで許容)
- `tests/` 配下の integration test の `.unwrap()`: 上記と同様の理由で本 issue では対象外。必要があれば別 issue で対応
- `pbt/` / `fuzz/` 配下: PBT / fuzz は失敗時に proptest / fuzzer のフレームワーク経由でメッセージが得られるため、本 issue の対象外

## 他 issue との関係

- 0068-0074: いずれも `examples/` の `.unwrap()` には触れない。順序依存なし
- 0076 (`fmt-translate-english-comments`): 英語コメント翻訳の別 issue で `examples/` を触る可能性は低いが、`examples/http2_client/src/main.rs` / `examples/http2_server/src/main.rs` / `examples/wt_server/src/main.rs` を編集する場合は本 issue と機械的に衝突する可能性がある。本 issue 先にマージするか、同時 PR で扱う

## CHANGES.md の扱い

本変更は `examples/` のスタイル変更で機能に影響しないため、`shiguredo-changelog` 規約「機能に直接影響しない変更 (ドキュメント追加、リファクタリング等) は `### misc` サブセクションに記載すること」に従い、`CHANGES.md` の `## develop` セクション内の `### misc` サブセクションに `[UPDATE]` エントリ 1 件を追加する。`### misc` サブセクションが存在しない場合は新規作成する。

## 変更対象ファイル一覧

### 編集するファイル

- `examples/http2_client/src/main.rs:56,58` — `.unwrap()` 2 箇所を `.expect("...")` に置換
- `examples/http2_server/src/main.rs:194,196` — `.unwrap()` 2 箇所を `.expect("...")` に置換
- `examples/wt_server/src/main.rs:272` — `.unwrap()` 1 箇所を `.expect("...")` に置換
- `CHANGES.md` — `### misc` サブセクションに `[UPDATE]` エントリ追加

## 対応手順

1. 作業ブランチ `feature/refactor-replace-unwrap-with-expect` を作成する
2. `examples/http2_client/src/main.rs:56,58` の 2 箇所を以下に置換する:

   ```rust
   HeaderField::new(":path", path).expect(":path に HTTP/2 で禁止された文字が含まれている"),
   HeaderField::new(":authority", format!("{host}:{port}")).expect(":authority に HTTP/2 で禁止された文字が含まれている"),
   ```

3. `examples/http2_server/src/main.rs:194,196` の 2 箇所を以下に置換する:

   ```rust
   HeaderField::new(":status", status).expect(":status の値が HTTP/2 で許容されない"),
   HeaderField::new("content-length", body.len().to_string()).expect("content-length の値が HTTP/2 で許容されない"),
   ```

4. `examples/wt_server/src/main.rs:272` の `.unwrap()` (上記 noargs チェーンの末尾) を以下のように `.expect("...")` に置換する:

   ```rust
   let listen: String = noargs::opt("listen")
       .short('l')
       .ty("ADDR")
       .doc("Listen address")
       .default(DEFAULT_LISTEN)
       .take(&mut args)
       .then(|o| Ok::<_, std::convert::Infallible>(o.value().to_string()))
       .expect("infallible: then クロージャは Ok::<_, Infallible> を返す");
   ```

5. `CHANGES.md` の `## develop` セクションに `### misc` サブセクションがあればその末尾に、なければ新規作成して以下のエントリと担当者行を追加する:

   ```markdown
   ### misc

   - [UPDATE] `examples/` 配下の `.unwrap()` を `.expect("理由")` に置換し、`shiguredo-rust` 規約に準拠させる (issue 0075)
     - @voluntas
   ```

6. 念のため `grep -rn "\.unwrap()" examples/` を実行し、対象の 5 箇所以外に `.unwrap()` が残っていないことを確認する
7. `cargo fmt --all -- --check` で整形違反がないことを確認する
8. `cargo build --workspace` でビルドが成功することを確認する (`examples/` も含めてビルドされる)
9. `cargo test --workspace` で全テスト通過を確認する
10. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する

## 完了条件

- `examples/http2_client/src/main.rs:56,58` の 2 箇所が `.expect("...")` に置換されている
- `examples/http2_server/src/main.rs:194,196` の 2 箇所が `.expect("...")` に置換されている
- `examples/wt_server/src/main.rs:272` の 1 箇所が `.expect("...")` に置換されている
- 各 `expect` メッセージが日本語で、「状況により発生する panic」と「仕様上絶対発生しない panic (Infallible)」を区別している
- `examples/` 配下に `.unwrap()` が残っていない (`grep -rn "\.unwrap()" examples/` で 0 件)
- `CHANGES.md` の `## develop` の `### misc` サブセクションに `[UPDATE]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo build --workspace` が成功する
- `cargo test --workspace` が成功する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する

## 参照

- `~/.claude/skills/shiguredo-rust/SKILL.md` — `.unwrap()` ではなく `.expect("MESSAGE")` を使う規約
- `~/.claude/skills/shiguredo-changelog/SKILL.md` — `### misc` サブセクションの扱い
- `examples/http2_client/src/main.rs:56,58` — 編集対象 1-2
- `examples/http2_server/src/main.rs:194,196` — 編集対象 3-4
- `examples/wt_server/src/main.rs:272` — 編集対象 5
