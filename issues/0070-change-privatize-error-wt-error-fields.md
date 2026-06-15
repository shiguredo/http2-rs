# Error と WtError のフィールドを private 化する

- Priority: High
- Created: 2026-06-11
- Polished: 2026-06-15
- Model: deepseek-v4-pro
- Branch: feature/change-error-and-wt-error-field-privatization

## 目的

`Error` (`src/error.rs`) と `WtError` (`src/webtransport/error.rs`) の全フィールド (`kind`, `reason`, `location`, `backtrace`) が `pub` で公開されており、外部コードがフィールドに直接代入することで `#[track_caller]` で記録した `Location` や `Backtrace::capture()` で取得したスタック情報を上書き可能になっている。専用コンストラクタ (`new`, `with_reason`, `connection_error` 等) で構築する設計の前提を構造的に保証するため、フィールドを private 化し getter 経由でのみ読み取れるようにする。

`Limits` (issue 0028) / `Settings` (issue 0043) / `HeaderField` (issue 0024) で確立された「フィールド private 化 + getter 提供」の方針に揃える。

## 優先度根拠

- 公開 API のフィールド private 化は破壊的変更 (SemVer の major bump 相当) のため、`shiguredo_http2` クレートが publish される前に完了させる必要がある (現状 develop ブランチで `CHANGES.md` の `## develop` に複数の破壊的変更が積まれており、本 issue もその一環として扱う)
- 0068 / 0072 との順序依存があるため、それらと近接した時期に対応する必要がある (詳細は「他 issue との関係」)

## 現状の問題

`src/error.rs` の `Error` 構造体定義 (現行 line 185-199):

```rust
pub struct Error {
    pub kind: ErrorKind,
    pub reason: String,
    pub location: &'static Location<'static>,
    pub backtrace: Backtrace,
}
```

`src/webtransport/error.rs` の `WtError` 構造体定義 (現行 line 56-70):

```rust
pub struct WtError {
    pub kind: WtErrorKind,
    pub reason: String,
    pub location: &'static Location<'static>,
    pub backtrace: Backtrace,
}
```

問題点:

- `kind` / `reason` / `location` / `backtrace` のいずれも `pub` のため外部から再代入可能。特に `backtrace` を `Backtrace::disabled()` で上書きする / `location` を別箇所の `Location` で上書きする等で `#[track_caller]` 由来のスタック情報が壊される
- 全フィールド `pub` のため、外部コードが構造体リテラル経由で `Error` / `WtError` を構築でき、専用コンストラクタを通らない経路が成立してしまう

## 不変条件

private 化により次の不変条件を保証する:

- `location` は `#[track_caller]` で取得した呼び出し元情報を保持する (外部からの上書き不可)
- `backtrace` は `Backtrace::capture()` のスナップショット (外部からの上書き不可)
- `reason` はコンストラクタが受け取った `Into<String>` 由来の文字列 (途中変更不可)
- `kind` はコンストラクタが指定したエラー種別 (途中変更不可)

## 設計方針

### getter の追加

`Limits` / `Settings` の getter パターンに揃える:

- `ErrorKind` / `WtErrorKind` は `#[derive(Copy)]` 済みなので値返し
- `reason()` も `pub const fn reason(&self) -> &str { self.reason.as_str() }` で実装する (`String::as_str` は Rust 1.84.0 で const stable 化済み。本リポジトリの MSRV は `Cargo.toml` の `rust-version = "1.88"` なので利用可能)
- 全 getter に `#[must_use]` を付与する (`Limits` / `Settings` の getter と整合)
- `location()` は `&'static Location<'static>` を返す (`Location<'static>` は `Copy` だが慣例的に参照を返す)
- `backtrace()` は `&Backtrace` を返す (`Backtrace` は非 `Copy`)
- doc コメントは名詞句で揃える (`Settings` 先行事例 `src/settings.rs:246-249` の形式に整合)

### setter は提供しない

