# CONNECT 確立済みストリームへの HEADERS ・未知フレームが接続終了を引き起こす問題を修正する

- Created: 2026-08-15
- Completed: 2026-08-15
- Branch: feature/fix-connect-established-frame-errors
- Polished: 2026-08-15

## 目的

CONNECT 確立済みストリームへの HEADERS 拒否経路 (`src/connection/headers.rs` の `handle_headers`) と未知フレーム処理経路 (`src/connection.rs` の `handle_frame` の `Frame::Unknown` 処理) が返す `Error::stream_error(ErrorCode::ProtocolError)` が `process()` から呼び出し側へそのまま伝播し、RST_STREAM が送信されず接続が終了する。RFC 9113 Section 8.5 は CONNECT 確立済みストリームへの DATA または stream management フレーム (RST_STREAM / WINDOW_UPDATE / PRIORITY) 以外のフレームをストリームエラーとして処理することを MUST と定めており、ストリームエラーは RST_STREAM で処理して接続を維持するべきである (RFC 9113 Section 5.4.2)。0106 / 0108 で確立した「ストリームエラーは RST_STREAM 送信 + 接続維持」の方針と整合させる。

## 現状

0106 の残課題として記録された 3 経路 (CONNECT 確立済みストリームへの HEADERS 拒否 / 同時ストリーム数上限超過 / 未知フレーム) のうち、0108 が同時ストリーム数上限超過経路を対応済みのため、本 issue は残る 2 経路が対象。

- `src/connection/headers.rs` の `handle_headers`: ストリームが `streams` に存在し `connect_established()` が真 (CONNECT トンネル確立済み) の場合に HEADERS を `Err(Error::stream_error(ErrorCode::ProtocolError, ...))` で拒否する。この `?` は `handle_frame` → `process()` を経由して呼び出し側へ伝播するため、RST_STREAM が送信されず接続が終了する
- `src/connection.rs` の `handle_frame` の `Frame::Unknown` 処理: `header.stream_id != 0` かつストリームが `streams` に存在し `connect_established()` が真の場合に `Err(Error::stream_error(ErrorCode::ProtocolError, ...))` を返す。同じく接続が終了する

いずれも RFC 9113 Section 8.5 の「Frame types other than DATA or stream management frames (RST_STREAM, WINDOW_UPDATE, and PRIORITY) MUST NOT be sent on a connected stream and MUST be treated as a stream error (Section 5.4.2) if received」に該当する経路である。

## 設計方針

0106 / 0108 と同じ `reset_stream_internal` パターンで、両経路を RST_STREAM 送信 + `Event::StreamReset` (connection_window_consumed: 0) 生成 + `closed_streams` 登録 + `streams` 削除に変換し、接続を維持する。

- HEADERS 経路は field block のデコード前に検出される。RFC 9113 Section 4.3 は破棄する場合でも field block の再組み立てと伸長を要求し、伸長しない場合は COMPRESSION_ERROR の接続エラーで終了しなければならない (MUST) ため、デコード完了後にリセットする (0108 の `header_concurrent_limit_exceeded` と同じ遅延機構を踏襲する)
- HEADERS 経路は単一フレーム・多フレーム (HEADERS + CONTINUATION) のいずれでもデコード完了後にリセットする。多フレームの場合は `handle_continuation` 側で記録した状態を消費してリセットへ分岐する (0108 と同じ機構)
- HEADERS 経路はストリームが `streams` に存在する (CONNECT 確立済み) ため、ストリーム生成は不要。デコード後に `reset_stream_internal` (PROTOCOL_ERROR) を直接呼ぶ
- 未知フレーム経路はデコード済みのフレームであり HPACK 状態への影響がないため、検出時点で `reset_stream_internal` (PROTOCOL_ERROR) を直接呼ぶ
- 未知フレームの分類は RFC 9113 Section 4.1 (未知フレームは無視して破棄する MUST) と Section 8.5 (CONNECT 確立済みストリームでのフレーム制限 MUST) が競合するが、既存実装の Section 8.5 優先の判断を維持する (本 issue は伝播の修正のみを対象とする)
- エラーコードは RFC 9113 Section 8.5 の MUST 違反 (許可されないフレームの受信) として PROTOCOL_ERROR を選択する
- 変換後は `Ok` を返す。HEADERS 経路はリセット分岐側で `last_successful_stream_id` を更新する (0108 と同じ根拠: RFC 9113 Section 6.8 は RST_STREAM 送信も last-stream-id 更新対象。リセット分岐は `process_headers` と通常の更新部をスキップするため、`reset_refused_concurrent_stream` と同じくリセットヘルパー内で更新する)。未知フレーム経路は `handle_frame` 内の非ヘッダー経路であり、`last_successful_stream_id` は更新しない (既存の DATA 経路 (`handle_data`) のストリームエラーと同じ扱い。更新箇所はヘッダー処理のみという既存方針を維持)

