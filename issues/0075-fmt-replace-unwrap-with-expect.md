# examples/ の .unwrap() を .expect() に置換する

- Priority: Low
- Created: 2026-06-11
- Polished: {Polished}
- Model: deepseek-v4-pro
- Branch: feature/fix-replace-unwrap-with-expect

## 目的

examples/ コードの `.unwrap()` を `.expect("MESSAGE")` に置換し、shiguredo-rust 規約に準拠させる。

## 現状の問題

shiguredo-rust 規約: `.unwrap()` ではなく `.expect("MESSAGE")` を使うこと。

以下の 5 箇所で `.unwrap()` が使用されている:

### 1-2. `examples/http2_client/src/main.rs:56,58`

```rust
HeaderField::new(":path", path).unwrap(),
HeaderField::new(":authority", format!("{host}:{port}")).unwrap(),
```

`:path` や `:authority` が動的値のため `from_static` が使えず、`new` の Result を `.unwrap()` で処理している。

### 3-4. `examples/http2_server/src/main.rs:194,196`

```rust
HeaderField::new(":status", status).unwrap(),
HeaderField::new("content-length", format!("{len}")).unwrap(),
```

### 5. `examples/wt_server/src/main.rs:272`

```rust
noargs::opt("listen").with_default("127.0.0.1:4443".to_string()).unwrap()
```

この `.unwrap()` は戻り値型が `Result<_, Infallible>` のため panic しないが、規約上一律 `.expect()` にすべき。

## 完了条件

- 上記 5 箇所の `.unwrap()` がすべて `.expect("MESSAGE")` に置換されていること
- expect メッセージは日本語であること（CLAUDE.md 規約: コメントは全て日本語）
- `cargo build --workspace` が成功すること
- CHANGES.md `## develop` に `[UPDATE]` エントリを追加すること

## 解決方法

各 `.unwrap()` を `.expect("エラーメッセージ")` に置換する。メッセージ例:

- `HeaderField::new(":path", path).expect(":path が不正な文字を含んでいます")`
- `HeaderField::new(":authority", ...).expect(":authority が不正な文字を含んでいます")`

## 参照

- `examples/http2_client/src/main.rs:56,58`
- `examples/http2_server/src/main.rs:194,196`
- `examples/wt_server/src/main.rs:272`
- CLAUDE.md / AGENTS.md — `.unwrap()` ではなく `.expect()` を使う規約