`Limits` / `Settings` と同じく setter は設けない。ミューテーション経路は既存のコンストラクタ (`Error::new` / `Error::with_reason` / `Error::connection_error` / `Error::stream_error` / `Error::hpack_error` / `Error::protocol_error` / `Error::frame_size_error` / `From<DecodeError>` および対応する `WtError::*`) のみに限定する。`WtError::*` のうち `incomplete()` / `buffer_too_short()` / `session_closed()` の 3 ヘルパーは 0072 で削除予定だが、本 issue では削除対象外。

### impl ブロック内のフィールド直接アクセスは維持

`src/error.rs` の `impl Error` ブロック内 (各コンストラクタ、`is_connection_error` / `is_stream_error` / `error_code`、`Debug` / `Display` 実装) は同一モジュール内なのでフィールド可視性に関係なく直接アクセス可能。これらは getter 経由に書き換えず、`self.kind` / `self.reason` / `self.location` / `self.backtrace` のままにする (`src/webtransport/error.rs` の `impl WtError` も同様)。

### `Error::source()` / `WtError::source()` の挙動は変更しない

`Error` / `WtError` ともに `impl std::error::Error for ...` のデフォルト実装 (`source() -> None`) のみ。本 issue でも source override は追加しない (挙動変更なし)。

## 構造体フィールドの doc コメント

現行の構造体定義にある各フィールドの doc コメント (`/// 発生したエラーの種類` 等) は private 化後もそのまま残す。`Settings` 先行事例 (`src/settings.rs`) でもフィールド側 doc コメントが維持されており、getter 側にも別途 doc コメントを書く方針。両側に同じ内容のコメントを書くのではなく、フィールド側は「内部実装の説明」、getter 側は「公開 API の説明」として書き分ける。具体例: `kind` フィールド側は「発生したエラーの種類 (コンストラクタが指定)」、getter 側 `kind()` は「エラー種別」のように、フィールド側で内部生成元を、getter 側で公開 API 説明を述べる。

## 1 issue / 1 branch にまとめる根拠

`Error` と `WtError` は以下の理由で 1 issue / 1 branch にまとめる:

- 両者ともフィールド構成 (`kind` / `reason` / `location` / `backtrace`) が同一
- 両者とも `#[track_caller]` + `Backtrace::capture()` という構築パターンが同一
- 追加する getter のシグネチャ (`kind()` / `reason()` / `location()` / `backtrace()`) が同一
- カテゴリは両方 `change` で、レビュー観点・テスト戦略・リスク評価が同一
- 分割しても両 issue が `tests/test_webtransport/` と `tests/test_error.rs` の getter 書き換えを完全対称に行うだけで、独立性のメリットがない
- `CHANGES.md` には 2 件の `[CHANGE]` エントリに分けて記載することで、変更単位の追跡可能性は確保する

## 他 issue との関係

- **issue 0068** (`bug-fix-wt-error-display-info-leak`): 本 issue 0070 は 0068 マージ後にマージされる前提。技術的競合はない (同一モジュール impl はフィールド可視性に関係なくアクセス可) が、同一行への変更でマージ衝突が発生するため順序を守る
- **issue 0072** (`refactor-remove-unused-code`): 0072 で削除予定の API (`WtError::incomplete()` / `buffer_too_short()` / `session_closed()`、`WtErrorKind::SessionClosed`) は本 issue の getter 追加・テスト書き換えで使用しない。テストでは `WtError::invalid_input(...)` / `WtError::new(WtErrorKind::Incomplete)` 等を用いる。0070 → 0072 / 0072 → 0070 どちらの順序でも衝突なし
- **issue 0071** (`refactor-remove-send-error`) / **issue 0077** (`change-tokio-http2-error-add-webtransport-variant`): いずれも `Error` / `WtError` フィールド private 化とは独立。順序依存なし

## 変更対象ファイル一覧

### `Error` / `WtError` 構造体定義の private 化

