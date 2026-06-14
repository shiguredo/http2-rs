# tokio-http2 のエラー型を整理して WtError 由来の情報を保持する

- Priority: Medium
- Created: 2026-06-12
- Polished: 2026-06-14
- Model: Opus 4.7
- Branch: feature/change-tokio-http2-error-add-webtransport-variant

## 目的

`crates/tokio-http2` の `wt_err` 関数が `WtError` を文字列化して `Error::InvalidArgument(String)` に押し込んでいるため、`WtError` の `kind` / `reason` / `location` / `backtrace` が全て失われている。本 issue は `Error` enum に `WebTransport(WtError)` バリアントを追加し、`tokio-http2` 利用者が構造化されたエラーマッチングを行えるようにする。

issue 0068 (`bug-fix-wt-error-display-info-leak`、`WtError::Display` 情報漏洩修正) のスコープ外として明示的に分離された作業。0068 は `src/webtransport/error.rs` の `Display`/`Debug` 修正のみを対象とした最小修正であり、`crates/tokio-http2` 側のエラー型整理は本 issue で扱う。

## 優先度根拠

- 0068 で `WtError::Display` の情報漏洩を修正すると、`WtError` 自体は適切に情報を保持できるようになる。しかし `tokio-http2` 側で文字列化される現状の経路では、その情報が利用者に届かない
- `tokio-http2` 利用者は現状 `matches!(err, Error::InvalidArgument(_))` でしか分岐できず、`SessionClosed` / `FlowControlError` / `StreamStateError` 等の `WtError` の種別判定ができない (誤った再接続戦略を選ぶリスク)
- 修正コストは中程度 (バリアント追加と `wt_err` 呼び出し 13 箇所の置換)
- `tokio-http2` クレートは未リリースのため、`tokio_http2::Error` enum へのバリアント追加 (SemVer 上 breaking change) を許容できる窓のうちに済ませる必要がある
- ただし本 issue は情報漏洩そのものやメモリ安全性の直接修正ではなく、主な効果は構造化エラー情報の保持と利用者 API の改善であるため Priority は Medium とする

## 現状の問題

`crates/tokio-http2/src/webtransport.rs` の `wt_err` 関数 (現状 line 1053-1055 付近):

```rust
fn wt_err(e: shiguredo_http2::webtransport::WtError) -> Error {
    Error::InvalidArgument(format!("webtransport: {e}"))
}
```

driver 内 13 箇所から `wt_err` が呼び出されており、`WtError` の構造化情報が全て失われる (`.map_err(wt_err)` 11 箇所、`Err(wt_err(e))` 2 箇所)。

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

`crates/tokio-http2/src/error.rs` の現状の `Error` enum (line 8-19) には `WebTransport` バリアントが存在しない。`#[non_exhaustive]` も付いていないため、バリアント追加は SemVer 上 breaking change。

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
- driver 内 13 箇所の `wt_err` 呼び出しを以下のように置換する:
  - `handle_cmd` 内の `.map_err(wt_err)` 6 箇所と `Err(wt_err(e))` 2 箇所: 結果を `ack.send(res)` 等で呼び出し元に返す必要があるため、`?` では置換せず `.map_err(Error::from)` / `Err(e.into())` に置換する
  - `handle_cmd` 外の `.map_err(wt_err)` 5 箇所 (`handle_event` / `maybe_grow_*` 等): 戻り値が `Result<()>` で呼び出し元に直接伝播してよいため、`?` を使って `From` 実装経由で変換する
- `crates/tokio-http2/src/webtransport.rs:203` の `initiate` 失敗経路も `Error::WebTransport(e)` 経由 (`From` 実装経由) に統一する
- `crates/tokio-http2/src/webtransport.rs:186` の WebTransport-Init parse error 経路も `Error::WebTransport(e)` 経由に統一する (parse error は `WtError::invalid_input` 由来のため)

### `WtError` の再エクスポート

