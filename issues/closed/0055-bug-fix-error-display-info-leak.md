# Error::Display がソースコード位置とバックトレースを露出する情報漏洩を修正する

- Priority: High
- Created: 2026-06-06
- Completed: 2026-06-06
- Model: DeepSeek V4 Pro
- Branch: feature/fix-error-display-info-leak
- Polished: 2026-06-06

## 目的

`src/error.rs:277-288` の `Display` 実装が `self.location.file()` / `self.location.line()` / `self.backtrace` を常に出力する。`Debug` 実装 (line 271-274) も `Display` に委譲しているため、両方の経路でサーバー内部のファイルパス・行番号・バックトレースがリモート攻撃者に露出する。

注: `src/webtransport/error.rs` の `WtError` も同型の問題を抱えている。本 issue では `Error` のみを対象とし、`WtError` は別 issue で対応する。

## 優先度根拠

- 攻撃者が内部ソース構造の推測、特定コードパスのヒット判定、コールグラフの逆算を行える
- `RUST_BACKTRACE` 環境変数設定時は全バックトレースが露出する
- `Display` はユーザー向けメッセージに使われ、本番環境でそのまま利用者にエラー文字列が返される可能性がある
- `tracing` の `?record` で Debug 出力がログに残る経路も存在する

## 現状

`src/error.rs:277-288`:

```rust
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        if !self.reason.is_empty() {
            write!(f, ": {}", self.reason)?;
        }
        write!(f, " (at {}:{})", self.location.file(), self.location.line())?;
        if self.backtrace.status() == BacktraceStatus::Captured {
            write!(f, "\n\nBacktrace:\n{}", self.backtrace)?;
        }
        Ok(())
    }
}
```

`Debug` 実装 (line 271-274) が `Display` に委譲している:

```rust
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}
```

## 設計方針

- `Display` では `kind` と `reason` のみを出力し、`location` と `backtrace` は出力しない
- `Debug` 通常フォーマットでは `kind`、`reason`、`location` を出力する（ファイルパス・行番号は開発時の診断に有用であり、`Debug` はその用途が許容される）
- `Debug` alternate format (`{:#?}`) でのみ `backtrace` を出力する
- `Debug` から `Display` への委譲をやめ、独立した実装にする
- `WtError`（`src/webtransport/error.rs`）は本 issue のスコープ外

## 対応手順

1. 作業ブランチ `feature/fix-error-display-info-leak` を作成する
2. `src/error.rs` の `Display` 実装から `location` と `backtrace` の出力を削除する
3. `Debug` 実装を `Display` 委譲から独立した実装に変更する:
   - 通常フォーマット: `kind`、`reason`、`location`
   - alternate format (`{:#?}`): 上記 + `backtrace`
4. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する
5. `tests/test_error.rs` に以下の単体テストを追加する:
   - `Display` 出力にファイルパスが含まれないこと
   - `Display` 出力に "Backtrace" が含まれないこと
   - `Debug` 通常フォーマット (`{:?}`) にバックトレースが含まれないこと
   - `Debug` alternate format (`{:#?}`) にバックトレースが含まれること
6. `cargo test --workspace` で全テスト通過を確認する
7. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する

## 完了条件

- `Display` 実装が `kind` と `reason` のみを出力する
- `Debug` 通常フォーマットが location を含むが backtrace を含まない
- `Debug` alternate format が backtrace を含む
- 情報漏洩を検証する単体テストが追加されている
- `CHANGES.md` の `## develop` にエントリが追加されている
- `cargo test --workspace` が通過する

## 解決方法

1. `Display` 実装から `location` (file/line) と `backtrace` の出力を削除し、`kind` と `reason` のみを出力するように変更した。
2. `Debug` 実装を `Display` 委譲から独立した実装に変更し、通常フォーマットでは `kind`、`reason`、`location` を、alternate format (`{:#?}`) でのみ `backtrace` を出力するようにした。
3. `tests/test_error.rs` に Display/Debug の情報漏洩防止を検証する単体テストを 4 件追加した。
4. `CHANGES.md` の `[FIX]` セクションにエントリを追加した。