- `src/error.rs` の `Error` 構造体定義 (現行 line 185-199): 全フィールドから `pub` を削除、getter 4 個を追加
- `src/webtransport/error.rs` の `WtError` 構造体定義 (現行 line 56-70): 全フィールドから `pub` を削除、getter 4 個を追加

### 外部からのフィールド直接アクセスを getter 経由に置換

`Error` / `WtError` の `pub` フィールドへの直接アクセスを grep で網羅した結果、以下の 17 箇所を確認した。これらを getter 呼び出しに置き換える:

- `tests/test_error.rs:36,37,46` — `err.reason.contains(...)` → `err.reason().contains(...)` (3 箇所)
- `tests/test_webtransport/root.rs:85,136` — `err.kind` → `err.kind()` (Copy 値比較、2 箇所)
- `tests/test_webtransport/integration.rs:40,91,108,114,133,382,404` — `err.kind` → `err.kind()` (7 箇所)
- `tests/test_webtransport/integration.rs:407` — `err.reason.contains(...)` → `err.reason().contains(...)` (1 箇所)
- `src/webtransport/capsule.rs:333,344` — `e.kind == WtErrorKind::Incomplete` → `e.kind() == WtErrorKind::Incomplete` (別モジュールなので private 化後はアクセス不可、getter 経由が必要、2 箇所)
- `src/connection/headers.rs:287,615` — `format!("HPACK decode error: {}", e.reason)` → `format!("HPACK decode error: {}", e.reason())` (2 箇所、`e` は `hpack_decoder.decode()` が返す `crate::error::Result<_>` の `Err` 中身 = `crate::error::Error` 型、`src/hpack/decoder.rs:3` で `use crate::error::{Error, Result};` を確認済み)

### 除外対象 (本 issue と無関係)

- `crates/tokio-http2/src/webtransport.rs` の `Capsule::WtCloseSession { reason }` は draft-ietf-webtrans-http2-14 capsule の reason フィールドで別物。スコープ外
- `crates/tokio-http2/src/webtransport.rs:1053-1055` の `wt_err` 関数は `format!("webtransport: {e}")` で `WtError::Display` を呼ぶのみ。Display 経由のため private 化の影響なし
- `pbt/tests/prop_error.rs` の `error_kind_strategy` 等は `ErrorKind` enum を生成する関数で、`Error` 構造体のフィールドアクセスではない
- `fuzz/fuzz_targets/` / `examples/` 配下に `Error` / `WtError` のフィールド直接アクセスは存在しない (grep 確認済み)
- `From<DecodeError> for Error` (`src/error.rs`) は内部で `Error::connection_error` を呼ぶのみで、フィールド直接アクセスはない

## テスト方針

- 既存テスト (`tests/test_error.rs` / `tests/test_webtransport/root.rs` / `tests/test_webtransport/integration.rs`) のフィールド直接アクセスを getter 呼び出しに書き換える。assert 意図 (左右の値) は変更しない
- getter ラウンドトリップ用の新規 PBT (`prop_error_getter_roundtrip` 等) は本 issue では **追加しない**。`Settings` private 化 (issue 0043) と同方針。理由: `Error` / `WtError` の `location` / `backtrace` は `#[track_caller]` / `Backtrace::capture()` 由来で、property test で任意値を与えて確認できる対象ではない。getter ラウンドトリップ可能なのは `kind` / `reason` のみで、その動作は impl が trivial (フィールドそのまま返却) なので個別 unit test なしでも既存テスト経由で十分検証される

## 対応手順

1. 作業ブランチ `feature/change-error-and-wt-error-field-privatization` を作成する
2. `src/error.rs` の `Error` 構造体定義から全フィールドの `pub` を削除し、以下 4 個の getter を追加する:

   ```rust
   impl Error {
       /// エラー種別
       #[must_use]
       pub const fn kind(&self) -> ErrorKind {
           self.kind
       }

       /// エラー理由
       #[must_use]
       pub const fn reason(&self) -> &str {
           self.reason.as_str()
       }

       /// エラー発生位置
       #[must_use]
       pub const fn location(&self) -> &'static Location<'static> {
           self.location
       }

       /// バックトレース
       #[must_use]
       pub const fn backtrace(&self) -> &Backtrace {
           &self.backtrace
       }
   }
   ```

