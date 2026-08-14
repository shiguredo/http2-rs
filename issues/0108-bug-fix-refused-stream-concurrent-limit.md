# 同時ストリーム数上限超過 (REFUSED_STREAM) が接続終了を引き起こす問題を修正する

- Created: 2026-08-10
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-refused-stream-concurrent-limit
- Polished: 2026-08-14

## 目的

`Connection::handle_headers` が同時ストリーム数上限超過を検出したときに返す `Error::stream_error(ErrorCode::RefusedStream)` が `process()` から呼び出し側へそのまま伝播し、RST_STREAM が送信されず接続が終了する。RFC 9113 Section 5.1.2 は受信した HEADERS が広告した同時ストリーム数上限を超える場合に PROTOCOL_ERROR または REFUSED_STREAM のストリームエラーで応答することを MUST と定めており、ストリームエラーは RST_STREAM で処理して接続を維持するべきである (RFC 9113 Section 5.4.2)。0106 で確立した「ストリームエラーは RST_STREAM 送信 + 接続維持」の方針と整合させる。

## 現状

`src/connection/headers.rs` の `handle_headers` は、新規ストリーム (ストリーム未作成) の HEADERS 受信時に `check_concurrent_streams_limit(sid)` を呼ぶ。上限超過時は `Err(Error::stream_error(ErrorCode::RefusedStream, "max concurrent streams exceeded"))` を返す (`src/connection.rs` の `check_concurrent_streams_limit`)。この `?` は `handle_frame` → `process()` を経由して呼び出し側へそのまま伝播するため:

- RST_STREAM が送信されない
- ストリームエラーが `process()` から `Err` として返り、呼び出し側が接続を終了する
- `check_concurrent_streams_limit` は `last_recv_stream_id` 更新より前に呼ばれるため、ストリーム未生成のまま `reset_stream_internal` を呼ぶと `is_idle_stream` 判定が発火しうる (0106 と同じ理由で、ストリームを生成してからリセットする必要がある)

これは 0106 の残課題として記録済みの経路であり、`handle_headers` の CONNECT 確立済みストリームへの HEADERS 拒否経路や `handle_frame` の未知フレーム処理と同種の未変換経路である。本 issue は同時ストリーム数上限超過の経路のみを対象とし、他の 2 経路は対象外とする (0106 の残課題として引き続き残す)。

## 設計方針

0106 の `reset_headers_validation_error` と同じパターンで、ストリームを生成してから `reset_stream_internal` でリセットする。ただし 0106 の各経路は HPACK デコード後に検出されたのに対し、本経路の `check_concurrent_streams_limit` は HPACK デコードより前に呼ばれる。したがってリセットは field block のデコード・吸収を済ませてから行う (順序を守らないと RFC 4.3 MUST 違反になる)。順序を明示すると:

