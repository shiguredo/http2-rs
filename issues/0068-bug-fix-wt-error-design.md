# WtError の Display 情報漏洩と Error 変換での情報消失を修正する

- Priority: High
- Created: 2026-06-11
- Polished: {Polished}
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-error-design

## 目的

`WtError` に関する以下の 2 つの問題を同時に修正する:

1. **Display の情報漏洩**: `WtError::Display` がファイルパス・行番号・バックトレースをユーザー向け出力に含めている（Error 型では issue 0055 で修正済みなのに WtError が未対応）
2. **Error 変換での情報消失**: `crates/tokio-http2/src/webtransport.rs` の `wt_err` 関数が WtError を文字列化して `Error::InvalidArgument` に押し込んでおり、kind / reason / location / backtrace が全て失われる

これら 2 つは相互依存しているため同時に修正する必要がある。Display から location/backtrace を削除するだけでは `wt_err` 経由で伝播する情報がさらに減少する。

## 現状の問題

### 問題 1: Display の情報漏洩

`src/webtransport/error.rs:145-163`:

- `WtError::Debug` (145-149) が `write!(f, "{self}")` で Display を呼んでおり、Debug と Display の責務分離が完全に崩壊している
- `WtError::Display` (151-163) が `location.file()`, `location.line()`, `backtrace` を出力している

### 問題 2: Error 変換での情報消失

`crates/tokio-http2/src/webtransport.rs:1053-1055`:

```rust
fn wt_err(e: shiguredo_http2::webtransport::WtError) -> Error {
    Error::InvalidArgument(format!("webtransport: {e}"))
}
```

WtError の kind / reason / location / backtrace が全て `Display` 経由の文字列化で失われ、tokio-http2 側では「無効な引数」としてしかエラーを認識できない。

`Error` 型 (`src/error.rs:285-293`) は issue 0055 で以下の正しい分離が実現されている:
- `Display`: kind + reason のみ
- `Debug`: kind + reason + location + alternate format でのみ backtrace

## 完了条件

### Display 情報漏洩修正

- `WtError::Display` が kind + reason のみを出力すること
- `WtError::Debug` が `Display` を呼ばず、独自に location を出力し、alternate format でのみ backtrace を出力すること
- `WtError::Display` から `location.file()`, `location.line()`, `backtrace` 出力が削除されていること

### Error 変換修正

- `crates/tokio-http2/src/error.rs` に `Error::WebTransport(WtError)` バリアントが追加されていること
- `wt_err` 関数が `Error::WebTransport(e)` に置き換えられていること
- 既存の `wt_err` 呼び出し箇所のエラーハンドリングが変更後も正しく動作すること

### 共通

- 既存のテストが全て通過すること
- CHANGES.md `## develop` に `[FIX]` エントリを追加すること（2 件を 1 エントリにまとめてよい）

## 解決方法

### 1. WtError::Display / Debug 修正 (`src/webtransport/error.rs`)

`Error` 型 (`src/error.rs:271-293`) と同じパターンに修正する:

- `Debug`: kind + reason + location を直接出力。`f.alternate()` かつ backtrace が captured の場合のみ backtrace を出力
- `Display`: kind + reason のみを出力

### 2. Error 変換修正 (`crates/tokio-http2/`)

`crates/tokio-http2/src/error.rs` にバリアントを追加:

```rust
WebTransport(shiguredo_http2::webtransport::WtError),
```

`crates/tokio-http2/src/webtransport.rs` の `wt_err` 関数:

```rust
fn wt_err(e: shiguredo_http2::webtransport::WtError) -> Error {
    Error::WebTransport(e)
}
```

## 参照

- issue 0055 (`issues/closed/0055-bug-fix-error-display-info-leak.md`) — Error 型の Display 情報漏洩修正（先行事例）
- `src/error.rs:271-293` — 修正後の Error::Debug / Error::Display（模範実装）
- `src/webtransport/error.rs:145-163` — 修正対象の WtError::Debug / WtError::Display
- `crates/tokio-http2/src/webtransport.rs:1053-1055` — 修正対象の `wt_err` 関数
- `crates/tokio-http2/src/error.rs` — Error 型定義（バリアント追加先）
