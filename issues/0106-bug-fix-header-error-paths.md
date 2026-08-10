# ヘッダー処理の残存エラー経路でストリームが streams に残り続ける問題を修正する

- Created: 2026-08-10
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-header-error-paths
- Polished: {YYYY-MM-DD}

## 目的

`Connection::process_headers` のエラー経路のうち、0105 で変換した 2 経路 (1xx + END_STREAM / Content-Length 不一致) 以外の経路は、`Err` を直接返して `is_closed` ブロック末尾の `streams.remove` に到達しない。このため:

- ストリームが `streams` に残り続ける (リソースリーク)
- ストリームエラーが `process()` から呼び出し側へそのまま伝播し、RST_STREAM が送信されない。RFC 9113 Section 8.1.1 は malformed をストリームエラー (PROTOCOL_ERROR) として処理することを MUST とし、Section 5.4.2 は検出したストリームエラーを RST_STREAM で処理すると定める

## 現状

`src/connection/headers.rs` の `process_headers` が `Err` を返す経路のうち、以下は変換対象外として残っている:

- 状態遷移前の検証エラー経路:
  - 初回 HEADERS の疑似ヘッダー欠如
  - トレーラー検証エラー (`validate_trailers`)
  - END_STREAM なしトレーラー (`trailers must be sent with END_STREAM`)
  - 非初回 HEADERS の疑似ヘッダー
  - リクエスト / レスポンス検証エラー (`validate_request_headers` / `validate_response_headers`)
  - `:protocol` のネゴシエーション違反 (ENABLE_CONNECT_PROTOCOL 未設定)
  - Content-Length パースエラー (`extract_content_length`)
- `stream.state_machine_mut().recv_headers(end_stream)?` の状態遷移エラー (HalfClosedRemote 状態のストリームへの HEADERS 受信等、ErrorCode::StreamClosed)

これらの `Err` は `handle_headers` / `handle_continuation` を経由して `Connection::process` へ伝播し、接続が終了する。ストリームは `streams` に残り続ける (0105 の修正前と同じ孤立の形態。0101 / 0105 の残課題として記録済み)。

## 設計方針

0105 と同じ `reset_stream_internal` パターンで、変換可能な経路を RST_STREAM 送信 + `streams` 削除に変換し、接続を維持する。

- 状態遷移エラー (`recv_headers` の Err): 0101 の `handle_data` の `recv_data` 処理と同じパターン (`.is_err()` で捕捉して `reset_stream_internal` を STREAM_CLOSED で呼ぶ)
- 状態遷移前の検証エラー: ストリームが `streams` に存在する場合は `reset_stream_internal` でリセットできる。新規ストリーム (ストリーム未作成) で検出される経路は RFC 9113 Section 6.4 の idle ストリームへの RST_STREAM 禁止との関係を踏まえ、経路ごとに扱いを判断する (ストリームを作成してからリセットする、接続エラーに昇格する等)
- エラーコードは RFC 9113 の該当節に従う (malformed は PROTOCOL_ERROR、状態遷移違反は STREAM_CLOSED)

## 完了条件

- 変換対象の経路で、ストリームが `streams` から削除され、RST_STREAM (該当エラーコード) が送信され、`Event::StreamReset` が生成される
- ストリームエラーが `process()` から接続エラーとして伝播せず、接続が維持される
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `refs/rfc9113.txt` — Section 5.4.2 (Stream Error Handling) / Section 6.4 (RST_STREAM) / Section 8.1.1 (Malformed Messages)
- `src/connection/headers.rs` — `Connection::process_headers` / `Connection::handle_headers` / `Connection::handle_continuation`
- `src/connection.rs` — `Connection::reset_stream_internal` / `Connection::handle_data`
- `issues/closed/0101-bug-fix-handle-data-stream-error.md` — ヘッダー検証エラー経路を残課題として記録した先行対応
- `issues/closed/0105-bug-fix-orphaned-closed-stream.md` — 状態遷移後エラー経路を変換した先行対応
