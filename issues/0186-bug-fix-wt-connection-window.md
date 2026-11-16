# 非 CONNECT ストリームの DATA / DataDiscarded で接続受信ウィンドウを補充する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-connection-window
- Polished: {YYYY-MM-DD}

## 目的

`tokio-http2` の WebTransport driver は CONNECT 以外のイベントを無視する。HTTP/2 層は他のオープンストリームや closed stream の DATA でも接続受信ウィンドウを減算し、`Event::DataDiscarded` を発行するが、driver は WINDOW_UPDATE を返さない。ピアが 2 本目のストリームで 65,535 バイト送るだけで接続ウィンドウが尽き、WT セッションが止まる問題を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::handle_event` は CONNECT ストリームの `Event::DataReceived` だけを処理し、それ以外は `_ => {}` で捨てる。`src/connection.rs` の `handle_data` は、closed stream への遅延 DATA を破棄するときに `Event::DataDiscarded { stream_id, connection_window_consumed }` を発行する。`Event::DataDiscarded` の doc は「アプリはこの値ぶん `send_window_update` で補充する」としている。

## 設計方針

- 非 CONNECT ストリームの `Event::DataReceived` でも、受信バイト数に応じて connection WINDOW_UPDATE を送る。
- `Event::DataDiscarded` の `connection_window_consumed` ぶん connection WINDOW_UPDATE を送る。
- 単一セッション接続を強制する方針なら、他ストリームを RST_STREAM / GOAWAY で明示拒否し、接続ウィンドウを消費させない。
- 2 本目のストリームで DATA を送る統合テストを追加する。

## 完了条件

- 非 CONNECT ストリームの DATA / DataDiscarded で接続受信ウィンドウが枯渇しないこと。
- 他ストリームを拒否する方針の場合、その動作が doc とテストで固定されていること。
- 接続ウィンドウの回帰テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
