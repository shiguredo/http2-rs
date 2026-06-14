# WtError::Display からのソースコード位置・バックトレース情報漏洩を修正し、Debug 実装を独立させる

- Priority: High
- Created: 2026-06-11
- Polished: 2026-06-14
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-error-display-info-leak

## 目的

`src/webtransport/error.rs` の `WtError::Display` 実装が `self.location.file()` / `self.location.line()` / `self.backtrace` を常に出力する。さらに `WtError::Debug` 実装も `write!(f, "{self}")` で `Display` に委譲しているため、`Debug` 経路でも同じ内部情報が出力される。

本 issue では `Display` から location と backtrace を削除して情報漏洩を防ぎつつ、`Debug` は開発者向け診断用途として location を通常フォーマットに残し、backtrace は alternate format かつ `BacktraceStatus::Captured` のときのみ出力するように、独立した実装に変更する。

issue 0055 (`Error::Display` 情報漏洩修正) で「`WtError` は別 issue で対応する」と予告された残課題に該当する。

## 優先度根拠

- `crates/tokio-http2/src/webtransport.rs` の `wt_err` 関数が `format!("webtransport: {e}")` で `WtError::Display` を呼んでおり、出力結果が `Error::InvalidArgument` の文字列に複製されて `tokio-http2` の利用者へ露出する。driver 内では `wt_err` 関数を 13 箇所から呼び出している (`map_err(wt_err)` 11 箇所 + `match` arm 内の `Err(wt_err(e))` 2 箇所)。`Display` を直すことで、この間接漏洩経路も塞がる
- `crates/tokio-http2/src/webtransport.rs` の `accept()` 内でも `WtError` を直接 `format!` して返却エラーに含めている箇所がある (WebTransport-Init パースエラー、WT セッション initiate 失敗)。`Display` を直すことで、これらの経路でも内部情報が `tokio-http2` 利用者へ露出しなくなる
- `RUST_BACKTRACE` 環境変数設定時は `Display` 出力に全バックトレースが追加で露出する
- `Display` はユーザー向けメッセージに使われ、本番環境でログやエラー応答を通じて間接的に利用者に届く可能性がある
- 現状の `Debug` 実装は `Display` に委譲しているため、`:?` や `:#?` 経由のログ出力でもファイルパス・行番号・バックトレースが情報漏洩源になる

## 現状

`src/webtransport/error.rs` の `WtError::Debug` (line 145-149) は `write!(f, "{self}")` のみで `Display` に委譲しており、独自実装を持たない。

`WtError::Display` (line 151-163) は `location.file()` / `location.line()` を常に、`backtrace` を Captured 時に常に出力している:

