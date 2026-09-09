# 自動ウィンドウ拡張のしきい値 `initial / 2` が initial=1 で 0 になりウィンドウが拡張されない

- Created: 2026-09-09
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-grow-window-threshold-initial-one
- Polished: 2026-09-10

## 目的

tokio-http2 の WebTransport ドライバの自動ウィンドウ拡張が、`initial_max_stream_data_*` に 1 を設定した場合に一度も拡張されず、ピアの送信が 1 バイトで止まる問題を修正する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `maybe_grow_stream_window` は `recv_available < initial / 2` を条件に `grow_stream_recv_window` を呼ぶ。`initial` が 1 のとき `initial / 2` は切り捨てで 0 になり、`recv_available < 0` が常に false となるため、受信ウィンドウが一度も拡張されない。1 バイト受信した時点で `recv_available` が 0 になり、以降ピアは送信できない。

同じ切り捨て除算は `WtFlowControl::should_send_max_data` (セッションレベル) にもある。

## 設計方針

- しきい値を `initial.div_ceil(2)` に変更し、`initial == 1` でも拡張されるようにする (`initial == 0` は 0 のまま拡張しない)
- セッションレベルの `WtFlowControl::should_send_max_data` も同様に修正する
- `initial_max_stream_data_*` / `initial_max_data` に 1 を設定した場合に拡張されることを検証するテストを追加する

## 完了条件

- `initial_max_stream_data_bidi_local = 1` 等の設定で受信ウィンドウが拡張されること
- `initial == 0` では従来どおり拡張されないこと
- テストが追加され、`cargo test --all` が通過すること
