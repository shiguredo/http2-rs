# SETTINGS_INITIAL_WINDOW_SIZE 増加時にキュー済み送信データがフラッシュされない

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-settings-window-flush
- Polished: 2026-09-09

## 目的

`Connection::handle_settings` (`src/connection.rs`) が SETTINGS_INITIAL_WINDOW_SIZE の増加を受信して送信ウィンドウを拡張した後、キューに滞留した送信データをフラッシュしないため、ピアが WINDOW_UPDATE ではなく SETTINGS でウィンドウを増やした場合にデータが無期限に滞留する (liveness 欠陥) 問題を修正する。

## 現状

`handle_settings` は `update_stream_windows` で既存ストリームの送信ウィンドウを増加させるが、その後 `flush_stream_data` / `flush_all_stream_data` を一切呼ばない。フラッシュのトリガーは以下のみ:

- `Connection::send_data` (`src/connection.rs`) — アプリが次のデータ送信を呼んだ場合
- `Connection::handle_window_update` — ピアが WINDOW_UPDATE を送った場合

ウィンドウ枯渇で `send_buffer` に滞留した DATA は、ピアが SETTINGS でウィンドウを増やしても、アプリが再び `send_data` を呼ぶかピアが WINDOW_UPDATE を送るまで送信されない。ピアによっては SETTINGS での拡張のみでデータ到着を待つため、回復不能な停滞が発生しうる。

ただし `flush_stream_data` の送信可否は接続レベルとストリームレベルの両ウィンドウの最小値で決まり、接続レベル送信ウィンドウは SETTINGS では変化しない (RFC 9113 Section 6.9.2)。本 issue が対象とするのは「ストリーム送信ウィンドウが枯渇し、接続レベル送信ウィンドウには空きがある」状態である。

## 設計方針

- `handle_settings` で `update_stream_windows` によりストリーム送信ウィンドウを拡張した後、SETTINGS ACK の送信 (RFC 9113 Section 6.5.3 の MUST) を済ませてから `flush_all_stream_data` を呼び、滞留 DATA を送信する。ACK より前に DATA を出すと ACK の即時送出が遅れるため、ACK の後に置く
- 接続レベルの送信ウィンドウは SETTINGS では変化せず (RFC 9113 Section 6.9.2)、接続ウィンドウが枯渇している場合は `flush_stream_data` が何も送らない。テストは、ピアが小さい `SETTINGS_INITIAL_WINDOW_SIZE` を広告してストリームウィンドウだけを枯渇させた後に増加させる、または接続レベル WINDOW_UPDATE で接続ウィンドウに余裕を持たせてから SETTINGS を送る構成にする
- `send_data` / `flush_stream_data` 周辺は 0134 (`send_response` / `send_trailers` の滞留 DATA 検査)・0137 (送信バッファ容量の分離)・0138 (`send_data` が未送信でも Ok を返す問題) と変更対象が近接する。本 issue は SETTINGS 受信時のフラッシュに閉じる
- SETTINGS_INITIAL_WINDOW_SIZE 増加時に滞留データが送信されることを検証するテストを追加する

## 完了条件

- ストリーム送信ウィンドウ枯渇で `send_buffer` に滞留した DATA が、接続レベル送信ウィンドウに空きがある状態で SETTINGS_INITIAL_WINDOW_SIZE が増加すると自動的に送信されること
- SETTINGS ACK が滞留 DATA より先に送信されること
- テストが追加され、`cargo test --all` が通過すること
