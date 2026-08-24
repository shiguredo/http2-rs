# send_response / send_trailers が END_STREAM 送信時にキュー済み送信データを黙って破棄する

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-send-response-drop-queued-data
- Polished: {YYYY-MM-DD}

## 目的

`Connection::send_response` / `Connection::send_trailers` (`src/connection/headers.rs`) が END_STREAM 付き HEADERS を送信する際、送信バッファに滞留した未送信 DATA を確認せずにストリームを削除し、データを無通知で消失させる問題を修正する。

## 現状

`send_response` (`src/connection/headers.rs` の `Connection::send_response`) は、`end_stream` が true で状態機械が `StreamState::Closed` に遷移した場合、`send_buffer` の中身を確認せずに `streams.remove(&sid)` を実行する。`send_trailers` (`Connection::send_trailers`) も同様の構造である。

フロー制御で送信ウィンドウが枯渇している状態 (`send_buffer` にデータが滞留) で END_STREAM 付き HEADERS を送ると、滞留 DATA がピアに送信されずに破棄される。

対照的に `Connection::send_data` (`src/connection.rs`) は `pending_end_stream` が true のストリームへの追加 DATA 送信を拒否する検査 (`queue_data` 前のチェック) があり、送信側の整合性検査が非対称である。

## 設計方針

- `send_response` / `send_trailers` の END_STREAM 送信時に `send_buffer` が空であることを検査し、非空ならエラーを返す (滞留 DATA が残ったままの END_STREAM を拒否)
- もしくは、END_STREAM 送信前に滞留 DATA を先に送信する (フラッシュ) 設計に変更する
- 滞留データがある状態で END_STREAM 付き HEADERS を送信した場合のテストを追加する

## 完了条件

- 滞留 DATA がある状態で `send_response(end_stream=true)` / `send_trailers` を呼ぶとエラーが返ること (または滞留 DATA が先に送信されること)
- データが無通知で消失しないこと
- テストが追加され、`cargo test --all` が通過すること