## 完了条件

- CONNECT 確立済みストリームへの HEADERS (単一フレーム・多フレーム (HEADERS + CONTINUATION) のいずれでも) で、RST_STREAM (PROTOCOL_ERROR) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成され、ストリームが `streams` から削除され、接続が維持される
- CONNECT 確立済みストリームへの未知フレームでも同様に RST_STREAM (PROTOCOL_ERROR) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成され、ストリームが `streams` から削除され、接続が維持される
- ストリームエラーが `process()` から呼び出し側へ伝播しない
- HEADERS 経路で field block がデコードされ、HPACK 状態が維持される (リセット後に別の新規 HEADERS が正常に処理でき、COMPRESSION_ERROR にならないこと)
- 変換対象の経路で `Event::HeadersReceived` / `Event::TrailersReceived` / `Event::StreamClosed` が生成されない
- HEADERS 経路でリセットしたストリームが GOAWAY の last-stream-id (`last_successful_stream_id`) に反映され、未知フレーム経路でリセットしたストリームは反映されない
- リセット後の遅延 DATA が `Event::DataDiscarded` で破棄され、接続が維持される
- 上記を検証する単体テストを `tests/test_connection.rs` の `mod reset_stream` (0111 の分割実施後は分割先の該当サブモジュール) に追加し、`CHANGES.md` の `## develop` に `[FIX]` エントリ (shiguredo-changelog スキルに従う) を追加し、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `refs/rfc9113.txt` — Section 4.3 (Field Section Compression and Decompression) / Section 5.4.2 (Stream Error Handling) / Section 6.8 (GOAWAY) / Section 8.5 (The CONNECT Method)
- `src/connection/headers.rs` — `Connection::handle_headers`
- `src/connection.rs` — `Connection::handle_frame` / `Connection::reset_stream_internal`
- `issues/closed/0106-bug-fix-header-error-paths.md` — 残課題として本経路を記録した先行対応
- `issues/closed/0108-bug-fix-refused-stream-concurrent-limit.md` — デコード前検出のストリームエラーをデコード後にリセットする遅延機構を確立した先行対応

## 解決方法

1. `src/connection/headers.rs` の `handle_headers` の CONNECT 確立済みストリームへの HEADERS 拒否経路を、`Err(stream_error)` の返却から `connect_established_headers_received` フラグの記録に変換し、field block のデコード完了後にヘルパー `reset_connect_established_headers` で RST_STREAM (PROTOCOL_ERROR) 送信・`Event::StreamReset` (connection_window_consumed: 0) 生成・`streams` 削除・接続維持に変換した (RFC 9113 Section 4.3 の伸長 MUST に従う遅延リセット。同時ストリーム数上限超過と同じ機構)
2. 多フレーム (HEADERS + CONTINUATION) の場合は `handle_continuation` がフラグを消費してデコード完了後にリセットする。`is_previously_closed` / `is_stream_closed` の閉塞分岐ではフラグの残留を防ぐため防御的にフラグをクリアする (現状到達不能)
3. `src/connection.rs` の `handle_frame` の `Frame::Unknown` 処理を、`Err(stream_error)` の返却から検出時点での `reset_stream_internal` (PROTOCOL_ERROR) 呼び出しに変換した (未知フレームはデコード済みで HPACK 状態に影響しないため遅延不要)
4. `last_successful_stream_id` の更新は両経路とも行わない。設計方針では HEADERS 経路を更新対象に含めるとしていたが、レビューで「CONNECT 確立済みストリームは CONNECT リクエスト処理で既に `last_successful_stream_id` に記録済みのため、リセット時の更新は常に no-op である」ことが判明したため、更新と GOAWAY 恒真テストを削除した (完了条件の「GOAWAY の last-stream-id に反映され」は CONNECT リクエスト処理で既に満たされる)
5. `tests/test_connection.rs` の `mod reset_stream` に単体テスト 5 件を追加した (単一フレーム HEADERS / CONTINUATION 分割 / 未知フレーム / HPACK 状態維持 単一・多フレーム)
6. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加した (shiguredo-changelog スキルに従う)
7. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ることを確認した
