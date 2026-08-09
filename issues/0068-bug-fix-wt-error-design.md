# WtError::Display がソースコード位置とバックトレースを露出する情報漏洩を修正する

- Priority: High
- Created: 2026-06-11
- Completed: 2026-08-09
- Polished: 2026-08-08
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-error-display-info-leak

## 目的

`src/webtransport/error.rs` の `impl Display for WtError` が `self.location.file()` / `self.location.line()` を常に、`self.backtrace` を `BacktraceStatus::Captured` のときに出力する。さらに `impl Debug for WtError` も `write!(f, "{self}")` で `Display` に委譲しているため、両方の経路でサーバー内部のファイルパス・行番号・バックトレースがリモート攻撃者に露出する。

issue 0055 (`Error::Display` 情報漏洩修正) で「`WtError` は別 issue で対応する」と予告された残課題に該当する。

## 優先度根拠

- 攻撃者が内部ソース構造の推測・コードパスのヒット判定・コールグラフの逆算を行える。`RUST_BACKTRACE` 環境変数設定時は全バックトレースが追加で露出する
- `Display` はユーザー向けメッセージに使われ、本番環境でそのまま利用者にエラー文字列が返される可能性がある
- `unwrap()` / `expect()` 失敗時のパニック文言 (Debug 経由) でログ・標準エラー出力に残る経路が存在する。本 issue の `test_debug_excludes_backtrace` でバックトレース不在を保証することで、これらの経路でも間接的に防御される
- `crates/tokio-http2/src/webtransport.rs` の `abort_session_with_wt_error` メソッドが返す `tokio_http2::Error` は `Error::WebTransport(WtError)` バリアントを保持し、`tokio_http2::Error` の Display 実装が `WtError::Display` に委譲するため、出力結果がエラー文字列に複製されて利用者へ露出する。本 issue で `Display` を直すことで、この間接漏洩経路も自動的に塞がる

## 現状

`src/webtransport/error.rs` の `impl Debug for WtError` は `write!(f, "{self}")` のみで `Display` に委譲しており、独自実装を持たない。

`impl Display for WtError` は `location.file()` / `location.line()` を常に、`backtrace` を Captured 時に常に出力している:

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

`Error` 型 (`src/error.rs` の `impl Debug for Error` / `impl Display for Error`) と同じ責務分離に揃える。

- `Display` では `kind` と `reason` のみを出力する (reason が空のときは `kind` のみ)
- `Debug` 通常フォーマット (`{:?}`) では `kind`、`reason`、`location` を出力する。ファイルパス・行番号は開発時の診断に有用であり、`Debug` 用途として許容される
- `Debug` alternate format (`{:#?}`) かつ `backtrace.status() == BacktraceStatus::Captured` の AND 条件のときのみ `backtrace` を出力する
- `#[derive(Debug)]` ではなく手書き `Debug` を採用するのは、上記の alternate / Captured 条件付き出力を制御するため (`Error` 型と同じ理由)
- `Debug` から `Display` への委譲をやめ、独立した実装にする

## スコープ外

- `crates/tokio-http2` 側のエラー型整理は本 issue の対象外。`Error::WebTransport(WtError)` バリアント新設と `abort_session_with_wt_error` の `Error::from` 化は issue 0077 で対応済み。本 issue は `src/webtransport/error.rs` の Display/Debug 実装のみを変更する
- `WtErrorKind` の Display 形式 (バリアント名のみを返す現行実装、`src/webtransport/error.rs` の `impl Display for WtErrorKind`) は本 issue では変更しない。本 issue のテストは `WtErrorKind::InvalidInput` の Display 出力が `"InvalidInput"` であることに依存して assert する
- `WT_CLOSE_SESSION` capsule の `reason` は draft-ietf-webtrans-http2-15 Section 6.12 の `Application Error Message` に対応する実装側フィールド名 (`src/webtransport/capsule.rs` の `Capsule::WtCloseSession`) で別物。本 issue とは無関係 (issue 0061 で別途対応済み)
- 構造体フィールド (`kind` / `reason` / `location` / `backtrace`) の可視性変更、および既存テストファイル (`tests/test_webtransport/root.rs` / `tests/test_webtransport/integration.rs` / `tests/test_error.rs` 等) の `err.kind` / `err.reason` 直接アクセスから getter 呼び出しへの置換は issue 0070 で扱う。`crates/tokio-http2/src/webtransport.rs` の `abort_session_with_wt_error` 内の `e.kind` 直接アクセスも別クレートからのアクセスであり、0070 の getter 化対象に含める必要がある (issue 0070 の変更対象一覧に列挙されていないため、0070 実装時に確認する)。本 issue は 0070 より先にマージされる前提で、impl ブロック内のフィールド直接アクセスのみを利用する (impl は同一モジュール内なので可視性変更後も動作する)

