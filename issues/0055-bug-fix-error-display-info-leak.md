# Error::Display がソースコード位置とバックトレースを常に漏洩する情報漏洩を修正する

- Priority: High
- Created: 2026-06-06
- Model: DeepSeek V4 Pro

## 目的

`src/error.rs:277-288` の `Display` 実装が `self.location.file()` / `self.location.line()` / `self.backtrace` を常に出力する。`Debug` 実装 (line 271-274) も `Display` に委譲しているため、両方の経路でサーバー内部のファイルパス・行番号・バックトレースがリモート攻撃者に露出する。

## 優先度根拠

- 攻撃者が内部ソース構造の推測、特定コードパスのヒット判定、コールグラフの逆算を行える
- `RUST_BACKTRACE` 環境変数設定時は全バックトレースが露出する
- `Display` はユーザー向けメッセージに使われることが一般的であり、本番環境でそのまま利用者に返されると情報漏洩になる

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

`Debug` 実装 (line 271-274):

```rust
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}
```

`Debug` が `Display` に委譲しているため、`{:?}` でも同様に漏洩する。

## 設計方針

- `Display` では `kind` と `reason` のみを出力し、`location` と `backtrace` は出力しない
- `Debug` では `location` を含めるが `backtrace` は通常フォーマットでは出力せず、alternate format (`{:#?}`) に限定する

## 完了条件

- `Display` 実装がソースコード位置とバックトレースを出力しなくなっている
- `Debug` 通常フォーマットでもバックトレースが出力されない
- エラーメッセージのテストが通過する

## 解決方法

1. 作業ブランチ `feature/fix-error-display-info-leak` を切る
2. `src/error.rs` の `Display` 実装から `location` と `backtrace` の出力を削除する
3. `Debug` 実装を `Display` 委譲から独立した実装に変更する
4. `Debug` に alternate format でのみ `backtrace` を出力するロジックを追加する
5. `cargo test --all` で全通過を確認する
