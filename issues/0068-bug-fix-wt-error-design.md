# WtError::Display がソースコード位置とバックトレースを露出する情報漏洩を修正する

- Priority: High
- Created: 2026-06-11
- Polished: 2026-06-11
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-error-display-info-leak

## 目的

`src/webtransport/error.rs:151-163` の `WtError::Display` 実装が `self.location.file()` / `self.location.line()` / `self.backtrace` を常に出力する。さらに `WtError::Debug` 実装 (line 145-149) も `write!(f, "{self}")` で `Display` に委譲しているため、両方の経路でサーバー内部のファイルパス・行番号・バックトレースがリモート攻撃者に露出する。

issue 0055 (`Error::Display` 情報漏洩修正) で「`WtError` は別 issue で対応する」と予告された残課題に該当する。

## 優先度根拠

- 攻撃者が内部ソース構造の推測・コードパスのヒット判定・コールグラフの逆算を行える。`RUST_BACKTRACE` 環境変数設定時は全バックトレースが追加で露出する
- `Display` はユーザー向けメッセージに使われ、本番環境でそのまま利用者にエラー文字列が返される可能性がある
- `tracing` の `?record` で Debug 出力がログに残る経路、および `unwrap()` / `expect()` 失敗時のパニック文言 (Debug 経由) でログ・標準エラー出力に残る経路が存在する。本 issue の `test_debug_excludes_backtrace` でバックトレース不在を保証することで、これらの経路でも間接的に防御される
- `crates/tokio-http2/src/webtransport.rs` の `wt_err` 関数 (現状 line 1053-1055 付近) が `format!("webtransport: {e}")` で `WtError::Display` を呼んでおり、出力結果が `Error::InvalidArgument` の文字列に複製されて利用者へ露出する。driver 内では `wt_err` 関数を 13 箇所から呼び出している (`map_err(wt_err)` 11 箇所 + 直接 2 箇所)。本 issue で `Display` を直すことで、この間接漏洩経路も自動的に塞がる

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

`Error` 型 (`src/error.rs:271-293`) と同じ責務分離に揃える。

- `Display` では `kind` と `reason` のみを出力する (reason が空のときは `kind` のみ)
- `Debug` 通常フォーマット (`{:?}`) では `kind`、`reason`、`location` を出力する。ファイルパス・行番号は開発時の診断に有用であり、`Debug` 用途として許容される
- `Debug` alternate format (`{:#?}`) かつ `backtrace.status() == BacktraceStatus::Captured` の AND 条件のときのみ `backtrace` を出力する
- `#[derive(Debug)]` ではなく手書き `Debug` を採用するのは、上記の alternate / Captured 条件付き出力を制御するため (`Error` 型と同じ理由)
- `Debug` から `Display` への委譲をやめ、独立した実装にする

## スコープ外

- `crates/tokio-http2` 側のエラー型整理 (`wt_err` 関数の置き換え、`Error::WebTransport(WtError)` バリアント新設など) は別 issue で扱う。本 issue は `src/webtransport/error.rs` の Display/Debug 実装のみを変更する
- `WtErrorKind` の Display 形式 (バリアント名のみを返す現行実装、`src/webtransport/error.rs:39-53`) は本 issue では変更しない。本 issue のテストは `WtErrorKind::InvalidInput` の Display 出力が `"InvalidInput"` であることに依存して assert する
- `WT_CLOSE_SESSION` capsule の `reason` (`src/webtransport/capsule.rs::Capsule::WtCloseSession`) は別物で、本 issue とは無関係 (issue 0061 で別途対応済み)
- 構造体フィールド (`kind` / `reason` / `location` / `backtrace`) の可視性変更、および既存テストファイル (`tests/test_webtransport/root.rs` / `tests/test_webtransport/integration.rs` / `tests/test_error.rs` 等) の `err.kind` / `err.reason` 直接アクセスから getter 呼び出しへの置換は issue 0070 で扱う。本 issue は 0070 より先にマージされる前提で、impl ブロック内のフィールド直接アクセスのみを利用する (impl は同一モジュール内なので可視性変更後も動作する)

## 対応手順