他の WebTransport 型 (`WtBidiStream` 等) と同じ公開階層で利用できるよう、以下の 2 箇所で再エクスポートする:

- `crates/tokio-http2/src/webtransport.rs` に `pub use shiguredo_http2::webtransport::WtError;` を追加し、`tokio_http2::webtransport::WtError` として参照可能にする
- `crates/tokio-http2/src/lib.rs` の `pub use webtransport::{...}` リストに `WtError` を追加し、`tokio_http2::WtError` としても参照可能にする

## 対応手順

1. 作業ブランチ `feature/change-tokio-http2-error-add-webtransport-variant` を作成する
2. `crates/tokio-http2/src/error.rs` の `Error` enum に `/// WebTransport エラー` 付きで `WebTransport(WtError)` バリアントを追加する
3. `Display` / `source()` の match arm を更新する
4. `impl From<shiguredo_http2::webtransport::WtError> for Error` を追加する
5. `crates/tokio-http2/src/webtransport.rs` から `wt_err` 関数を削除し、以下のように呼び出しを置換する:
   - `handle_cmd` 内の `.map_err(wt_err)` 6 箇所: `.map_err(Error::from)` に置換 (`.map_err(Into::into)` でも可)
   - `handle_cmd` 内の `Err(wt_err(e))` 2 箇所 (`OpenBidi` / `OpenUni`): `Err(e.into())` に置換
   - `handle_cmd` 外の `.map_err(wt_err)` 5 箇所 (`handle_event` / `maybe_grow_*` 等) は `?` に置換して `From` 実装経由にする
   - 0065 (`add-tls-keying-material-exporter`) および 0066 (`add-wt-subprotocol-negotiation`) が先にマージされている場合、それらの変更で追加された `wt_err` 呼び出し、あるいは `WtError` を文字列化して `Error::InvalidArgument` に押し込む類似経路も、`Error::WebTransport(WtError)` 経由に統合する
6. `crates/tokio-http2/src/webtransport.rs:203` の `initiate()` 失敗経路を `?` に置換する
7. `crates/tokio-http2/src/webtransport.rs:186` の WebTransport-Init parse error 経路を `?` に置換する (400 レスポンス送信後に `return Err(e.into());`)
8. `crates/tokio-http2/src/webtransport.rs` に `pub use shiguredo_http2::webtransport::WtError;` を追加する
9. `crates/tokio-http2/src/lib.rs` の `pub use webtransport::{...}` リストに `WtError` を追加する
10. `crates/tokio-http2/src/error.rs` 内に `#[cfg(test)] mod tests` を追加し、以下を検証する単体テストを書く:
    - `Error::WebTransport(e).source()` の戻り値が `WtError` であることを確認すること (`source()` は `Option<&(dyn std::error::Error + 'static)>` を返すため、`downcast_ref::<WtError>()` で検証する)
    - `WtError::with_reason(WtErrorKind::InvalidInput, "test reason")` 由来の `Error::WebTransport` の `Display` が `webtransport error: InvalidInput: test reason` を含むこと
    - `WtError::new(WtErrorKind::Incomplete)` 由来の `Error::WebTransport` の `Display` が `webtransport error: Incomplete` を含むこと (reason 空のケース)
11. `crates/tokio-http2/tests/test_webtransport.rs` の負値 WebTransport-Init テスト (約 1187-1193 行目) を、文字列部分一致 (`"WebTransport-Init parse error"`) から `matches!(err, tokio_http2::Error::WebTransport(_))` 等に書き換える
12. `CHANGES.md` の `## develop` セクションの先頭に以下のエントリと担当者行を追加する (`shiguredo-issues` 規約により issue 番号は含めない):

    ```markdown
    - [CHANGE] `tokio_http2::Error` に `WebTransport(WtError)` バリアントを追加し、`WtError` の構造化情報を保持できるようにする
      - @voluntas
    ```