```rust
impl std::fmt::Display for WtError {
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

## 設計方針

`Error` 型 (`src/error.rs`) と同じ責務分離に揃える。

- `Display` では `kind` と `reason` のみを出力する (reason が空のときは `kind` のみ)。これはエンドユーザー・ライブラリ利用者に返すメッセージであり、内部情報を含めない
- `Debug` 通常フォーマット (`{:?}`) では `kind`、`reason`、`location` を出力する。ファイルパス・行番号は開発者向け診断に有用であり、`Debug` 用途として許容される
- `Debug` alternate format (`{:#?}`) かつ `backtrace.status() == BacktraceStatus::Captured` のときのみ `backtrace` を出力する
- `#[derive(Debug)]` ではなく手書き `Debug` を採用するのは、上記の alternate / Captured 条件付き出力を制御するため (`Error` 型と同じ理由)
- `Debug` から `Display` への委譲をやめ、独立した実装にする

## スコープ外

- `crates/tokio-http2` 側のエラー型整理 (`wt_err` 関数の置き換え、`Error::WebTransport(WtError)` バリアント新設など) は issue 0077 で扱う。本 issue は `src/webtransport/error.rs` の Display/Debug 実装のみを変更する
- `WtErrorKind` の Display 形式 (バリアント名のみを返す現行実装) は本 issue では変更しない。本 issue のテストは `WtErrorKind::InvalidInput` の Display 出力が `"InvalidInput"` であること、および `WtErrorKind::Incomplete` の Display 出力が `"Incomplete"` であることに依存して assert する
- 構造体フィールド (`kind` / `reason` / `location` / `backtrace`) の可視性変更、および既存テストファイル (`tests/test_webtransport/root.rs` / `tests/test_webtransport/integration.rs` / `tests/test_error.rs` 等) の `err.kind` / `err.reason` 直接アクセスから getter 呼び出しへの置換は issue 0070 で扱う。本 issue は 0070 より先にマージされる前提で、impl ブロック内のフィールド直接アクセスのみを利用する (impl は同一モジュール内なので可視性変更後も動作する)

## 他 issue との関係

- **issue 0055** (`Error::Display` 情報漏洩修正): 同種の修正を `Error` 型に対して先行して実施済み。本 issue は `WtError` 版
- **issue 0070** (`Error` / `WtError` フィールド private 化): 本 issue は 0070 より先にマージされる前提。同一モジュール内の impl はフィールド可視性に関係なく直接アクセス可能なため、技術的競合はない
- **issue 0072** (未使用コード削除): 本 issue は 0072 より先にマージされる前提。0072 で削除予定の API (`WtErrorKind::SessionClosed`、`WtError::incomplete` / `buffer_too_short` / `session_closed`) は本 issue のテストで使用しない
- **issue 0065** (`add-tls-keying-material-exporter`): `tests/test_webtransport/main.rs` に `mod error;` を追加する本 issue と、`mod exporter;` を追加する 0065 がともに `mod capsule;` 直後の挿入位置を扱う。アルファベット順は `capsule` → `error` → `exporter` → `flow_control` なので、実装時に重複・欠落がないよう手動で調整する
- **issue 0074** (refs draft-15 更新): `src/webtransport/error.rs` 内の draft-14 コメントを機械的に置換するため、本 issue マージ後に実施する
- **issue 0077** (`tokio-http2` 側のエラー型整理): 本 issue のスコープ外。`wt_err` 関数の置き換え等は 0077 で扱う。本 issue マージ後に実施する

## 対応手順

1. `src/webtransport/error.rs` の `Display` 実装から `location` と `backtrace` の出力を削除し、`kind` と `reason` のみを出力する
2. `Debug` 実装を `Display` 委譲から独立した実装に変更する:
   - 通常フォーマット: `kind`、`reason`、`location`
   - `f.alternate() && backtrace.status() == BacktraceStatus::Captured` のときのみ `backtrace` を追加出力
3. `tests/test_webtransport/error.rs` を新規作成する。ファイル先頭には以下のような日本語のモジュール説明コメントを置く:

   ```rust
   //! WtError の Display / Debug 出力が内部情報を漏洩しないことを検証する
   ```

   `use` 文は以下を最低限含む:

   ```rust
   use shiguredo_http2::webtransport::{WtError, WtErrorKind};
   ```

   issue 0055 (`tests/test_error.rs`) と対称に、`tests/test_webtransport/` モジュール内既存テストの命名規約 (`test_` プレフィックス) に揃えて以下 4 件を追加する。4 件のテスト全体で reason 空ケースと reason 非空ケースをそれぞれカバーする。各テスト関数には日本語の doc コメントを付け、assert メッセージも日本語とする:

   - `test_display_excludes_location`: `WtError::invalid_input("test reason")` の `Display` 出力にパス区切り文字 (`/`、`\`)、かつ location パターン ` (at ` が含まれないことを確認する。加えて `assert_eq!(display, "InvalidInput: test reason")` で出力形式を直接保証する
   - `test_display_excludes_backtrace`: `WtError::new(WtErrorKind::Incomplete)` と `WtError::invalid_input("test reason")` の両方で `Display` 出力に `"Backtrace"` 文字列が含まれないことを確認する。reason 空のケースでは `assert_eq!(display, "Incomplete")` も保証する
   - `test_debug_excludes_backtrace`: `WtError::new(WtErrorKind::Incomplete)` と `WtError::invalid_input("test reason")` の両方で `Debug` 通常フォーマット (`{:?}`) に `"Backtrace"` 文字列が含まれないこと、かつ `"InvalidInput"` / `"Incomplete"`、`"test reason"`、location パターン ` (at ` が含まれることを確認する。`Location::file()` は `#[track_caller]` の影響でテストファイル内の呼び出し行を指すため、固定のライブラリパスを期待しない
   - `test_debug_alternate_format_includes_location`: `WtError::invalid_input("test reason")` の `Debug` alternate format (`{:#?}`) に `"InvalidInput"`、`"test reason"`、location パターン ` (at ` が含まれることを確認する。`Backtrace::capture()` は `RUST_BACKTRACE` 未設定時に `BacktraceStatus::Disabled` を返すため、backtrace 文字列自体の検証は行わない (0055 と同じ妥協方針)

4. 各テストは `format!("{err}")` / `format!("{err:?}")` / `format!("{err:#?}")` の文字列観察のみで検証し、`err.location` / `err.backtrace` 等の構造体フィールドへの直接アクセスはしない (issue 0070 のフィールド private 化が後続でマージされた際に書き換え不要にするため)
5. `tests/test_webtransport/main.rs` の `mod` 宣言群に `mod error;` を、アルファベット順を維持して `mod capsule;` の直後に追加する
6. `CHANGES.md` の `## develop` セクション内、既存 `[FIX]` エントリの末尾 (`CHANGES.md` 上で最後の `[FIX]` エントリの直後、`### misc` 直前) に以下のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネストする:

   ```markdown
   - [FIX] `WtError::Display` からファイルパス・行番号・バックトレースの出力を削除し、情報漏洩を防止する。`Debug` は location のみ出力し、alternate format かつ `Backtrace::Captured` のときのみバックトレースを出力する。`Debug` 実装を `Display` 委譲から独立させる
     - @voluntas
   ```

7. `cargo fmt --all -- --check` で整形違反がないことを確認する
8. `cargo test --workspace` で全テスト通過を確認する
9. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する。clippy 警告が出た場合は `#[allow(...)]` で抑制せず、コード自体を修正する

## 完了条件

- `WtError::Display` 実装が `kind` と `reason` のみを出力する (reason 空のとき `kind` のみ)
- `WtError::Debug` 通常フォーマットが `kind`、`reason`、`location` を出力するが backtrace は含まない
- `WtError::Debug` alternate format が `f.alternate() && backtrace.status() == BacktraceStatus::Captured` のときのみ backtrace を含む (テストは backtrace 文字列自体の検証は行わず、alternate format パスでも location が含まれることのみ確認)
- `Debug` 実装が `Display` への委譲をやめ、独立した実装になっている
- `tests/test_webtransport/error.rs` に 4 件のテストが追加されている
- `tests/test_webtransport/main.rs` の `mod` 宣言群に `mod error;` が追加されている
- 既存の以下のテストが退行しないこと:
  - `tests/test_webtransport/init.rs` (`format!("{err}")` で reason 部分文字列を検証)
  - `tests/test_webtransport/integration.rs` (`{err:?}` によるテスト失敗時メッセージ、`err.reason` 直接アクセスは issue 0070 の対象)
  - `tests/test_webtransport/root.rs` (`err.kind` 直接アクセスは issue 0070 の対象)
  - `crates/tokio-http2/tests/test_webtransport.rs` (`format!("{err}")` で reason 部分文字列を検証。検証対象は接頭辞のみのため、Display から location/backtrace が削除されても影響を受けにくい)
- `CHANGES.md` の `## develop` に `[FIX]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する

## 解決方法

### `WtError::Debug` / `WtError::Display` の修正 (`src/webtransport/error.rs`)

`Error` 型 (`src/error.rs`) と同じパターンで以下に置き換える:

```rust
impl std::fmt::Debug for WtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        if !self.reason.is_empty() {
            write!(f, ": {}", self.reason)?;
        }
        write!(f, " (at {}:{})", self.location.file(), self.location.line())?;
        if f.alternate() && self.backtrace.status() == BacktraceStatus::Captured {
            write!(f, "\n\nBacktrace:\n{}", self.backtrace)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for WtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        if !self.reason.is_empty() {
            write!(f, ": {}", self.reason)?;
        }
        Ok(())
    }
}
```

### テスト方針

`tests/test_webtransport/error.rs` を新規作成し、issue 0055 (`tests/test_error.rs`) と対称の 4 件のテストを追加する。テストでは文字列観察のみを用い、`err.location` / `err.backtrace` 等の構造体フィールドには直接アクセスしない。

## 後方互換性への影響

`WtError::Display` / `Debug` の出力形式が変わるため、旧形式に依存するコードは破壊される可能性がある。本変更は情報漏洩防止を目的とした意図的な修正である。