1. `src/webtransport/error.rs` の `Display` 実装から `location` と `backtrace` の出力を削除し、`kind` と `reason` のみを出力する。`use std::backtrace::{Backtrace, BacktraceStatus};` は line 5 で既に import 済みのため追加 use 宣言は不要 (`Backtrace` 型自体は `WtError` 構造体定義 line 69 で継続使用するため import は維持)
2. `Debug` 実装を `Display` 委譲から独立した実装に変更する:
   - 通常フォーマット: `kind`、`reason`、`location`
   - `f.alternate() && backtrace.status() == BacktraceStatus::Captured` の AND 条件のときのみ `backtrace` を追加出力
3. `tests/test_webtransport/error.rs` を新規作成する。冒頭の `use` 文は既存ファイル (`tests/test_webtransport/capsule.rs:1` 等) に倣う。issue 0055 が `tests/test_error.rs:134-190` に追加した 4 件と対称に、`tests/test_webtransport/` モジュール内既存テストの命名規約 (`test_` プレフィックス) に揃えて以下 4 件を追加する:
   - `test_display_excludes_location`: `WtError::Display` 出力にパス区切り文字 (`/`、`\`) が含まれないこと
   - `test_display_excludes_backtrace`: `WtError::Display` 出力に "Backtrace" 文字列が含まれないこと
   - `test_debug_excludes_backtrace`: `WtError::Debug` 通常フォーマット (`{:?}`) にバックトレースが含まれず、location は含まれること (`assert!(debug.contains('/'))` のように OS 非依存のパス区切り検査にとどめる)
   - `test_debug_alternate_accepts_backtrace`: `WtError::Debug` alternate format (`{:#?}`) で location が含まれること。`Backtrace::Captured` 分岐の実行を間接的に裏付けるため alternate format 自体は呼び出すが、出力された backtrace 文字列の検証は行わない (`Backtrace::capture()` は `RUST_BACKTRACE` 未設定時に `BacktraceStatus::Disabled` を返すため、CI 環境で実バックトレース文字列を強制取得する手段がない。0055 `tests/test_error.rs:181-190` と同じ妥協方針)
4. 各テストは reason 空ケースと reason 非空ケースの両方をカバーする。reason 空のテストは `WtError::new(WtErrorKind::Incomplete)` を、reason 非空のテストは `WtError::invalid_input("test reason")` のような呼び出しを用いる。`WtError::incomplete()` / `WtError::buffer_too_short()` / `WtError::session_closed()` および `WtErrorKind::SessionClosed` は issue 0072 で削除予定のためテストでは使用しない (本 issue は 0072 より先にマージされる前提)
5. テストは `format!("{err}")` / `format!("{err:?}")` / `format!("{err:#?}")` の文字列観察のみで検証し、`wt_error.location` / `wt_error.backtrace` 等の構造体フィールドへの直接アクセスはしない (issue 0070 のフィールド private 化が後続でマージされた際に書き換え不要にするため)
6. `tests/test_webtransport/main.rs` の `mod` 宣言群にアルファベット順を維持して `mod error;` を追記する。追記後の main.rs 全体は以下の 8 行になる:

   ```rust
   mod capsule;
   mod error;
   mod flow_control;
   mod init;
   mod integration;
   mod root;
   mod stream;
   mod varint;
   ```

7. `CHANGES.md` の `## develop` セクション内の既存 `[FIX]` 群の末尾 (issue 0055 / 0061 等のエントリの後) に以下のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネストする:

   ```markdown
   - [FIX] `WtError::Display` からファイルパス・行番号・バックトレースの出力を削除し、情報漏洩を防止する。`Debug` は location のみ出力し、alternate format かつ `Backtrace::Captured` のときのみバックトレースを出力する。`Debug` 実装を `Display` 委譲から独立させる (issue 0068)
     - @voluntas
   ```

8. `cargo fmt --all -- --check` で整形違反がないことを確認する (`prek.toml` の pre-commit hook で priority 0 として登録されているため、CI でも実行される)
9. `cargo test --workspace` で全テスト通過を確認する。`pbt/` は workspace member として実行されるが、本変更は Display/Debug の整形のみで proptest が観察するエラー作成・伝播ロジックには触れないため、proptest が新規に落ちることはない
10. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する (`--all-targets` は tests/ 配下も対象)。clippy 警告が出た場合は `#[allow(...)]` で抑制せず、コード自体を修正する

## 完了条件

- `WtError::Display` 実装が `kind` と `reason` のみを出力する (reason 空のとき `kind` のみ)
- `WtError::Debug` 通常フォーマットが `kind`、`reason`、`location` を出力するが backtrace は含まない
- `WtError::Debug` alternate format が `f.alternate() && backtrace.status() == BacktraceStatus::Captured` の AND 条件のときのみ backtrace を含む (テストは backtrace 文字列自体の検証は行わず、alternate format パスでも location が含まれることのみ確認)
- `Debug` 実装が `Display` への委譲をやめ、独立した実装になっている
- `tests/test_webtransport/error.rs` に 4 件のテストが追加されている
- `tests/test_webtransport/main.rs` の `mod` 宣言群に `mod error;` がアルファベット順で追記されている (`mod capsule;` と `mod flow_control;` の間)
- `CHANGES.md` の `## develop` に `[FIX]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する

## 解決方法

### `WtError::Debug` / `WtError::Display` の修正 (`src/webtransport/error.rs`)

`Error` 型 (`src/error.rs:271-293`) と同じパターンで以下に置き換える。`Debug` 実装内で `write!(f, "{self}")` ではなく `write!(f, "{}", self.kind)` を使うのは、前者だと Display 仕様 (reason まで描画して終わる) に従ってしまい Debug の責務 (location 必須) が崩れるため。

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

### 修正後の出力例

`#[track_caller]` の効果で `Location::caller()` は呼び出し元 = テスト関数の位置を返す:

- `Display` (`WtError::invalid_input("test reason")`): `InvalidInput: test reason`
- `Display` (`WtError::new(WtErrorKind::Incomplete)`、reason 空): `Incomplete`
- `Debug` 通常 (`{:?}`): `InvalidInput: test reason (at tests/test_webtransport/error.rs:NN)` (NN は呼び出し元の行番号)
- `Debug` alternate (`{:#?}`、`RUST_BACKTRACE=1` かつ `Captured`): 上記末尾に `\n\nBacktrace:\n...` を追加

### テスト記述例

`tests/test_webtransport/error.rs` の冒頭と 4 件のテストの雛形。残り部分は 0055 `tests/test_error.rs:134-190` と完全対称に書く。

```rust
use shiguredo_http2::webtransport::{WtError, WtErrorKind};

/// WtError::Display 出力にファイルパスが含まれないこと
#[test]
fn test_display_excludes_location() {
    let err = WtError::invalid_input("test reason");
    let display = format!("{err}");
    assert!(
        !display.contains('/'),
        "Display にファイルパスが含まれていないこと: {display}"
    );
    assert!(
        !display.contains('\\'),
        "Display にファイルパスが含まれていないこと: {display}"
    );
    assert!(
        display.contains("InvalidInput"),
        "Display にエラー種別が含まれていること: {display}"
    );
    assert!(
        display.contains("test reason"),
        "Display に理由が含まれていること: {display}"
    );
}

/// WtError::Display 出力に "Backtrace" 文字列が含まれないこと (reason 空ケース)
#[test]
fn test_display_excludes_backtrace() {
    let err = WtError::new(WtErrorKind::Incomplete);
    let display = format!("{err}");
    assert!(
        !display.contains("Backtrace"),
        "Display に Backtrace が含まれていないこと: {display}"
    );
    assert_eq!(display, "Incomplete", "reason 空時は kind のみ出力されること");
}

/// WtError::Debug (通常) にバックトレースが含まれず、location は含まれること
#[test]
fn test_debug_excludes_backtrace() {
    let err = WtError::invalid_input("test reason");
    let debug = format!("{err:?}");
    assert!(
        !debug.contains("Backtrace"),
        "Debug 通常に Backtrace が含まれていないこと: {debug}"
    );
    assert!(
        debug.contains('/'),
        "Debug 通常に location (ファイルパス) が含まれていること: {debug}"
    );
}

/// WtError::Debug (alternate) でも location が含まれること
/// (Backtrace の検証は RUST_BACKTRACE 環境依存のため行わない)
#[test]
fn test_debug_alternate_accepts_backtrace() {
    let err = WtError::invalid_input("test reason");
    let alt_debug = format!("{err:#?}");
    assert!(
        alt_debug.contains('/'),
        "Debug alternate にファイルパスが含まれていること: {alt_debug}"
    );
}
```