13. `cargo fmt --all -- --check` / `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過することを確認する

## 完了条件

- `crates/tokio-http2/src/error.rs` の `Error` enum に `WebTransport(WtError)` バリアントが追加され、doc コメントが付いている
- `Display` / `source()` の match arm が更新されている
- `impl From<WtError> for Error` が追加されている
- `wt_err` 関数が削除され、driver 内の全 `wt_err` 呼び出しが `.map_err(Error::from)` / `Err(e.into())` / `?` に置換されている (0065 および 0066 がマージ済みの場合、それらで追加された `wt_err` 呼び出し、あるいは `WtError` を文字列化して `Error::InvalidArgument` に押し込む類似経路も `Error::WebTransport(WtError)` 経由に統合する)
- `crates/tokio-http2/src/webtransport.rs:186,203` の文字列化経路が `Error::WebTransport` 経由に統一されている
- `crates/tokio-http2/src/webtransport.rs` と `crates/tokio-http2/src/lib.rs` で `WtError` が再エクスポートされている
- `crates/tokio-http2/src/error.rs` に `Error::WebTransport` の `source()` / `Display` を検証する単体テストが追加されている
- `crates/tokio-http2/tests/test_webtransport.rs` の負値 WebTransport-Init テストが新しいエラー型に対応している
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリが追加されている (issue 番号なし)
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通過する

## 他 issue との関係

- **0065 (`add-tls-keying-material-exporter`)**: 0065 の `DriverState::handle_cmd` 内に追加される `.map_err(wt_err)` 1 箇所も、本 issue 実装時に `Error::WebTransport(WtError)` 経由に置換する
- **0066 (`add-wt-subprotocol-negotiation`)**: 0066 の `WtServerRequest::accept()` 内で追加される `WtError` を文字列化して `Error::InvalidArgument` に押し込む経路も、本 issue 実装時に `Error::WebTransport(WtError)` 経由に統合する
- **0068 (`bug-fix-wt-error-display-info-leak`)**: 先行 issue。本 issue は 0068 マージ後にマージされる前提
- **0070 (`change-privatize-error-wt-error-fields`)**: `WtError` のフィールド private 化。0070 マージ後に本 issue をマージするのが安全
- **0072 (`refactor-remove-unused-code`)**: 0072 で `WtErrorKind::SessionClosed` が削除されるため、本 issue の単体テストでは削除されない `WtErrorKind::Incomplete` を使用する。0072 マージ後に本 issue をマージしてもコンパイルエラーにならない

## 解決方法

issue 0068 および 0070 マージ後、`crates/tokio-http2/src/error.rs` / `crates/tokio-http2/src/webtransport.rs` / `crates/tokio-http2/src/lib.rs` / `crates/tokio-http2/tests/test_webtransport.rs` を上記対応手順に従って修正する。

## 参照

- `issues/0068-bug-fix-wt-error-display-info-leak.md` — 先行 issue (WtError::Display 情報漏洩修正)。本 issue のスコープ外として分離された経緯が書かれている
- `issues/0070-change-privatize-error-wt-error-fields.md` — `WtError` のフィールド private 化。0070 マージ後に本 issue をマージするのが安全
- `shiguredo-issues` スキル — issue 番号を含めてはいけない場所 (CHANGES.md) の規約
- `shiguredo-changelog` スキル — `[CHANGE]` エントリの扱い
- `shiguredo-rust` スキル — Rust コーディング規約
- `crates/tokio-http2/src/error.rs` — `Error` enum 定義 (バリアント追加先)
- `crates/tokio-http2/src/webtransport.rs:1053-1055` — `wt_err` 関数 (削除対象)
- `crates/tokio-http2/src/webtransport.rs:186,203` — 文字列化経路 (統一対象)
- `crates/tokio-http2/tests/test_webtransport.rs:1187-1193` — 負値 WebTransport-Init テスト (書き換え対象)
- `src/webtransport/error.rs` — `WtError` 型定義 (`std::error::Error` 実装済み)
