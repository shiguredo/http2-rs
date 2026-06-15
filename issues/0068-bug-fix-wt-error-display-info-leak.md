# WtError::Display からのソースコード位置・バックトレース情報漏洩を修正し、Debug 実装を独立させる

- Priority: High
- Created: 2026-06-11
- Polished: 2026-06-15
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-error-display-info-leak

## 目的

`src/webtransport/error.rs` の `WtError::Display` 実装が `self.location.file()` / `self.location.line()` / `self.backtrace` を常に出力する。さらに `WtError::Debug` 実装も `write!(f, "{self}")` で `Display` に委譲しているため、`Debug` 経路でも同じ内部情報が出力される。

本 issue では `Display` から location と backtrace を削除して情報漏洩を防ぎつつ、`Debug` は開発者向け診断用途として location を通常フォーマットに残し、backtrace は alternate format かつ `BacktraceStatus::Captured` のときのみ出力するように、独立した実装に変更する。

issue 0055 (`Error::Display` 情報漏洩修正) で「`WtError` は別 issue で対応する」と予告された残課題に該当する。

## 優先度根拠

`WtError::Display` 出力が以下 15 箇所で `tokio-http2` 利用者へ間接的に露出する:

- `crates/tokio-http2/src/webtransport.rs` の `wt_err` 関数: `format!("webtransport: {e}")` で `WtError::Display` を呼ぶ。driver 内で `wt_err` を 13 箇所から呼び出している (`.map_err(wt_err)` 11 箇所 + `Err(wt_err(e))` 2 箇所)
- 同ファイルの `accept()` 内 2 箇所: `format!("WebTransport-Init parse error: {e}")` (WebTransport-Init パース失敗)、`format!("failed to initiate WT session: {e}")` (WT セッション initiate 失敗) で `WtError` を直接 format している

加えて:
- `RUST_BACKTRACE` 環境変数設定時は `Display` 出力に全バックトレースが追加で露出する
- `Display` はユーザー向けメッセージに使われ、本番環境でログやエラー応答を通じて間接的に利用者に届く可能性がある
- 現状の `Debug` 実装は `Display` に委譲しているため、`{:?}` や `{:#?}` 経由のログ出力でもファイルパス・行番号・バックトレースが情報漏洩源になる。特に `tracing` の `?record` 構文は `{:?}` に展開されるため、ログ経由で意図せず流出するリスクが高い

## 現状

`src/webtransport/error.rs` の `WtError::Debug` (現行 line 145-149) は `write!(f, "{self}")` のみで `Display` に委譲しており、独自実装を持たない。

`WtError::Display` (現行 line 151-163) は `location.file()` / `location.line()` を常に、`backtrace` を Captured 時に常に出力する形になっており、L157 `write!(f, " (at {}:{})", self.location.file(), self.location.line())?;` と L158-L160 (`backtrace.status() == BacktraceStatus::Captured` のとき `"\n\nBacktrace:\n{}"` を追記) が情報漏洩源。

## 設計方針

`Error` 型 (`src/error.rs`、0055 で先行修正済み) と同じ責務分離に揃える。`Display` は kind+reason のみ、`Debug` 通常は kind+reason+location、`Debug` alternate (`{:#?}`) かつ `BacktraceStatus::Captured` のときのみ backtrace を追加する独立実装にする。`#[derive(Debug)]` を採用しないのは alternate/Captured 条件付き出力を制御するため。

副作用: `derive(Debug)` で得られる構造体フィールド分解形式 (`WtError { kind: ..., reason: ..., ... }`) ではなく Display 風カスタム出力になる。`{err:?}` 経由でテスト失敗時メッセージを観察しているコード (`tests/test_webtransport/integration.rs`、`tests/test_webtransport/root.rs`) の出力体験が変わり、修正後は location までで止まる。バックトレースを参照したい場合は `{err:#?}` を使う。

## スコープ外

