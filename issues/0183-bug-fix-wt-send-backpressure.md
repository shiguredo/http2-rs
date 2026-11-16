# 大容量 WT 送信と HTTP/2 送信ウィンドウ枯渇で driver が接続ごと終了する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-send-backpressure
- Polished: {YYYY-MM-DD}

## 目的

`tokio-http2` の WebTransport driver は、WT 出力を 65,535 バイトずつ `Connection::send_data` に渡すが、flush 中に WINDOW_UPDATE を読まない。WT 出力が HTTP/2 の connection / stream 送信ウィンドウと固定送信バッファを超えると `send buffer full` になり、driver が接続ごと終了する。既定設定で 200 KB の DATAGRAM を送るテストが、この接続終了を期待値として固定している。送信バックプレッシャーを正しく扱う。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `flush_wt_output` は `WtSession::poll_output` の全出力を 65,535 バイトずつ `send_data` する。`send_data` は送信ウィンドウが不足すると内部バッファに積むが、バッファ容量を超えるとエラーになる。`send_cmd_result` は flush エラーで `false` を返し、driver を終了する。

`crates/tokio-http2/tests/test_webtransport.rs` の `test_wt_command_flush_error_not_masked_as_connection_closed` は 200 KB DATAGRAM の送信で `send buffer full` を期待しており、この挙動を固定している。

## 設計方針

closed 0144 は 1 回の `send_data` が送信バッファ上限を超える問題をチャンク分割で修正したが、分割後のチャンクが送信ウィンドウ枯渇で累積する経路は未解決である。本 issue はその残存経路を対象とする。

- 未送信の WT 出力をキューに保持し、WINDOW_UPDATE 受信後に続きを送る。
- 接続を破棄せず、呼び出し側が再試行できるエラーを返す。
- `WtBidiStream::send` / `WtUniSendStream::send` / `send_datagram` の最大サイズと分割責任を rustdoc に明記する。
- 既定設定で 200 KB を送るテストを「接続終了」ではなく「バックプレッシャー後に送信完了」の期待値に変更する。
- ウィンドウ枯渇と回復の統合テストを追加する。

## 完了条件

- HTTP/2 の送信ウィンドウが不足しても driver が終了せず、WINDOW_UPDATE 後に送信が再開されること。
- 既定設定で 200 KB の DATAGRAM / ストリームデータを送れること、または API が事前に拒否すること。
- `send` の最大サイズと分割責任が rustdoc に明記されていること。
- バックプレッシャーの回帰テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
