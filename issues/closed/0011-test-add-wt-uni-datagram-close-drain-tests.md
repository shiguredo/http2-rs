# tokio-http2 の WebTransport 統合テストに uni / datagram / close / drain を追加する

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

`crates/tokio-http2/tests/test_webtransport.rs` に、0007 で残件となっていた 4 ケースを追加する。

## 背景

0007 で `test_wt_bidi_echo` / `test_wt_reject` のみ実装した。実装本体 (0005 / 0006) は完了しているので、残りのテストを追加してカバレッジを上げる。

## 根拠

- 「お手本」と位置付けた `examples/wt_server` の動作保証には uni / datagram / close / drain の回帰を防ぐテストが必要
- `WtServerSession::close` / `drain` の公開 API が仕様通り `WT_CLOSE_SESSION` / `WT_DRAIN_SESSION` を送出することを固定する

## 対応内容

- `test_wt_uni_echo`: クライアント→サーバー単方向ストリームを accept し、`open_uni` で返す経路をエコー
- `test_wt_datagram_echo`: WT DATAGRAM capsule のラウンドトリップ
- `test_wt_close`: `WtServerSession::close(error_code, reason)` で `WT_CLOSE_SESSION` が送られ、クライアント側が `WtEvent::SessionClosed` を受信
- `test_wt_drain`: `WtServerSession::drain()` で `WT_DRAIN_SESSION` が送られ、クライアント側が `WtEvent::SessionDraining` を受信

## 完了条件

- `cargo test -p tokio-http2 --test test_webtransport` が 6 ケース pass
- `cargo fmt` / `cargo clippy -D warnings` が通る

## 依存

- 0003, 0004, 0005, 0006

## 解決方法

- `crates/tokio-http2/tests/test_webtransport.rs` に 4 ケース追加:
  - `test_wt_uni_echo`: client 起点 uni の到着 → server が `open_uni` で echo
  - `test_wt_datagram_echo`: `send_datagram` / `recv_datagram` のラウンドトリップ
  - `test_wt_close`: `WtServerSession::close(99, "shutdown")` → client が `SessionClosed` 受信
  - `test_wt_drain`: `WtServerSession::drain()` → client が `SessionDraining` 受信
- 重複コードを `connect_request` / `perform_connect` / `await_connect_headers` ヘルパーに切り出し
- `cargo test -p tokio-http2 --test test_webtransport` が 6 ケースすべて pass