- `crates/tokio-http2` 側のエラー型整理 (`wt_err` 関数の置き換え、`Error::WebTransport(WtError)` バリアント新設) は issue 0077 で扱う
- 構造体フィールド (`kind` / `reason` / `location` / `backtrace`) の可視性変更、および既存テストファイルの `err.kind` / `err.reason` 直接アクセスから getter 呼び出しへの置換は issue 0070 で扱う
- `src/webtransport/error.rs` 内の draft-14 コメントの draft 番号更新は issue 0074 で扱う

## 他 issue との関係

- **issue 0055** (`Error::Display` 情報漏洩修正、closed): 同種の修正を `Error` 型に対して先行実施済み。本 issue は `WtError` 版で、設計・テスト件数・テスト命名はこれと対称
- **issue 0070** (`Error` / `WtError` フィールド private 化): 本 issue は 0070 より先にマージされる前提。同一モジュール内の impl はフィールド可視性に関係なく直接アクセス可能なため技術的競合なし。本 issue の impl ブロックでは `self.kind` / `self.reason` / `self.location` / `self.backtrace` の直接アクセスを維持する (0070 設計とも整合)
- **issue 0072** (未使用コード削除): 本 issue は 0072 より先にマージされる前提。0072 削除予定 API (`WtErrorKind::SessionClosed`、ヘルパー `WtError::incomplete()` / `buffer_too_short()` / `session_closed()`) は本 issue のテストで使用しない。テストでは `WtError::new(WtErrorKind::Incomplete)` のようにコンストラクタを直接呼ぶ
- **issue 0065** (`add-tls-keying-material-exporter`): 本 issue が `tests/test_webtransport/main.rs` に `mod error;` を追加し、0065 が `mod exporter;` を追加する。両 issue は同ファイルを編集するため自動マージは確実にコンフリクトする。順序の合意: 本 issue が先にマージされる前提とし、0065 側がコンフリクト解消する。最終アルファベット順は `capsule` → `error` → `exporter` → `flow_control` → 残り
- **issue 0074** (refs draft-15 更新): 本 issue マージ後に実施。draft-14 コメントの置換は 0074 で機械的に行う
- **issue 0077** (`tokio-http2` 側のエラー型整理): 本 issue マージ後に実施。`wt_err` 関数の置換は 0077 で扱う

`WtErrorKind` の Display 形式 (バリアント名のみを返す現行実装) は本 issue では変更しない。本 issue のテストは `WtErrorKind::InvalidInput` の Display 出力が `"InvalidInput"` であること、および `WtErrorKind::Incomplete` の Display 出力が `"Incomplete"` であることに依存して `assert_eq!` で出力形式を保証する。`WtErrorKind` の Display を将来変更する際は、本 issue のテストが先に壊れて検知できる構造になっている。

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

   - `test_display_excludes_location`: `WtError::invalid_input("test reason")` の `Display` 出力に location パターン ` (at ` が含まれないことを確認する (本質的な検証)。加えて `assert_eq!(display, "InvalidInput: test reason")` で出力形式を直接保証する。補助として `'/'` / `'\\'` (パス区切り) の不在も assert する (`(at` 不在チェックが本質的なので、これらは補助検証)
   - `test_display_excludes_backtrace`: `WtError::new(WtErrorKind::Incomplete)` と `WtError::invalid_input("test reason")` の両方で `Display` 出力に `"Backtrace"` 文字列が含まれないことを確認する。reason 空のケースでは `assert_eq!(display, "Incomplete")` も保証する
   - `test_debug_excludes_backtrace`: `WtError::new(WtErrorKind::Incomplete)` と `WtError::invalid_input("test reason")` の両方で `Debug` 通常フォーマット (`{:?}`) に `"Backtrace"` 文字列が含まれないこと、かつ `"InvalidInput"` / `"Incomplete"`、`"test reason"`、location パターン ` (at ` が含まれることを確認する。`Location::file()` は `#[track_caller]` の影響でテストファイル内の呼び出し行を指すため、固定のライブラリパスを期待しない
   - `test_debug_alternate_format_includes_location`: `WtError::invalid_input("test reason")` の `Debug` alternate format (`{:#?}`) に `"InvalidInput"`、`"test reason"`、location パターン ` (at ` が含まれることを確認する。`Backtrace::capture()` は `RUST_BACKTRACE` 未設定時に `BacktraceStatus::Disabled` を返すため、backtrace 文字列自体の検証は行わない (0055 と同じ妥協方針: `RUST_BACKTRACE` 未設定下では `{:?}` と `{:#?}` の出力は同一になるため、両者の差は CI/手元での `RUST_BACKTRACE=1` 動作時に手動確認する)

   ヘルパー `WtError::incomplete()` は使わず `WtError::new(WtErrorKind::Incomplete)` を直接使う (0072 で `incomplete()` ヘルパーは削除予定)。

