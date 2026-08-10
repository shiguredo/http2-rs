# マップ内 Closed ストリーム判定が到達不能になったため削除する

- Created: 2026-08-10
- Completed: {YYYY-MM-DD}
- Branch: feature/change-remove-closed-stream-check
- Polished: {YYYY-MM-DD}

## 目的

0105 の修正により、`process_headers` の状態遷移後エラー経路 (1xx + END_STREAM / Content-Length 不一致) が `reset_stream_internal` による `streams` 削除に変換されたため、マップ内に Closed 状態のストリームを残す経路がなくなった。これに伴い、`Connection::is_stream_closed` による「マップ内 Closed」判定 (3 箇所) は実質的に到達不能になった。到達不能な判定を削除してコードを整理する。

## 現状

- `src/connection.rs` の `is_stream_closed` (stream_id がマップ内で Closed 状態かどうかを判定するヘルパー)
- 使用箇所 3 箇所:
  - `src/connection.rs` の `handle_data` のクローズ済み破棄判定 (`is_stream_closed(sid) || !streams.contains_key(&sid)`)
  - `src/connection/headers.rs` の `handle_headers` の遅延 HEADERS 破棄判定 (`is_previously_closed || is_stream_closed(sid)`)
  - `src/connection/headers.rs` の `handle_continuation` の遅延 HEADERS 破棄判定 (`is_previously_closed || is_stream_closed(expected_stream_id)`)
- 0105 の修正前は、`process_headers` のエラー経路 (状態遷移後に Err を返して削除処理に到達しない) でマップ内 Closed が発生し、`handle_data` の破棄判定や遅延 HEADERS の破棄に寄与していた
- マップ内 Closed を生み出す経路は 0105 の修正で消滅しており、`recv_headers` の状態遷移エラー経路は HalfClosedRemote を残すのみで Closed は残らない (0105 の残課題として記録済み)

## 設計方針

- 3 箇所の「マップ内 Closed」判定を削除し、`is_stream_closed` ヘルパーごと削除する
- `handle_data` の破棄判定は `!streams.contains_key(&sid)` のみにする
- `handle_headers` / `handle_continuation` の遅延 HEADERS 破棄判定は `is_previously_closed` のみにする
- 削除後にマップ内 Closed が発生する経路が存在しないことをテストで確認する

## 完了条件

- `is_stream_closed` と 3 箇所の「マップ内 Closed」判定が削除されている
- クローズ済みストリームへの遅延 DATA / 遅延 HEADERS の破棄挙動が変わらないこと (既存テストが通ること)
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `src/connection.rs` — `Connection::is_stream_closed` / `Connection::handle_data`
- `src/connection/headers.rs` — `Connection::handle_headers` / `Connection::handle_continuation`
- `issues/closed/0105-bug-fix-orphaned-closed-stream.md` — マップ内 Closed の実質到達不能化を残課題として記録した先行対応
