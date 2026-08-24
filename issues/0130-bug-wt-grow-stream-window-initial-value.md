# tokio ドライバの自動ウィンドウ拡張がローカル開始 bidi ストリームに誤った初期値を使用する

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-grow-stream-window-initial-value
- Polished: {YYYY-MM-DD}

## 目的

`tokio-http2` の WebTransport ドライバが、ローカル開始の双方向ストリームに対して誤ったフロー制御初期値 (`initial_max_stream_data_bidi_remote`) を使用し、非対称設定で受信ウィンドウが拡張されずピアからのデータが永久に止まる問題を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::maybe_grow_stream_window` は、全 bidi ストリームに対して `WtConfig::initial_max_stream_data_bidi_remote` を「初期値」としてウィンドウ拡張のしきい値判定に使う。

しかし `src/webtransport.rs` の `WtSession::open_stream` は、ローカル開始 bidi ストリームの `recv_max` を `self.config.initial_max_stream_data_bidi_local` で初期化する (ピア開始 bidi は `initial_max_stream_data_bidi_remote`)。したがって、`initial_max_stream_data_bidi_local` と `initial_max_stream_data_bidi_remote` が異なる非対称設定 (例: remote=0, local>0) では、ローカル開始 bidi ストリームの `recv_available < initial/2` 判定が誤り、受信ウィンドウが二度と拡張されない。

デフォルト設定 (両方 262144) では顕在化しないため、既存テストでは検出されていない。

## 設計方針

- `maybe_grow_stream_window` がストリームの開始主体 (`WtStream::is_bidirectional` に加えてローカル開始かどうか) を判定し、ローカル開始 bidi には `initial_max_stream_data_bidi_local` を、ピア開始 bidi には `initial_max_stream_data_bidi_remote` を使用する
- 非対称設定でローカル開始 bidi の受信ウィンドウが拡張されることを検証するテストを追加する

## 完了条件

- ローカル開始 bidi ストリームの自動ウィンドウ拡張が `initial_max_stream_data_bidi_local` を基準に動作すること
- 非対称設定 (bidi_local と bidi_remote が異なる) でローカル開始 bidi の受信ウィンドウが拡張されるテストが追加されていること
- `cargo test --all` が通過すること
