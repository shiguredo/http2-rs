# CONNECT 確立済みストリームへの HEADERS ・未知フレームが接続終了を引き起こす問題を修正する

- Created: 2026-08-15
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-connect-established-frame-errors
- Polished: {YYYY-MM-DD}

## 目的

CONNECT 確立済みストリームへの HEADERS 拒否経路 (`src/connection/headers.rs` の `handle_headers`) と未知フレーム処理経路 (`src/connection.rs` の `handle_frame` の `Frame::Unknown` 処理) が返す `Error::stream_error(ErrorCode::ProtocolError)` が `process()` から呼び出し側へそのまま伝播し、RST_STREAM が送信されず接続が終了する。RFC 9113 Section 8.5 は CONNECT 確立済みストリームへの DATA 以外のフレーム (RST_STREAM / WINDOW_UPDATE / PRIORITY を除く) をストリームエラーとして処理することを MUST と定めており、ストリームエラーは RST_STREAM で処理して接続を維持するべきである (RFC 9113 Section 5.4.2)。0106 / 0108 で確立した「ストリームエラーは RST_STREAM 送信 + 接続維持」の方針と整合させる。

## 現状

0106 の解決方法に残課題として記録された 2 経路が対象。

- `src/connection/headers.rs` の `handle_headers`: ストリームが `streams` に存在し `connect_established()` が真 (CONNECT トンネル確立済み) の場合に HEADERS を `Err(Error::stream_error(ErrorCode::ProtocolError, ...))` で拒否する。この `?` は `handle_frame` → `process()` を経由して呼び出し側へ伝播するため、RST_STREAM が送信されず接続が終了する
- `src/connection.rs` の `handle_frame` の `Frame::Unknown` 処理: `header.stream_id != 0` かつストリームが `streams` に存在し `connect_established()` が真の場合に `Err(Error::stream_error(ErrorCode::ProtocolError, ...))` を返す。同じく接続が終了する

いずれも RFC 9113 Section 8.5 の「Frame types other than DATA or stream management frames (RST_STREAM, WINDOW_UPDATE, and PRIORITY) MUST NOT be sent on a connected stream and MUST be treated as a stream error (Section 5.4.2) if received」に該当する経路である。

## 設計方針

0106 / 0108 と同じ `reset_stream_internal` パターンで、両経路を RST_STREAM 送信 + `Event::StreamReset` (connection_window_consumed: 0) 生成 + `closed_streams` 登録 + `streams` 削除に変換し、接続を維持する。

- HEADERS 経路は field block のデコード前に検出される。RFC 9113 Section 4.3 は破棄する場合でも field block の再組み立てと伸長を要求し、伸長しない場合は COMPRESSION_ERROR の接続エラーで終了しなければならない (MUST) ため、デコード完了後にリセットする (0108 の `header_concurrent_limit_exceeded` と同じ遅延機構を踏襲する)
- HEADERS 経路はストリームが `streams` に存在する (CONNECT 確立済み) ため、ストリーム生成は不要。デコード後に `reset_stream_internal` (PROTOCOL_ERROR) を直接呼ぶ
- 未知フレーム経路はデコード済みのフレームであり HPACK 状態への影響がないため、検出時点で `reset_stream_internal` (PROTOCOL_ERROR) を直接呼ぶ
- エラーコードは RFC 9113 Section 8.5 の MUST 違反 (許可されないフレームの受信) として PROTOCOL_ERROR を選択する
- 変換後は `Ok` を返す。HEADERS 経路は `last_successful_stream_id` の更新対象に含める (0108 と同じ根拠: RFC 9113 Section 6.8 は RST_STREAM 送信も last-stream-id 更新対象)

## 完了条件

- CONNECT 確立済みストリームへの HEADERS で、RST_STREAM (PROTOCOL_ERROR) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成され、ストリームが `streams` から削除され、接続が維持される
- CONNECT 確立済みストリームへの未知フレームでも同様に RST_STREAM (PROTOCOL_ERROR) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成され、ストリームが `streams` から削除され、接続が維持される
- ストリームエラーが `process()` から呼び出し側へ伝播しない
- HEADERS 経路で field block がデコードされ、HPACK 状態が維持される (リセット後に別の新規 HEADERS が正常に処理でき、COMPRESSION_ERROR にならないこと)
- 変換対象の経路で `Event::HeadersReceived` / `Event::TrailersReceived` / `Event::StreamClosed` が生成されない
- リセット後の遅延 DATA が `Event::DataDiscarded` で破棄され、接続が維持される
- 上記を検証する単体テストを `tests/test_connection.rs` の `mod reset_stream` に追加し、`CHANGES.md` の `## develop` に `[FIX]` エントリ (shiguredo-changelog スキルに従う) を追加し、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `refs/rfc9113.txt` — Section 4.3 (Header Compression and Decompression) / Section 5.4.2 (Stream Error Handling) / Section 6.8 (GOAWAY) / Section 8.5 (The CONNECT Method)
- `src/connection/headers.rs` — `Connection::handle_headers`
- `src/connection.rs` — `Connection::handle_frame` / `Connection::reset_stream_internal`
- `issues/closed/0106-bug-fix-header-error-paths.md` — 残課題として本経路を記録した先行対応
- `issues/closed/0108-bug-fix-refused-stream-concurrent-limit.md` — デコード前検出のストリームエラーをデコード後にリセットする遅延機構を確立した先行対応
