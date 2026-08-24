# SETTINGS_INITIAL_WINDOW_SIZE 増加時にキュー済み送信データがフラッシュされない

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-settings-window-flush
- Polished: {YYYY-MM-DD}

## 目的

`Connection::handle_settings` (`src/connection.rs`) が SETTINGS_INITIAL_WINDOW_SIZE の増加を受信して送信ウィンドウを拡張した後、キューに滞留した送信データをフラッシュしないため、ピアが WINDOW_UPDATE ではなく SETTINGS でウィンドウを増やした場合にデータが無期限に滞留する (liveness 欠陥) 問題を修正する。

## 現状

`handle_settings` は `update_stream_windows` で既存ストリームの送信ウィンドウを増加させるが、その後 `flush_stream_data` / `flush_all_stream_data` を一切呼ばない。フラッシュのトリガーは以下のみ:

- `Connection::send_data` (`src/connection.rs`) — アプリが次のデータ送信を呼んだ場合
- `Connection::handle_window_update` — ピアが WINDOW_UPDATE を送った場合

ウィンドウ枯渇で `send_buffer` に滞留した DATA は、ピアが SETTINGS でウィンドウを増やしても、アプリが再び `send_data` を呼ぶかピアが WINDOW_UPDATE を送るまで送信されない。ピアによっては SETTINGS での拡張のみでデータ到着を待つため、回復不能な停滞が発生しうる。

## 設計方針

- `handle_settings` の `update_stream_windows` 呼び出し後に `flush_all_stream_data` を呼び、滞留データを送信する
- SETTINGS_INITIAL_WINDOW_SIZE 増加時に滞留データが送信されることを検証するテストを追加する

## 完了条件

- SETTINGS_INITIAL_WINDOW_SIZE 増加時にキュー済みの送信データが自動的に送信されること
- テストが追加され、`cargo test --all` が通過すること
