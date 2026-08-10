# 同時ストリーム数上限超過 (REFUSED_STREAM) が接続終了を引き起こす問題を修正する

- Created: 2026-08-10
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-refused-stream-concurrent-limit
- Polished: {YYYY-MM-DD}

## 目的

`Connection::handle_headers` が同時ストリーム数上限超過を検出したときに返す `Error::stream_error(ErrorCode::RefusedStream)` が `process()` から呼び出し側へそのまま伝播し、RST_STREAM が送信されず接続が終了する。RFC 9113 Section 5.1.2 は受信した HEADERS が広告した同時ストリーム数上限を超える場合に PROTOCOL_ERROR または REFUSED_STREAM のストリームエラーで応答することを MUST と定めており、ストリームエラーは RST_STREAM で処理して接続を維持するべきである (RFC 9113 Section 5.4.2)。0106 で確立した「ストリームエラーは RST_STREAM 送信 + 接続維持」の方針と整合させる。

## 現状

`src/connection/headers.rs` の `handle_headers` は、新規ストリーム (ストリーム未作成) の HEADERS 受信時に `check_concurrent_streams_limit(sid)` を呼ぶ。上限超過時は `Err(Error::stream_error(ErrorCode::RefusedStream, "max concurrent streams exceeded"))` を返す (`src/connection.rs` の `check_concurrent_streams_limit`)。この `?` は `handle_frame` → `process()` を経由して呼び出し側へそのまま伝播するため:

- RST_STREAM が送信されない
- ストリームエラーが `process()` から `Err` として返り、呼び出し側が接続を終了する
- `check_concurrent_streams_limit` は `last_recv_stream_id` 更新より前に呼ばれるため、ストリーム未生成のまま `reset_stream_internal` を呼ぶと `is_idle_stream` 判定が発火しうる (0106 と同じ理由で、ストリームを生成してからリセットする必要がある)

これは 0106 の残課題として記録済みの経路であり、`handle_headers` の CONNECT 確立済みストリームへの HEADERS 拒否経路と同種の未変換経路である。

## 設計方針

0106 の `reset_headers_validation_error` と同じパターンで、ストリームを生成してから `reset_stream_internal` でリセットする:

- `check_concurrent_streams_limit` の `Err` を捕捉し、ストリームを生成したうえで `reset_stream_internal` を REFUSED_STREAM で呼ぶ (RST_STREAM (REFUSED_STREAM) 送信・`Event::StreamReset` (connection_window_consumed: 0) 生成・`closed_streams` 登録・`streams` 削除を一貫させる)
- ストリームを生成してからリセットすることで、`is_idle_stream` 判定 (`src/connection.rs` は最初に `streams` への存在を判定する) が発火しないことを保証する
- 変換後は `process_headers` と同じく `Ok` を返し、`last_successful_stream_id` の更新対象に含める (RST_STREAM 送信は RFC 9113 Section 6.8 の last-stream-id 更新対象)
- エラーコードは RFC 9113 Section 5.1.2 の MUST に従い REFUSED_STREAM とする

## 完了条件

- 同時ストリーム数上限超過時の新規 HEADERS で、RST_STREAM (REFUSED_STREAM) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成され、ストリームが `streams` から削除される
- ストリームエラーが `process()` から呼び出し側へ伝播せず、接続が維持される
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `refs/rfc9113.txt` — Section 5.1.2 (Stream Concurrency) / Section 5.4.2 (Stream Error Handling) / Section 6.4 (RST_STREAM) / Section 6.8 (GOAWAY)
- `src/connection/headers.rs` — `Connection::handle_headers`
- `src/connection.rs` — `Connection::check_concurrent_streams_limit` / `Connection::reset_stream_internal` / `Connection::is_idle_stream`
- `issues/closed/0106-bug-fix-header-error-paths.md` — 同種のエラー経路変換を実施した先行対応。残課題として本経路を記録
