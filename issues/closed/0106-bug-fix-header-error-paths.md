# ヘッダー処理の残存エラー経路でストリームが streams に残り続け、接続が終了する問題を修正する

- Created: 2026-08-10
- Completed: 2026-08-10
- Branch: feature/fix-header-error-paths
- Polished: 2026-08-10

## 目的

`Connection::process_headers` のエラー経路のうち、0105 で変換した 2 経路 (1xx + END_STREAM / Content-Length 不一致) 以外の経路は、`Err` を直接返して `is_closed` ブロック末尾の `streams.remove` に到達しない。このため:

- 既存ストリームで検出される経路では、ストリームが `streams` に残り続ける (リソースリーク)
- 新規ストリーム (ストリーム未作成) で検出される経路を含むすべての経路で、ストリームエラーが `process()` から呼び出し側へそのまま伝播し、RST_STREAM が送信されない。RFC 9113 Section 8.1.1 は malformed をストリームエラー (PROTOCOL_ERROR) として処理することを MUST とし、Section 5.4.2 は検出したストリームエラーを RST_STREAM で処理すると定める

## 現状

`src/connection/headers.rs` の `process_headers` が `Err` を返す経路のうち、以下は変換対象として残っている:

- 状態遷移前の検証エラー経路:
  - 初回 HEADERS の疑似ヘッダー欠如
  - トレーラー検証エラー (`validate_trailers`)
  - END_STREAM なしトレーラー (`trailers must be sent with END_STREAM`)
  - 非初回 HEADERS の疑似ヘッダー
  - リクエスト / レスポンス検証エラー (`validate_request_headers` / `validate_response_headers`)
  - `:protocol` のネゴシエーション違反 (ENABLE_CONNECT_PROTOCOL 未設定。サーバーロールのみで検出される)
  - Content-Length パースエラー (`extract_content_length`)
- `stream.state_machine_mut().recv_headers(end_stream)?` の状態遷移エラー (HalfClosedRemote 状態のストリームへの HEADERS 受信等、ErrorCode::StreamClosed)

これらの `Err` は `handle_headers` / `handle_continuation` を経由して `Connection::process` へ伝播し、接続が終了する。検出時点のストリームの扱いは経路により異なる:

- 既存ストリームで検出される経路 (非初回 HEADERS の疑似ヘッダー、トレーラー検証エラー、END_STREAM なしトレーラー、クライアントの初回レスポンス検証エラー、`recv_headers` 状態遷移エラー等) は、ストリームが `streams` に残り続ける (0105 の修正前と同じ孤立の形態。0101 / 0105 の残課題として記録済み)
- 新規ストリーム (ストリーム未作成) で検出される経路 (サーバーが初回リクエスト HEADERS で検出する疑似ヘッダー欠如・`validate_request_headers`・`:protocol` ネゴシエーション違反等) は、ストリームが作成されないため `streams` に残らないが、ストリームエラーが `process()` から呼び出し側へそのまま伝播して接続が終了する (同じ検証エラーでも、クライアントの初回レスポンスなど検出時点でストリームが生成済みの場合は既存ストリームで検出される)

## 設計方針

0105 と同じ `reset_stream_internal` パターンで、変換可能な経路を RST_STREAM 送信 + `streams` 削除に変換し、接続を維持する。

- 状態遷移エラー (`recv_headers` の Err): 0101 の `handle_data` の `recv_data` 処理と同じパターン (`.is_err()` で捕捉して `reset_stream_internal` を STREAM_CLOSED で呼ぶ)
- 状態遷移前の検証エラー: `reset_stream_internal` を PROTOCOL_ERROR で呼ぶ。ストリームが `streams` に存在する場合はそのままリセットできる。新規ストリーム (ストリーム未作成) で検出される経路 (サーバーが初回リクエスト HEADERS で検出する疑似ヘッダー欠如・`validate_request_headers`・`:protocol` ネゴシエーション違反) は、ストリームを生成してから `reset_stream_internal` でリセットする (RST_STREAM 送信・`Event::StreamReset` 生成・`closed_streams` 登録・`streams` 削除を一貫させる)
- RFC 9113 Section 6.4 の idle ストリームへの RST_STREAM 禁止は本 issue の経路では抵触しない。ワイヤー上のストリームは HEADERS 受信により idle ではなくなる (Section 5.1 / 5.1.1)。実装上も `handle_headers` が `process_headers` より先に `last_recv_stream_id` を更新するため、`reset_stream_internal` の idle 判定 (`is_idle_stream`) は新規ストリーム経路でも発火しない。接続エラーへの昇格は行わない (RFC 9113 Section 8.1.1 は malformed をストリームエラーとして処理することを MUST としている)
- エラーコードは RFC 9113 の該当節に従う (malformed は PROTOCOL_ERROR、状態遷移違反は STREAM_CLOSED)

