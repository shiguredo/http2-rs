# Error と WtError の pub フィールドを private 化する

- Priority: High
- Created: 2026-06-11
- Polished: {Polished}
- Model: deepseek-v4-pro
- Branch: feature/change-privatize-error-fields

## 目的

`Error` と `WtError` の全フィールド (`kind`, `reason`, `location`, `backtrace`) が `pub` で公開されており、外部コードが自由に読み書きできる状態を修正する。

専用コンストラクタ (`new`, `with_reason`, `connection_error` 等) で構築する設計にも関わらず、フィールドが直接改変可能なため不変条件を保証できない。

## 現状の問題

`src/error.rs:185-199`:

```rust
pub struct Error {
    pub kind: ErrorKind,
    pub reason: String,
    pub location: &'static Location<'static>,
    pub backtrace: Backtrace,
}
```

`src/webtransport/error.rs:56-70`:

```rust
pub struct WtError {
    pub kind: WtErrorKind,
    pub reason: String,
    pub location: &'static Location<'static>,
    pub backtrace: Backtrace,
}
```

問題点:
- `backtrace` が `pub` なため再代入可能
- `location` が `pub` なため `#[track_caller]` で設定した呼び出し元情報を書き換え可能
- `reason` が `pub` なためエラーメッセージを外部から改変可能
- `shiguredo_http2` クレート外の `crates/tokio-http2/` 等のエラー変換経路で整合性が破壊されるリスクがある

## 完了条件

- `Error` の全フィールドが private 化され、getter メソッド経由でのみ読み取り可能になっていること
- `WtError` の全フィールドが private 化され、getter メソッド経由でのみ読み取り可能になっていること
- getter メソッドとして以下が追加されていること:
  - `kind() -> &ErrorKind` / `kind() -> &WtErrorKind`
  - `reason() -> &str`
  - `location() -> &'static Location<'static>`
  - `backtrace() -> &Backtrace`（読み取り専用参照のみ）
- 既存のフィールド直接アクセス (`error.kind`, `error.reason` 等) を getter メソッド呼び出しに置き換えること
- PBT / fuzz ターゲット等の外部クレートからのアクセスも getter 経由に修正すること
- CHANGES.md `## develop` に `[CHANGE]` エントリを追加すること

## 解決方法

1. `src/error.rs` の `Error` 構造体:
   - 全フィールドから `pub` を削除
   - getter メソッドを追加

2. `src/webtransport/error.rs` の `WtError` 構造体:
   - 全フィールドから `pub` を削除
   - getter メソッドを追加

3. 全コードベースで `error.kind` → `error.kind()` 等の置き換え（`src/`, `crates/tokio-http2/`, `tests/`, `pbt/`, `fuzz/`）

## 参照

- `src/error.rs:185-199` — Error 構造体定義
- `src/webtransport/error.rs:56-70` — WtError 構造体定義
- issue 0043 (Settings フィールド private 化) — 同様の private 化の先行事例
- issue 0024 (HeaderField フィールド private 化) — 同様の先行事例
