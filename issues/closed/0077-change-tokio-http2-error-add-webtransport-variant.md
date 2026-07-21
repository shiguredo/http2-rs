# tokio-http2 のエラー型を整理して WtError 由来の情報を保持する

- Priority: Medium
- Created: 2026-06-12
- Completed: 2026-07-21
- Polished: 2026-07-21
- Model: Opus 4.7
- Branch: feature/change-wt-draft15-remaining

## 目的

`crates/tokio-http2` の `wt_err` 関数が `WtError` を文字列化して `Error::InvalidArgument(String)` に押し込んでいるため、`WtError` の `kind` / `reason` / `location` / `backtrace` が全て失われている。本 issue は `Error` enum に `WebTransport(WtError)` バリアントを追加し、`tokio-http2` 利用者が構造化されたエラーマッチングを行えるようにする。

issue 0068 (`bug-fix-wt-error-design`、`WtError::Display` 情報漏洩修正) のスコープ外として明示的に分離された作業。0068 は `src/webtransport/error.rs` の `Display`/`Debug` 修正のみを対象とした最小修正であり、`crates/tokio-http2` 側のエラー型整理は本 issue で扱う。

## 優先度根拠

- 0068 で `WtError::Display` の情報漏洩を修正すると、`WtError` 自体は適切に情報を保持できるようになる。しかし `tokio-http2` 側で文字列化される現状の経路では、その情報が利用者に届かない
- `tokio-http2` 利用者は現状 `matches!(err, Error::InvalidArgument(_))` でしか分岐できず、`SessionClosed` / `FlowControlError` / `StreamStateError` 等の `WtError` の種別判定ができない (誤った再接続戦略を選ぶリスク)
- 修正コストは中程度 (バリアント追加と `wt_err` 呼び出し 11 箇所 + 直接文字列化 2 箇所の計 13 箇所の置換)
- `shiguredo_http2` クレートは未リリースのため、Error enum へのバリアント追加 (SemVer 上 breaking change) を許容できる窓のうちに済ませる必要がある
- ただし本 issue は情報漏洩そのものやメモリ安全性の直接修正ではなく、主な効果は構造化エラー情報の保持と利用者 API の改善であるため Priority は Medium とする

## 現状の問題

`crates/tokio-http2/src/webtransport.rs` の `wt_err` 関数 (line 1054-1056):

```rust
fn wt_err(e: shiguredo_http2::webtransport::WtError) -> Error {
    Error::InvalidArgument(format!("webtransport: {e}"))
}
```

driver 内 11 箇所 (line 738, 775, 789, 803, 814, 829, 853, 854, 918, 937, 960) から `.map_err(wt_err)` で呼び出されており、`WtError` の構造化情報が全て失われる。

加えて、同ファイル内に `wt_err` を経由せず直接 `WtError` を文字列化する経路が 2 箇所ある:

- line 204 の `WtSession::initiate()` 失敗時 (`WtError` を受けて文字列化):

  ```rust
  .map_err(|e| Error::InvalidArgument(format!("failed to initiate WT session: {e}")))?;
  ```

- line 187 の `WtInit::parse()` 失敗時 (`WtInit::parse` は `Result<Self, WtError>` を返す):

  ```rust
  return Err(Error::InvalidArgument(format!(
      "WebTransport-Init parse error: {e}"
  )));
  ```

これら 2 箇所も `WtError` を文字列化しており、`Error::WebTransport` 経由に統一できる。

なお、同ファイルには `WtError` とは無関係な `Error::InvalidArgument(format!(...))` 経路も存在する (TLS バージョン拒否 line 141、Origin 拒否 line 169、ストリームリセット line 493/546)。これらは本 issue の対象外であり、`InvalidArgument` のまま据え置く。

`crates/tokio-http2/src/error.rs` の現状の `Error` enum には `WebTransport` バリアントが存在しない。`#[non_exhaustive]` も付いていないため、バリアント追加は SemVer 上 breaking change。

## 設計方針

### `Error` enum へのバリアント追加

`crates/tokio-http2/src/error.rs` の `pub enum Error` に以下のバリアントを追加する:

```rust
WebTransport(shiguredo_http2::webtransport::WtError),
```

### `Display` / `source()` の更新

- `Display` 実装の match arm に `Error::WebTransport(e) => write!(f, "webtransport error: {}", e)` を追加
- `source()` の match arm に `Error::WebTransport(e) => Some(e)` を追加 (`WtError` は `std::error::Error` を実装済み)

`Error::WebTransport` の `Display` 出力は `WtError` の `Display` 実装に委譲する。`WtError::Display` の形式は issue 0068 で修正される (現状は location / backtrace を含む情報漏洩あり)。0068 マージ後は `WtError::Display` が `<kind>: <reason>` 形式になるため、`Error::WebTransport` の表示も自動的に `webtransport error: <kind>: <reason>` になる。

### `From<WtError> for Error` 実装の追加

`?` 演算子で透過変換できるように `impl From<shiguredo_http2::webtransport::WtError> for Error` を追加する。これにより `wt_err` 関数自体が不要になる。

### 既存呼び出しの置換

- `wt_err` 関数を削除する
- driver 内 11 箇所の `.map_err(wt_err)` を `?` に置換する (`From<WtError> for Error` 実装により `?` で透過変換される)
- line 204 の `initiate` 失敗経路も `.map_err(|e| Error::InvalidArgument(...))` から `?` に置換する
- line 187 の `WtInit::parse` 失敗経路も `?` に置換する (`WtInit::parse` は `Result<Self, WtError>` を返すため `From` 実装で透過変換可能)

### `WtError` / `WtErrorKind` の再エクスポート

`crates/tokio-http2/src/lib.rs` で以下を再エクスポートし、利用者が `tokio_http2::WtError` / `tokio_http2::WtErrorKind` で参照できるようにする:

```rust
pub use shiguredo_http2::webtransport::{WtError, WtErrorKind};
```

利用者は `Error::WebTransport(e)` をマッチした後に `e.kind` (0070 マージ後はアクセサ経由) で `WtErrorKind` を取得し、`SessionClosed` / `FlowControlError` / `StreamStateError` 等の種別判定ができるようになる。

## 完了条件

- `crates/tokio-http2/src/error.rs` の `Error` enum に `WebTransport(WtError)` バリアントが追加されている
- `Display` / `source()` の match arm が更新されている
- `impl From<WtError> for Error` が追加されている
- `wt_err` 関数が削除され、driver 内の 11 箇所 + 直接文字列化 2 箇所 (line 187, 204) の計 13 箇所がすべて `?` 経由に置換されている
- `crates/tokio-http2/src/lib.rs` で `WtError` と `WtErrorKind` が再エクスポートされている
- `Error::WebTransport(e)` の `source()` が `Some(&WtError)` を返すこと、`Display` が `webtransport error: ...` 形式になることを検証する単体テストが追加されている
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
- `crates/tokio-http2/src/webtransport.rs` line 1054-1056 — `wt_err` 関数 (削除対象)
- `crates/tokio-http2/src/webtransport.rs` line 187, 204 — 直接文字列化経路 (統一対象)
- `src/webtransport/error.rs` — `WtError` / `WtErrorKind` 型定義 (`std::error::Error` 実装済み)
- `src/webtransport/init.rs` — `WtInit::parse` (`Result<Self, WtError>` を返す)
- `issues/0070-change-privatize-error-wt-error-fields.md` — `WtError` のフィールド private 化。0070 マージ後に本 issue をマージするのが安全 (impl 内アクセス維持のため技術的競合はないが、コンフリクト回避)