## 完了条件

- 既存ストリームで検出される変換対象の経路で、ストリームが `streams` から削除され、RST_STREAM (該当エラーコード) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成される
- 新規ストリーム (ストリーム未作成) で検出される変換対象の経路で、ストリームが生成されてからリセットされ、RST_STREAM (PROTOCOL_ERROR) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成される
- `Event::HeadersReceived` / `Event::TrailersReceived` / `Event::StreamClosed` は変換対象の経路で生成されない
- ストリームエラーが `process()` から呼び出し側へ伝播せず、接続が維持される
- リセット後の遅延 DATA が `Event::DataDiscarded` で破棄され、接続が維持される
- 上記を検証する単体テスト (変換対象の経路を、適用可能なロール (クライアント / サーバー) で) が追加され、既存テスト `test_initial_headers_without_pseudo_is_error` (tests/test_connection.rs) が新挙動を検証する形に書き換えられる (テスト名も新挙動に合わせて変更する)
- `CHANGES.md` の `## develop` に `[FIX]` エントリ (shiguredo-changelog スキルに従う) が追加される
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 解決方法

1. `src/connection/headers.rs` の `process_headers` のエラー経路 (状態遷移前のヘッダー検証エラーと `recv_headers` 状態遷移エラー) を `reset_stream_internal` による処理に変換した。`Event::HeadersReceived` / `Event::TrailersReceived` は push せず、`connection_window_consumed: 0` で `Event::StreamReset` を生成する
2. 新規ストリーム (ストリーム未作成) で検出される経路は、ヘルパー `reset_headers_validation_error` がストリームを生成してから `reset_stream_internal` (PROTOCOL_ERROR) でリセットする (RST_STREAM 送信・`Event::StreamReset` 生成・`closed_streams` 登録・`streams` 削除を一貫させる)
3. 状態遷移エラー (`recv_headers` の Err) は `handle_data` の `recv_data` 処理と同じパターンで `reset_stream_internal` を STREAM_CLOSED で呼ぶ
4. 変換後は `process_headers` が `Ok` を返すため、`handle_headers` / `handle_continuation` の `last_successful_stream_id` 更新が RST_STREAM 送信済みのストリームにも適用される (0105 で確立した挙動を継続し、コード変更は不要)
5. `process_headers` の doc コメントを新挙動に合わせて更新した (検証エラーが状態遷移エラーより優先される実装判断の明記、idle 判定が発火しない根拠)
6. `tests/test_connection.rs` の `mod reset_stream` に単体テストを追加・書き換えした。変換対象の全経路 × 適用可能ロール (クライアント / サーバー) をカバーし、RST_STREAM のエラーコード検証・`Event::HeadersReceived` / `Event::TrailersReceived` / `Event::StreamClosed` の非生成・リセット後の遅延 DATA の `Event::DataDiscarded`・GOAWAY last-stream-id への反映 (新規ストリーム経路 / CONTINUATION 経路含む) を検証する。既存の `test_initial_headers_without_pseudo_is_error` を `test_initial_headers_without_pseudo_resets_stream` に書き換えた
7. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加した (shiguredo-changelog スキルに従う)
8. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通ることを確認した

## 参照

- `refs/rfc9113.txt` — Section 5.1 (Stream States) / Section 5.4.2 (Stream Error Handling) / Section 6.4 (RST_STREAM) / Section 8.1.1 (Malformed Messages)
- `refs/rfc8441.txt` — Section 3 (`:protocol` ネゴシエーション違反。RFC 7540 の Section 8.1.2.6 を引用しており、RFC 9113 では Section 8.1.1 に相当)
- `src/connection/headers.rs` — `Connection::process_headers` / `Connection::handle_headers` / `Connection::handle_continuation`
- `src/connection.rs` — `Connection::reset_stream_internal` / `Connection::handle_data`
- `issues/closed/0101-bug-fix-handle-data-stream-error.md` — ヘッダー検証エラー経路を残課題として記録した先行対応
- `issues/closed/0105-bug-fix-orphaned-closed-stream.md` — 状態遷移後エラー経路を変換した先行対応
- 残課題: `src/connection/headers.rs` の `handle_headers` の CONNECT 確立済みストリームへの HEADERS 拒否経路と同時ストリーム数上限チェック (REFUSED_STREAM) 経路、`src/connection.rs` の `handle_frame` の CONNECT 確立ストリームへの未知フレーム処理も同様に `Err(stream_error)` を返して接続終了を引き起こすが、本 issue では対象外とする (0101 の残課題と同様に別途対応)
