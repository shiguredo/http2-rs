# 非 CONNECT ストリームの DATA / DataDiscarded / StreamReset で接続受信ウィンドウを補充する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-connection-window
- Polished: 2026-11-17

## 目的

`tokio-http2` の WebTransport driver は CONNECT ストリーム以外のイベントを無視する。HTTP/2 層は、他のオープンストリームの DATA では `Event::DataReceived` を、closed stream への遅延 DATA では `Event::DataDiscarded` を、DATA 違反で破棄した DATA では `Event::StreamReset` を発行し、いずれも接続受信ウィンドウを減算する (RFC 9113 Section 6.9: 受信者はフロー制御フレームを接続エラーとして扱う場合を除き、常に接続フロー制御ウィンドウに計上しなければならない MUST)。しかし driver はこれらのイベントに対して WINDOW_UPDATE を返さない。デフォルト設定 (接続ウィンドウ 65,535) では、ピアが 2 本目のストリームで 65,535 バイト送るだけで接続ウィンドウが尽き、WT セッションが止まる問題を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::handle_event` は、CONNECT ストリームの `Event::DataReceived` と、CONNECT ストリームの `Event::StreamReset` / `Event::StreamClosed` だけを処理し、それ以外は `_ => {}` で捨てる。`src/connection.rs` の `handle_data` は、closed stream への遅延 DATA を破棄するときに `Event::DataDiscarded { stream_id, connection_window_consumed }` を、DATA 違反でストリームをリセットするときに `Event::StreamReset { stream_id, error_code, connection_window_consumed }` を発行する。`Event::DataDiscarded` と `Event::StreamReset` の doc は「アプリはこの値ぶん `send_window_update` で補充する」としている。driver は非 CONNECT ストリームのこれらのイベントを無視するため、接続ウィンドウが回復しない。

## 設計方針

- 非 CONNECT ストリームの `Event::DataReceived` では、受信した `data.len()` に応じて connection WINDOW_UPDATE を送る。CONNECT ストリームの既存の WINDOW_UPDATE 送信は維持し、非 CONNECT ストリームの処理と二重送信しない。
- 非 CONNECT ストリームの `Event::DataDiscarded` では、`connection_window_consumed` ぶん connection WINDOW_UPDATE を送る。
- 非 CONNECT ストリームの `Event::StreamReset` でも、`connection_window_consumed` ぶん connection WINDOW_UPDATE を送る (DATA 違反で破棄された分の補充)。
- `connection_window_consumed` は `usize` なので `u32` への変換に `try_from` を使い、値域を確認する。0 の場合は WINDOW_UPDATE を送らない。
- 他ストリームの扱い (無視するか RST_STREAM / GOAWAY で拒否するか) は 0190 で決める。本 issue は、たとえ無視する方針でも、受信済み DATA の接続ウィンドウを補充することを保証する。拒否する方針でも、RST_STREAM / GOAWAY が届く前に受信した DATA は接続ウィンドウを消費するため、その分の補充は必要である。
- 2 本目のストリームで DATA を送る統合テストを追加する。2 本目のストリームで 65,535 バイト送った後も、CONNECT ストリームで WT データ (DATAGRAM など) を送受信できることを検証する。DATA 違反による `Event::StreamReset` と、その後同一ストリームへの遅延 DATA による `Event::DataDiscarded` の両方についても、接続ウィンドウが補充されることを検証する。
- 既知の制限: `Event::DataReceived` はパディング分のフロー制御消費量を運ばないため、パディング付き DATA では `data.len()` 基準の補充でパディング分 (1 + pad length + padding) が残留する。これは CONNECT ストリームの既存経路と同じ制限であり、本 issue の対象外とする。

## 完了条件

- 非 CONNECT ストリームの `Event::DataReceived` / `Event::DataDiscarded` / `Event::StreamReset` で接続受信ウィンドウが補充されること。
- 2 本目のストリームで 65,535 バイト送った後も、CONNECT ストリームで WT データを送受信できること (接続ウィンドウの回帰テスト)。
- 非 CONNECT ストリームの DATA 違反で `Event::StreamReset` が発行される場合も、接続ウィンドウが補充されること。
- パディングなしの DATA で接続受信ウィンドウが枯渇しないこと (パディング分の残留は既知の制限)。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。

## 参照

- `refs/rfc9113.txt` — Section 6.9 (Flow Control) / Section 6.9.1 (The Flow-Control Window) / Section 6.9.2 (Initial Flow-Control Window Size)
- `refs/draft-ietf-webtrans-http2-16.txt` — Section 3 (Session Establishment and Termination) / Section 4 (Flow Control) / Section 4.1 (Limiting the Number of Simultaneous Sessions)
- `crates/tokio-http2/src/webtransport.rs` — `DriverState::handle_event`
- `src/connection.rs` — `Connection::handle_data` / `Connection::reset_stream_internal`
- `src/event.rs` — `Event::DataReceived` / `Event::DataDiscarded` / `Event::StreamReset`
- `crates/tokio-http2/tests/test_webtransport.rs` — WebTransport サーバー統合テスト
- `issues/closed/0102-change-connection-window-exhaustion.md` — `Event::DataDiscarded` / `Event::StreamReset` の追加と、tokio-http2 側の対応を別 issue とした経緯
- `issues/0190-change-wt-multi-session.md` — 同一接続の他ストリームの扱い (単一セッション制約 / 複数セッション)
