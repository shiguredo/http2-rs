# tokio ドライバの自動ウィンドウ拡張がローカル開始 bidi ストリームに誤った初期値を使用する

- Created: 2026-08-24
- Completed: 2026-09-09
- Branch: feature/fix-wt-grow-stream-window-initial-value
- Polished: 2026-09-09

## 目的

`tokio-http2` の WebTransport ドライバが、ローカル開始の双方向ストリームに対して誤ったフロー制御初期値 (`initial_max_stream_data_bidi_remote`) を使用し、非対称設定で受信ウィンドウが拡張されずピアからのデータが永久に止まる問題を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` は、全 bidi ストリームに対して `WtConfig::initial_max_stream_data_bidi_remote` を「初期値」としてウィンドウ拡張のしきい値判定に使う。

しかし `src/webtransport.rs` の `WtSession::open_stream` は、ローカル開始 bidi ストリームの `recv_max` を `self.config.initial_max_stream_data_bidi_local` で初期化する (ピア開始 bidi の `recv_max` は `WtSession::handle_stream_data` が `self.config.initial_max_stream_data_bidi_remote` で初期化する)。したがって、`initial_max_stream_data_bidi_local` と `initial_max_stream_data_bidi_remote` が異なる非対称設定では、ローカル開始 bidi ストリームの `recv_available < initial/2` 判定と拡張量が誤る。特に `remote=0` かつ `local>0` の場合、`initial=0` により `recv_available < 0` が常に false となり、受信ウィンドウが一度も拡張されない (`remote>0` の場合は誤った値で拡張される)。

デフォルト設定 (両方 262144) では顕在化しないため、既存テストでは検出されていない。

## 設計方針

- `maybe_grow_stream_window` がストリームの開始主体を判定し、ローカル開始 bidi には `initial_max_stream_data_bidi_local` を、ピア開始 bidi には `initial_max_stream_data_bidi_remote` を使用する。`WtStream` に開始主体を返す公開 getter は存在しないため、`WtSession::role()` と `stream::stream_id::is_client_initiated` / `is_server_initiated` でストリーム ID から導出する (公開 API を増やさない。ドライバは `wt_stream_id` を既に import 済み)
- `initial_max_stream_data_bidi_remote = 0` かつ `initial_max_stream_data_bidi_local > 0` の非対称設定で、ローカル開始 bidi の受信ウィンドウが拡張されることを検証するテストを追加する。`remote > 0` では誤った初期値でも拡張自体は発生してしまうため、この条件で修正前は失敗するテストにする。`WtServerSession::accept` は `config.overlay_settings(conn.local_settings())` で接続 SETTINGS を `WtConfig` に上書きするため、テストでは `Limits` 側で `wt_initial_max_stream_data_bidi_remote = Some(0)` を広告するなどして remote=0 を実際に反映させる

## 完了条件

- ローカル開始 bidi ストリームの自動ウィンドウ拡張が `initial_max_stream_data_bidi_local` を基準に動作すること
- `initial_max_stream_data_bidi_remote = 0` かつ `initial_max_stream_data_bidi_local > 0` の非対称設定で、ローカル開始 bidi の受信ウィンドウが拡張されるテストが追加されていること
- テストは tokio-http2 のドライバ経路 (`crates/tokio-http2/tests/`) に置く。sans-io 層の `WtSession::grow_stream_recv_window` は正しいため、sans-io 層のテストでは本バグを検出できない
- `cargo test --all` が通過すること

## 解決方法

- `crates/tokio-http2/src/webtransport.rs` の `maybe_grow_stream_window` で、bidi ストリームの開始主体を `WtSession::role()` と `stream::stream_id::is_client_initiated` / `is_server_initiated` で判定し、ローカル開始 bidi には `initial_max_stream_data_bidi_local`、ピア開始 bidi には `initial_max_stream_data_bidi_remote` をしきい値・拡張量に使うようにした (draft-ietf-webtrans-http2-15 Section 11.2)。uni は従来どおり `initial_max_stream_data_uni`
- `crates/tokio-http2/tests/test_webtransport.rs` に、`bidi_remote=0` かつ `bidi_local>0` の非対称 Limits を広告するサーバーで、サーバー開始 bidi の受信ウィンドウが拡張されることを検証するテストを追加した (`asymmetric_server_limits` ヘルパー)
- `CHANGES.md` の `## develop` に `[FIX]` エントリを追加した
