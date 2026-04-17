# shiguredo_http2 の WebTransport 統合層を整備する

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

`shiguredo_http2` の SETTINGS / Connection / Stream / Event に、WebTransport over HTTP/2 (draft-ietf-webtrans-http2-14) のサーバー実装から利用可能な公開 API を整備する。

## 背景

現状、WebTransport 関連 SETTINGS は定数と `WtInitialSettings` 構造体として定義されているが、`Settings::apply()` は WT SETTINGS を無視する (unknown として扱う)。
また、Extended CONNECT で届く `:protocol` 擬似ヘッダーは `Stream` の内部フィールドに格納されているが公開されておらず、`Connection::remote_settings` も private である。

これらを公開/統合しないと、tokio-http2 の上位で `WtSession` を起動・接続することができない。

## 根拠

- draft-ietf-webtrans-http2-14 Section 11.2: SETTINGS_WT_INITIAL_MAX_* (0x2b61〜0x2b66) の授受は必須
- RFC 8441: `SETTINGS_ENABLE_CONNECT_PROTOCOL=1` を先に交換しないと Extended CONNECT が成立しない
- サーバー側で peer の WT SETTINGS を参照しないとフロー制御の初期値が決まらない

## 対応内容

### SETTINGS

- `src/settings.rs`:
  - `SettingId` enum に WT SETTINGS 6 項目を追加
  - `Settings` 構造体に `wt_initial: WtInitialSettings` を埋め込み
  - `Settings::apply()` で WT SETTINGS を `wt_initial` に振り分ける
  - `Settings::to_settings_list()` で `wt_initial` を出力に含める
  - 境界値バリデーション (u32 範囲等)

### Limits

- `src/limits.rs`:
  - `Limits::with_webtransport(WtConfig)` を追加
  - `Limits` に `wt_config: Option<WtConfig>` を保持し、初期 SETTINGS に反映

### Connection

- `src/connection/mod.rs`:
  - `Connection::remote_settings() -> &Settings` を public 化
  - `Connection::local_settings() -> &Settings` を public 化
  - `initiate()` で `wt_config` が設定されていれば WT SETTINGS を送信する

### Stream

- `src/stream/mod.rs`:
  - `Stream::has_protocol() -> bool` と `Stream::protocol() -> Option<&[u8]>` を public 化 (Extended CONNECT の `:protocol` 参照用)

### Event

- `src/event.rs`:
  - `Event::HeadersReceived` に `protocol: Option<Vec<u8>>` フィールドを追加
  - Connection 側の生成箇所を修正

### テスト

- PBT: `pbt/tests/prop_settings.rs` があれば WT SETTINGS ラウンドトリップを含める
- 単体テスト: `tests/test_settings.rs` に `apply_webtransport` 相当の境界値
- 既存テスト (`cargo test --workspace`) が全て通る

## 完了条件

- `cargo fmt` / `cargo clippy -D warnings` / `cargo test --workspace` が全て通る
- `Event::HeadersReceived.protocol` が Extended CONNECT 受信時に `Some(b"webtransport")` を返す
- `Connection::remote_settings().wt_initial` で peer の WT SETTINGS を取得できる
- `Limits::with_webtransport(WtConfig::default())` で初期 SETTINGS に WT SETTINGS が含まれる

## 破壊的変更

- `Event::HeadersReceived` にフィールド追加 (enum バリアントの非網羅拡張)
- 既存 `http2_server` / `http2_client` サンプルとテストを追随修正

## 依存

- なし (この issue が最初)

## 解決方法

- `src/settings.rs`
  - `SettingId` enum に WT SETTINGS 6 項目 (`0x2b61`〜`0x2b66`) を追加し、`from_u16` / `as_u16` を拡張
  - `Settings` に `wt_initial: WtInitialSettings` フィールドを埋め込み
  - `Settings::apply()` で WT SETTINGS を `wt_initial` に分配
  - `Settings::to_settings_list()` で `wt_initial` の Setting を出力に含める
  - 廃止予定だった `Settings::apply_webtransport()` は重複するため削除
- `src/limits.rs`
  - `Limits` に `wt_initial: WtInitialSettings` フィールドを追加し、`Limits::with_webtransport(WtInitialSettings)` ビルダーを提供
- `src/connection/mod.rs`
  - `Connection::new()` で `local_settings.wt_initial = limits.wt_initial.clone()` を反映
  - `Connection::local_settings()` / `remote_settings()` public アクセサを追加
  - サーバー受信時と クライアント送信時に `:protocol` 擬似ヘッダー値を `Stream::set_protocol` で保存
  - `Event::HeadersReceived` 生成時に `protocol: stream.protocol().map(|p| p.to_vec())` を付与
- `src/stream/mod.rs`
  - `Stream` に `protocol: Option<Vec<u8>>` フィールドを追加し、`protocol()` / `set_protocol()` を公開
- `src/event.rs`
  - `Event::HeadersReceived` に `protocol: Option<Vec<u8>>` フィールドを追加
- 追随修正
  - `pbt/tests/prop_event.rs`: `protocol` strategy を追加
  - `examples/http2_server/src/main.rs`, `examples/http2_client/src/main.rs`: pattern に `..` を追加
  - `crates/tokio-http2/tests/client_server.rs`, `crates/tokio-http2/tests/interop.rs`: `..` を一括追加 + `#![allow(clippy::collapsible_match)]` を追記
  - `crates/tokio-nghttp2/tests/client_server.rs`: 既存の潜在 clippy 警告を抑える `#![allow(clippy::collapsible_match)]` を追記
  - `README.md`, `crates/tokio-http2/README.md`: サンプルコードを `..` に追随
- 検証
  - `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` がすべて green