3. `src/webtransport/error.rs` の `WtError` 構造体定義に対し 2 と同じ作業を行う (`WtErrorKind` 版):

   ```rust
   impl WtError {
       /// エラー種別
       #[must_use]
       pub const fn kind(&self) -> WtErrorKind {
           self.kind
       }

       /// エラー理由
       #[must_use]
       pub const fn reason(&self) -> &str {
           self.reason.as_str()
       }

       /// エラー発生位置
       #[must_use]
       pub const fn location(&self) -> &'static Location<'static> {
           self.location
       }

       /// バックトレース
       #[must_use]
       pub const fn backtrace(&self) -> &Backtrace {
           &self.backtrace
       }
   }
   ```

4. 「変更対象ファイル一覧」セクションの「外部からのフィールド直接アクセスを getter 経由に置換」で挙げた 17 箇所を順次 getter 呼び出しに書き換える。テスト本体の assert 意図は変更しない
5. `CHANGES.md` の `## develop` セクション内の `[CHANGE]` 群の末尾 (= `[FIX]` セクションの直前) に以下 2 件のエントリを追加する。担当者行は親アイテム本文先頭 (`[` カラム) と同じ位置 (スペース 2 個 + `-` + スペース + `@<name>`) にネストする:

   ```markdown
   - [CHANGE] `Error` のフィールドを private 化し、getter `kind()` / `reason()` / `location()` / `backtrace()` を追加する
     - @voluntas
   - [CHANGE] `WtError` のフィールドを private 化し、getter `kind()` / `reason()` / `location()` / `backtrace()` を追加する
     - @voluntas
   ```

6. `cargo fmt --all -- --check` で整形違反がないことを確認する
7. `cargo test --workspace` で全テスト通過を確認する (既存の `tests/test_error.rs` / `tests/test_webtransport/*` が getter 書き換え後も意図通り通ること、`pbt/` / `fuzz/` の既存ビルドが退行しないこと)
8. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する。clippy 警告が出た場合は `#[allow(...)]` で抑制せず、コード自体を修正する

## 完了条件

- `Error` の全フィールドから `pub` が削除され、`kind()` / `reason()` / `location()` / `backtrace()` getter 経由でのみ読み取り可能になっている
- `WtError` の全フィールドから `pub` が削除され、`kind()` / `reason()` / `location()` / `backtrace()` getter 経由でのみ読み取り可能になっている
- 全 getter に `#[must_use]` が付与され、`pub const fn` で実装されている
- `Error` / `WtError` への setter は提供されていない (ミューテーション経路は既存コンストラクタのみ)
- `impl std::error::Error for Error` / `impl std::error::Error for WtError` は空 impl のまま (source override 追加なし)
- 既存のフィールド直接アクセス 17 箇所 (`tests/test_error.rs` / `tests/test_webtransport/*` / `src/webtransport/capsule.rs` / `src/connection/headers.rs`) が getter 呼び出しに置き換えられている
- `CHANGES.md` の `## develop` に 2 件の `[CHANGE]` エントリ (`Error` 用と `WtError` 用) と担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する

## 参照

- `issues/closed/0028-change-limits-builder-result.md` — `Limits` のフィールド private 化先行事例
- `issues/closed/0043-change-settings-field-privatization.md` — `Settings` のフィールド private 化先行事例
- `issues/closed/0024-change-header-field-construct-time-validation.md` — `HeaderField` のフィールド private 化先行事例
- `src/settings.rs` の `Settings` の getter 群 — `#[must_use]` + `pub const fn` + 名詞句 doc コメントのシグネチャ参考
- `src/limits.rs` の `Limits` の getter 群 — 同上
