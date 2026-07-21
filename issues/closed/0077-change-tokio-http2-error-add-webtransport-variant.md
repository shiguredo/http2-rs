# tokio-http2 のエラー型を整理して WtError 由来の情報を保持する

- Priority: Medium
- Created: 2026-06-12
- Completed: 2026-07-21
- Polished: {Polished}
- Model: Opus 4.7
- Branch: feature/change-wt-draft15-remaining

## 目的

`crates/tokio-http2` の `wt_err` 関数が `WtError` を文字列化して `Error::InvalidArgument(String)` に押し込んでいるため、`WtError` の `kind` / `reason` / `location` / `backtrace` が全て失われている。本 issue は `Error` enum に `WebTransport(WtError)` バリアントを追加し、`tokio-http2` 利用者が構造化されたエラーマッチングを行えるようにする。

issue 0068 (`bug-fix-wt-error-design`、`WtError::Display` 情報漏洩修正) のスコープ外として明示的に分離された作業。0068 は `src/webtransport/error.rs` の `Display`/`Debug` 修正のみを対象とした最小修正であり、`crates/tokio-http2` 側のエラー型整理は本 issue で扱う。

## 優先度根拠

- 0068 で `WtError::Display` の情報漏洩を修正すると、`WtError` 自体は適切に情報を保持できるようになる。しかし `tokio-http2` 側で文字列化される現状の経路では、その情報が利用者に届かない
- `tokio-http2` 利用者は現状 `matches!(err, Error::InvalidArgument(_))` でしか分岐できず、`SessionClosed` / `FlowControlError` / `StreamStateError` 等の `WtError` の種別判定ができない (誤った再接続戦略を選ぶリスク)
- 修正コストは中程度 (バリアント追加と `wt_err` 呼び出し 13 箇所の置換)
- `shiguredo_http2` クレートは未リリースのため、Error enum へのバリアント追加 (SemVer 上 breaking change) を許容できる窓のうちに済ませる必要がある
- ただし本 issue は情報漏洩そのものやメモリ安全性の直接修正ではなく、主な効果は構造化エラー情報の保持と利用者 API の改善であるため Priority は Medium とする

## 現状の問題

`crates/tokio-http2/src/webtransport.rs` の `wt_err` 関数 (現状 line 1053-1055 付近):

```rust
fn wt_err(e: shiguredo_http2::webtransport::WtError) -> Error {
    Error::InvalidArgument(format!("webtransport: {e}"))
}
```

driver 内 13 箇所から `.map_err(wt_err)` で呼び出されており、`WtError` の構造化情報が全て失われる。

加えて、同ファイル内の類似経路:

- `crates/tokio-http2/src/webtransport.rs:203` の `WtSession::initiate()` 失敗時:

  ```rust
  Error::InvalidArgument(format!("failed to initiate WT session: {e}"))
  ```

- `crates/tokio-http2/src/webtransport.rs:186` の WebTransport-Init parse error 経路:

  ```rust
  Error::InvalidArgument(format!("WebTransport-Init parse error: {e}"))
  ```

も同様に `WtError` を文字列化している。

`crates/tokio-http2/src/error.rs` の現状の `Error` enum (line 7-19) には `WebTransport` バリアントが存在しない。`#[non_exhaustive]` も付いていないため、バリアント追加は SemVer 上 breaking change。

## 設計方針

### `Error` enum へのバリアント追加

`crates/tokio-http2/src/error.rs` の `pub enum Error` に以下のバリアントを追加する:

```rust
WebTransport(shiguredo_http2::webtransport::WtError),
```

### `Display` / `source()` の更新

- `Display` 実装の match arm に `Error::WebTransport(e) => write!(f, "webtransport error: {}", e)` を追加
- `source()` の match arm に `Error::WebTransport(e) => Some(e)` を追加 (`WtError` は `std::error::Error` を実装済み)

### `From<WtError> for Error` 実装の追加

`?` 演算子で透過変換できるように `impl From<shiguredo_http2::webtransport::WtError> for Error` を追加する。これにより `wt_err` 関数自体が不要になる。

### 既存呼び出しの置換

- `wt_err` 関数を削除する
- driver 内 13 箇所の `.map_err(wt_err)` を `?` または `.map_err(Error::from)` に置換する
- `crates/tokio-http2/src/webtransport.rs:203` の `initiate` 失敗経路も `Error::WebTransport(e)` 経由 (`From` 実装経由) に統一する
- `crates/tokio-http2/src/webtransport.rs:186` の WebTransport-Init parse error 経路も `Error::WebTransport(e)` 経由に統一するかは要検討 (parse error は `WtError::invalid_input` 由来のため統一可能)

### `WtError` の再エクスポート

`crates/tokio-http2/src/lib.rs` で `pub use shiguredo_http2::webtransport::WtError` を再エクスポートし、利用者が `tokio_http2::WtError` で参照できるようにする。

## 完了条件

- `crates/tokio-http2/src/error.rs` の `Error` enum に `WebTransport(WtError)` バリアントが追加されている
- `Display` / `source()` の match arm が更新されている
- `impl From<WtError> for Error` が追加されている
- `wt_err` 関数が削除され、driver 内の全呼び出し箇所が `?` または `From` 経由に置換されている
- `crates/tokio-http2/src/webtransport.rs:186,203` の文字列化経路も `Error::WebTransport` 経由に統一されている
- `crates/tokio-http2/src/lib.rs` で `WtError` が再エクスポートされている
- `Error::WebTransport(e)` の `source()` が `Some(&WtError)` を返すこと、`Display` が `webtransport error: <kind>: <reason>` 形式になることを検証する単体テストが追加されている
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリが追加されている
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 解決方法

- `crates/tokio-http2/src/error.rs` に `Error::WebTransport(WtError)` を追加し、`Display` / `source()` / `From<WtError>` を実装した。`wt_err` を削除し driver 内を `Error::from` に置換した。
- セッション終了系の `WtErrorKind` (`StreamStateError` / `FlowControlError` / `SessionStateError`) を受信したとき、CONNECT ストリームへ `RST_STREAM` (`WT_STREAM_STATE_ERROR` / `WT_FLOW_CONTROL_ERROR` / `WT_ERROR`) を送ってから `Error::WebTransport` を返すようにした。
- `tokio_http2::WtError` を re-export した。`CHANGES.md` に `[CHANGE]` を追記した。
- draft-15 残り対応と同じブランチ `feature/change-wt-draft15-remaining` で実装した。

## 参照

- `issues/0068-bug-fix-wt-error-design.md` — 先行 issue (WtError::Display 情報漏洩修正)。本 issue のスコープ外として分離された経緯が書かれている
- `crates/tokio-http2/src/error.rs` — `Error` enum 定義 (バリアント追加先)
- `crates/tokio-http2/src/webtransport.rs:1053-1055` — `wt_err` 関数 (削除対象)
- `crates/tokio-http2/src/webtransport.rs:186,203` — 文字列化経路 (統一対象)
- `src/webtransport/error.rs` — `WtError` 型定義 (`std::error::Error` 実装済み)
- `issues/0070-change-privatize-error-wt-error-fields.md` — `WtError` のフィールド private 化。0070 マージ後に本 issue をマージするのが安全 (impl 内アクセス維持のため技術的競合はないが、コンフリクト回避)