1. **HPACK 状態同期 (RFC 9113 Section 4.3 MUST)**: 受信した HEADERS / CONTINUATION の field block は、破棄する場合でも再組み立てして伸長しなければならない。伸長しないままリセットして接続を維持すると、デコーダとピアのエンコーダ文脈がずれて以降の field block で COMPRESSION_ERROR の接続エラーになる。したがって上限超過時も field block のデコードは必ず行う
2. **多フレーム field block (HEADERS + CONTINUATION) への対応**: `frame.end_headers == false` の場合、後続の CONTINUATION を `header_continuation_stream` で受け止めて field block を完成させてからリセットする。CONTINUATION を含む field block も 1 の MUST に従い破棄する場合でも伸長し、RST_STREAM 送信後もフライト中のフレームを受信処理する準備が必要である (RFC 9113 Section 6.4)。`Event::HeadersReceived` / `Event::TrailersReceived` は生成しない
3. **リセットの実行**: `check_concurrent_streams_limit` の `Err` を捕捉し、ストリームを生成したうえで `reset_stream_internal` を REFUSED_STREAM で呼ぶ (RST_STREAM (REFUSED_STREAM) 送信・`Event::StreamReset` (connection_window_consumed: 0) 生成・`closed_streams` 登録・`streams` 削除を一貫させる)。上限超過の検出はデコード前なので、`handle_headers` 内で上限超過を記録しておき、単一フレームは `handle_headers` のデコード後、多フレームは `handle_continuation` のデコード後に `process_headers` へ進まずリセットへ分岐する (記録した状態をデコード後に消費する機構を設ける)。リセット分岐は `process_headers` をスキップするため、既存の `last_successful_stream_id` 更新 (headers.rs の `handle_headers` / `handle_continuation` の更新部) を通らない。このため、リセット分岐側で `last_successful_stream_id` の更新も行う
4. **ストリーム生成の要否**: ストリームを生成してからリセットする。これは `reset_stream_internal` が `streams` に存在するストリーム (`exists` が真) のときだけ `Event::StreamReset` 生成・`closed_streams` 登録・`streams` 削除を行うためであり、完了条件の `Event::StreamReset` 生成を満たすために生成が必須である。本経路ではリセットは `last_recv_stream_id` 更新後に行われるため `is_idle_stream` 判定は発火しないが、生成してからリセットすることで一貫性を保証する
5. **ストリーム ID 更新**: 受信したストリーム ID は `last_recv_stream_id` の更新対象とし、RST_STREAM 送信済みのストリームは `last_successful_stream_id` の更新対象に含める (RST_STREAM 送信は RFC 9113 Section 6.8 の last-stream-id 更新対象。組み込み位置は 3 参照)。変換後は `Ok` を返す
6. **エラーコード**: RFC 9113 Section 5.1.2 の MUST (PROTOCOL_ERROR または REFUSED_STREAM のストリームエラーで応答する) に従い、REFUSED_STREAM を選択する。REFUSED_STREAM は RFC 9113 Section 8.7 の自動再試行を許可し、送信側 `start_stream` の同時上限超過と同じエラーコードで一貫する

## 完了条件

- 同時ストリーム数上限超過時の新規 HEADERS で、RST_STREAM (REFUSED_STREAM) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成され、ストリームが `streams` から削除される
- ストリームエラーが `process()` から呼び出し側へ伝播せず、接続が維持される
- 上限超過時も field block がデコードされ、HPACK 状態が維持される (リセット後に別の新規 HEADERS が正常に処理でき、COMPRESSION_ERROR にならないこと)
- 上限超過時は単一フレーム・多フレームのいずれでも `Event::HeadersReceived` / `Event::TrailersReceived` / `Event::StreamClosed` が生成されない
- 多フレーム field block (HEADERS + CONTINUATION) で上限超過した場合も、CONTINUATION を吸収してリセットされる
- GOAWAY の last-stream-id に REFUSED_STREAM したストリームが反映される
- リセット後の遅延 DATA が `Event::DataDiscarded` で破棄され、接続が維持される
- 上記を検証する単体テスト (同時ストリーム数上限を超えた新規 HEADERS をサーバーロールで受信するケース) を `tests/test_connection.rs` の `mod reset_stream` に追加し、`CHANGES.md` の `## develop` に `[FIX]` エントリ (shiguredo-changelog スキルに従う) を追加し、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `refs/rfc9113.txt` — Section 4.3 (Header Compression and Decompression) / Section 5.1.2 (Stream Concurrency) / Section 5.4.2 (Stream Error Handling) / Section 6.4 (RST_STREAM) / Section 6.8 (GOAWAY) / Section 8.7 (Retry)
- `src/connection/headers.rs` — `Connection::handle_headers`
- `src/connection.rs` — `Connection::check_concurrent_streams_limit` / `Connection::reset_stream_internal` / `Connection::is_idle_stream`
- `issues/closed/0106-bug-fix-header-error-paths.md` — 同種のエラー経路変換を実施した先行対応。残課題として本経路を記録