4. 各テストは `format!("{err}")` / `format!("{err:?}")` / `format!("{err:#?}")` の文字列観察のみで検証し、`err.location` / `err.backtrace` 等の構造体フィールドへの直接アクセスはしない (issue 0070 のフィールド private 化が後続でマージされた際に書き換え不要にするため)
5. `tests/test_webtransport/main.rs` の `mod` 宣言群に `mod error;` を、アルファベット順を維持して `mod capsule;` の直後に追加する
6. `CHANGES.md` の `## develop` セクション内、最後の `[FIX]` エントリの直後 (`### misc` 直前) に以下の 2 行を追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置にネスト:

   ```markdown
   - [FIX] `WtError::Display` からファイルパス・行番号・バックトレースの出力を削除し、情報漏洩を防止する。`Debug` は location のみ出力し、alternate format かつ `Backtrace::Captured` のときのみバックトレースを出力する。`Debug` 実装を `Display` 委譲から独立させる
     - @voluntas
   ```

7. `cargo fmt --all -- --check` で整形違反がないことを確認する
8. `cargo test --workspace` で全テスト通過を確認する (build も兼ねるため `cargo build` は省略)
9. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する。clippy 警告が出た場合は `#[allow(...)]` で抑制せず、コード自体を修正する

## 完了条件

- `WtError::Display` 実装が `kind` と `reason` のみを出力する (reason 空のとき `kind` のみ)
- `WtError::Debug` 通常フォーマットが `kind`、`reason`、`location` を出力するが backtrace は含まない
- `WtError::Debug` alternate format が `f.alternate() && backtrace.status() == BacktraceStatus::Captured` のときのみ backtrace を含む (テストは backtrace 文字列自体の検証は行わず、alternate format パスでも location が含まれることのみ確認)
- `Debug` 実装が `Display` への委譲をやめ、独立した実装になっている
- `tests/test_webtransport/error.rs` に 4 件のテストが追加されている
- `tests/test_webtransport/main.rs` の `mod` 宣言群に `mod error;` が追加されている
- 既存の以下のテストが退行しないこと:
  - `tests/test_webtransport/init.rs`: `format!("{err}")` の出力で `Integer` / `non-negative` / `trailing comma` / `non-ASCII` / `exceeds 15 digits` 等の reason 部分文字列を `contains` で検証する箇所が 9 箇所ある。修正後の Display は `"InvalidInput: <reason>"` 形式となり `<reason>` がそのまま含まれるため通る (`src/webtransport/init.rs` 内の `WtError::invalid_input("<reason>")` のリテラル文字列を変更しない限り退行しない)
  - `tests/test_webtransport/integration.rs`: `{err:?}` をテスト失敗時メッセージに使う箇所がある (`err.reason` 直接アクセスは issue 0070 の対象)。修正後は `{err:?}` 出力から backtrace が消え location までで止まるが、これは退行ではなく仕様変更 (設計方針参照)
  - `tests/test_webtransport/root.rs`: `err.kind` 直接アクセスは issue 0070 の対象。本 issue では関与しない
  - `crates/tokio-http2/tests/test_webtransport.rs`: `format!("{err}")` で reason 部分文字列を検証する箇所は `tokio_http2::Error::InvalidArgument(format!("WebTransport-Init parse error: {e}"))` 等の wrap 元 format! が先頭一致するため、location 行削除後も `contains("WebTransport-Init parse error")` が引き続き通る
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