## 対応手順

1. `src/webtransport/error.rs` の `impl Display for WtError` から `location` と `backtrace` の出力を削除し、`kind` と `reason` のみを出力する
2. `impl Debug for WtError` を `Display` 委譲から独立した実装に変更する:
   - 通常フォーマット: `kind`、`reason`、`location`
   - `f.alternate() && backtrace.status() == BacktraceStatus::Captured` の AND 条件のときのみ `backtrace` を追加出力
3. `tests/test_webtransport/error.rs` を新規作成する。issue 0055 が `tests/test_error.rs` に追加した 4 件と対称に、`tests/test_webtransport/` モジュール内既存テストの命名規約 (`test_` プレフィックス) に揃えて以下 4 件を追加する:
   - `test_display_excludes_location`: `WtError::Display` 出力にパス区切り文字 (`/`、`\`) が含まれないこと
   - `test_display_excludes_backtrace`: `WtError::Display` 出力に "Backtrace" 文字列が含まれないこと
   - `test_debug_excludes_backtrace`: `WtError::Debug` 通常フォーマット (`{:?}`) にバックトレースが含まれず、location は含まれること (パス区切り文字 (`/`) の存在検査にとどめる。0055 `tests/test_error.rs` と同じ方針)
   - `test_debug_alternate_accepts_backtrace`: `WtError::Debug` alternate format (`{:#?}`) で location が含まれること。alternate format 経路で location が保持される smoke test として位置づける (出力された backtrace 文字列の検証は行わない。`WtError` のコンストラクタは内部で `Backtrace::capture()` を呼ぶため、`RUST_BACKTRACE` 未設定の CI 環境では `Captured` 状態の `WtError` を構築できない。`Backtrace::force_capture()` で `Captured` 状態を作るにはフィールド直接アクセスが必要だが、これは issue 0070 のフィールド private 化と衝突する。テスト内で `std::env::set_var("RUST_BACKTRACE", "1")` を設定する方法はフィールド直接アクセス不要だが、unsafe (Rust 2024 edition) かつ並列テストへのグローバル副作用があるため採用しない。0055 `tests/test_error.rs` と同じ妥協方針)
4. 4 件のテスト全体で reason 空ケースと reason 非空ケースの両方をカバーする (Display 側は `test_display_excludes_backtrace` が reason 空、他 3 件が reason 非空。Debug 側は reason 非空のみで、reason 空は `WtError::new(WtErrorKind::Incomplete)` の Debug 出力を検証するテストは追加しない。0055 との対称性を優先する。実装時に Debug の reason 空分岐の簡易検証を足すかは実装者判断)。reason 空のテストは `WtError::new(WtErrorKind::Incomplete)` を、reason 非空のテストは `WtError::invalid_input("test reason")` のような呼び出しを用いる。`WtError::incomplete()` / `WtError::buffer_too_short()` / `WtError::session_closed()` および `WtErrorKind::SessionClosed` は issue 0072 で削除予定のためテストでは使用しない (本 issue は 0072 より先にマージされる前提)
5. テストは `format!("{err}")` / `format!("{err:?}")` / `format!("{err:#?}")` の文字列観察のみで検証し、`wt_error.location` / `wt_error.backtrace` 等の構造体フィールドへの直接アクセスはしない (issue 0070 のフィールド private 化が後続でマージされた際に書き換え不要にするため)
6. `tests/test_webtransport/main.rs` の `mod` 宣言群にアルファベット順を維持して `mod error;` を追記する。追記後の main.rs 全体は以下の 10 行になる:

   ```rust
   mod capsule;
   mod error;
   mod exporter;
   mod flow_control;
   mod init;
   mod integration;
   mod protocols;
   mod root;
   mod stream;
   mod varint;
   ```

7. `CHANGES.md` の `## develop` セクション内の既存 `[FIX]` 群の末尾に以下のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネストする。本 issue は情報漏洩というバグの修正であり、リリース時の最終差分として `[FIX]` に記載する (issue 0072 が扱う未リリース API の削除とは異なり、修正内容は最終リリースに残るため):

   ```markdown
   - [FIX] `WtError::Display` からファイルパス・行番号・バックトレースの出力を削除し、情報漏洩を防止する。`Debug` は kind・reason・location を出力し、alternate format かつ `Backtrace::Captured` のときのみバックトレースを出力する。`Debug` 実装を `Display` 委譲から独立させる
     - @voluntas
   ```

8. `cargo fmt --all -- --check` で整形違反がないことを確認する
9. `cargo test --workspace` で全テスト通過を確認する
10. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する。clippy 警告が出た場合は `#[allow(...)]` で抑制せず、コード自体を修正する

## 完了条件

- `WtError::Display` 実装が `kind` と `reason` のみを出力する (reason 空のとき `kind` のみ)
- `WtError::Debug` 通常フォーマットが `kind`、`reason`、`location` を出力するが backtrace は含まない
- `WtError::Debug` alternate format が `f.alternate() && backtrace.status() == BacktraceStatus::Captured` の AND 条件のときのみ backtrace を含む (テストは backtrace 文字列自体の検証は行わず、alternate format パスでも location が含まれることのみ確認)
- `Debug` 実装が `Display` への委譲をやめ、独立した実装になっている
- `tests/test_webtransport/error.rs` に 4 件のテストが追加されている
- `tests/test_webtransport/main.rs` の `mod` 宣言群に `mod error;` がアルファベット順で追記されている (`mod capsule;` と `mod exporter;` の間)
- `CHANGES.md` の `## develop` に `[FIX]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する

## 解決方法

### `WtError::Debug` / `WtError::Display` の修正 (`src/webtransport/error.rs`)

上記の設計に従い、`src/webtransport/error.rs` の `impl Debug for WtError` を `Display` 委譲から独立した実装に、`impl Display for WtError` から `location` / `backtrace` の出力を削除して `kind` / `reason` のみの出力に変更した。実装は本セクションの設計どおり完了し、`cargo fmt` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過することを確認した。`RUST_BACKTRACE=1` 設定時と未設定時の両方で新規テスト 4 件が通過することを確認済み。

`Error` 型 (`src/error.rs` の `impl Debug for Error` / `impl Display for Error`) と同じパターンで以下に置き換える。`Debug` 実装内で `write!(f, "{self}")` ではなく `write!(f, "{}", self.kind)` を使うのは、前者だと `Debug` 出力が `Display` の形式 (kind と reason のみ) に暗黙に結合され、将来 `Display` の形式が変わると `Debug` 出力も連動して変わるため。`Debug` を `Display` から独立させるのが本 issue の目的の一つであり、責務分離を構造的に保つ。

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

`tests/test_webtransport/error.rs` の冒頭と 4 件のテストの雛形。

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

/// WtError::Display 出力に "Backtrace" 文字列が含まれず、reason 空時は kind のみになること
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
        debug.contains("InvalidInput"),
        "Debug 通常にエラー種別が含まれていること: {debug}"
    );
    assert!(
        debug.contains("test reason"),
        "Debug 通常に理由が含まれていること: {debug}"
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
