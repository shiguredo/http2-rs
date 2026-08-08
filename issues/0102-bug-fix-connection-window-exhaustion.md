# ストリームエラーで破棄された DATA の接続ウィンドウ消費をアプリが補充できず枯渇する問題を修正する

- Priority: Medium
- Created: 2026-08-08
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-connection-window-exhaustion
- Polished: {YYYY-MM-DD}

## 目的

`Connection::handle_data` は、ストリームエラーで破棄する DATA も接続フロー制御ウィンドウに計上する (RFC 9113 Section 6.9 の MUST)。しかし破棄経路では `Event::DataReceived` が生成されず、`Event::StreamReset` には消費バイト数が含まれないため、アプリは何バイト補充すべきかを原理的に知ることができない。悪意のあるピアが Content-Length 超過の DATA を新ストリーム ID で繰り返し送ると、接続ウィンドウ (デフォルト 65535) が枯渇し、正当なストリームの DATA 受信が FLOW_CONTROL_ERROR の接続エラーで遮断される。

本修正でストリームエラーが RST_STREAM 処理に変換され接続が維持されるようになったことで、この経路が実用的な攻撃面として顕在化した。繰り返し違反に対する接続維持が成立するよう、破棄データのウィンドウ補充手段を提供する。

## 現状

- `Connection::handle_data` はフロー制御ウィンドウの計上を `self.flow_control.consume_recv(flow_control_size)` (`src/connection.rs`) で全経路に先立って行う (RFC 9113 Section 6.9 の MUST)。エラー経路 (状態遷移違反 / no-content 違反 / Content-Length 超過 / END_STREAM 時不一致) とクローズ済みストリームへの遅延 DATA 破棄経路では、計上されたウィンドウが永久に回復しない
- `FlowControl::should_send_window_update` / `FlowControl::window_update_increment` (`src/flow_control.rs`) は存在するが、`src/connection.rs` から一切呼ばれていない
- `Event::StreamReset` (`src/event.rs`) は `stream_id` と `error_code` のみで、消費バイト数を持たない
- `crates/tokio-http2` 層では WebTransport セッション (`crates/tokio-http2/src/webtransport.rs` の `handle_event`) のみ `Event::DataReceived` の `data.len()` に応じて WINDOW_UPDATE を送信する。それ以外の通常 HTTP/2 サーバー・クライアントはアプリが `send_window_update` を手動呼び出しする設計
- 既存のクローズ済みストリームへの遅延 DATA 破棄経路にも同種の穴はあったが、ストリームエラーが接続終了を引き起こしていたため実用的な攻撃経路ではなかった

## 設計方針

破棄・リセット経路で消費された接続ウィンドウを自動回復するか、アプリが補充量を知れるようにする。以下の 2 案を検討する:

- 案 A: `Event::StreamReset` に消費バイト数 (接続ウィンドウ計上分) のフィールドを追加する。アプリは受信パス (`handle_rst_stream` 由来) と送信パス (`reset_stream` 由来) の両方で通知されたバイト数ぶん `send_window_update(StreamId::Connection, ...)` を送る
- 案 B: ライブラリ内部で破棄・リセット経路の接続ウィンドウ計上分を自動補充し、WINDOW_UPDATE を自動送信する。アプリの変更が不要になるが、Sans I/O 層の責務 (イベント生成のみ) と出力制御 (WINDOW_UPDATE 送信はアプリの判断) の設計に変更が及ぶ

## 完了条件

- ストリームエラーまたは遅延 DATA 破棄で消費された接続ウィンドウのバイト数をアプリが認識できる (または自動回復される) こと
- 違反 DATA を繰り返し送信しても接続ウィンドウが枯渇せず、正当なストリームの DATA 受信が継続できること
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ること

## 解決方法

1. 設計方針の案 A / 案 B のいずれかを採用して実装する
2. 破棄・リセット経路の接続ウィンドウ消費が補充されることを検証する単体テストを追加する (例: Content-Length 超過の DATA を繰り返し送信しても接続ウィンドウが枯渇せず、`Event::WindowUpdateReceived` が通常どおり処理されること)
3. `CHANGES.md` の `## develop` にエントリを追加する (shiguredo-changelog スキルを参照)

## 参照

- `refs/rfc9113.txt` — Section 6.9 (Flow Control) / Section 5.4.1 (Connection Errors) / Section 5.4.2 (Stream Errors)
- `src/connection.rs` — `Connection::handle_data` / `Connection::reset_stream`
- `src/flow_control.rs` — `FlowControl::consume_recv` / `FlowControl::should_send_window_update` / `FlowControl::window_update_increment`
- `src/event.rs` — `Event::StreamReset` (バイト数フィールドなし)
- `crates/tokio-http2/src/webtransport.rs` — `Event::DataReceived` に応じた WINDOW_UPDATE 送信の先例
