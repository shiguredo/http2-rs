# send_response / send_trailers が END_STREAM 送信時にキュー済み送信データを黙って破棄する

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-send-response-drop-queued-data
- Polished: 2026-09-09

## 目的

`Connection::send_response` / `Connection::send_trailers` (`src/connection/headers.rs`) が END_STREAM 付き HEADERS を送信する際、送信バッファに滞留した未送信 DATA を確認せずにストリームを削除し、データを無通知で消失させる問題を修正する。

## 現状

`send_response` (`src/connection/headers.rs` の `Connection::send_response`) は、`end_stream` が true で状態機械が `StreamState::Closed` に遷移した場合、`send_buffer` の中身を確認せずに `streams.remove(&sid)` を実行する。`send_trailers` (`Connection::send_trailers`) も同様の構造である。

フロー制御で送信ウィンドウが枯渇している状態 (`send_buffer` にデータが滞留) で END_STREAM 付き HEADERS を送ると、滞留 DATA がピアに送信されずに破棄される。

対照的に `Connection::send_data` (`src/connection.rs`) は `pending_end_stream` が true のストリームへの追加 DATA 送信を拒否する検査 (`queue_data` 前のチェック) があり、送信側の整合性検査が非対称である。

到達可能な経路は `send_trailers` である (`send_response(end_stream=false)` → `send_data(..., end_stream=false)` が送信ウィンドウ枯渇で `send_buffer` に滞留 → `send_trailers` という正しいメッセージ順序)。一方 `send_response(end_stream=true)` で滞留 DATA が存在するのは、最終レスポンス HEADERS より前に `send_data` を呼んだ場合だけであり、これは API 上は通るが RFC 9113 Section 8.1 のメッセージ順序 (HEADERS → DATA → trailers) に反する。`send_response` 側の検査は防御目的で残す。

## 設計方針

- `send_response` / `send_trailers` の END_STREAM 送信時に、`send_headers(end_stream)` を呼ぶ前に `send_buffer` が空であることを検査し、非空ならエラーを返す (滞留 DATA が残ったままの END_STREAM を拒否)。検査を状態遷移・HEADERS 送信の後に置くと、HEADERS 未送信のまま状態だけ `Closed` に進む不整合が生じる
- エラーは既存の `send_data` の `pending_end_stream` 検査と同じ `ErrorCode::StreamClosed` のストリームエラーとし、ストリームは削除せず滞留 DATA を保持する (WINDOW_UPDATE 受信時に `flush_stream_data` で送信される)
- 「END_STREAM 送信前に滞留 DATA をフラッシュする」案は採らない。`send_buffer` に滞留している時点で送信ウィンドウが枯渇しており、`flush_stream_data` は `available == 0` で何も送らずに `Ok` を返すため、フラッシュではデータを送れない
- `send_trailers` の再現手順と `send_response(end_stream=true)` の防御検査のテストを追加する
- 0135 (`handle_settings` 後の `flush_all_stream_data`)・0137 (送信バッファ容量の分離)・0138 (`send_data` が未送信でも Ok を返す問題) と `send_data` / `flush_stream_data` 周辺の変更対象が重なる。本 issue は `send_response` / `send_trailers` の検査に閉じ、`send_data` の Ok 返却仕様の変更は 0138 に委ねる

## 完了条件

- 送信ウィンドウ枯渇で `send_buffer` に滞留 DATA がある状態で `send_trailers` を呼ぶと、`ErrorCode::StreamClosed` のエラーが返り、END_STREAM 付き HEADERS が送信されず、ストリームと滞留 DATA が保持されること
- `send_response(end_stream=true)` でも同様に、滞留 DATA がある場合はエラーが返ること (防御検査)
- エラー後に送信ウィンドウが回復 (WINDOW_UPDATE 受信) すると滞留 DATA が送信されること
- テストが追加され、`cargo test --all` が通過すること
