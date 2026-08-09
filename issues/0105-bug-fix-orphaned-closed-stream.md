# process_headers のエラー経路で Closed 状態のストリームが streams に残り続ける問題を修正する

- Created: 2026-08-09
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-orphaned-closed-stream
- Polished: {YYYY-MM-DD}

## 目的

`Connection::handle_headers` (`src/connection/headers.rs`) の `process_headers` は、状態機械 `recv_headers` による状態遷移 (HalfClosedLocal + END_STREAM → Closed) を完了させた後に、1xx + END_STREAM malformed 検出 (RFC 9113 Section 8.1 / Section 8.1.1) や Content-Length 不一致検出で `Err` を返す経路を持つ。この `Err` は `streams` からの削除処理 (`is_closed` 判定後の `streams.remove`) に到達しないため、**Closed 状態のストリームが `streams` マップに残り続ける**。

残ったストリームへの遅延 DATA は `is_stream_closed` 判定で破棄される (0102 で対応済み) が、エントリ自体は RST_STREAM 受信まで永続し、以下の問題を引き起こす:

- マップ内に Closed エントリが蓄積する (リソースリーク)
- このストリームエラーは `process()` から呼び出し側へそのまま伝播し、RST_STREAM が送信されない (RFC 9113 Section 5.4.2: ストリームエラー検出時は RST_STREAM を送信しなければならない MUST に抵触しうる)

## 現状

- `src/connection/headers.rs` の `process_headers` は、`stream.state_machine_mut().recv_headers(end_stream)?` (状態遷移) の後に以下で `Err` を返す経路を持つ:
  - 1xx 情報レスポンス + END_STREAM (malformed、RFC 9113 Section 8.1 / 8.1.1)
  - END_STREAM + Content-Length != 0 の不一致 (malformed、RFC 9113 Section 8.1.1)
- これらの `Err` は `process_headers` から `handle_headers` → `handle_frame` → `process()` へ伝播し、`streams` からの削除処理 (`is_closed` ブロック末尾の `streams.remove`) に到達しない
- 一方、`handle_data` のストリームエラー経路は 0101 で `reset_stream` による RST_STREAM 変換 + `streams` 削除に統一済みであり、HEADERS 経路だけが未統一のまま残っている
- 0102 の対応で `handle_data` は `is_stream_closed` 判定を復活させたため、マップ内 Closed エントリへの遅延 DATA は `Event::DataDiscarded` で破棄される (挙動は正しい)。ただしエントリ自体は削除されない

## 設計方針

### 採用: ストリームエラー経路の `reset_stream` 化 (0101 のパターンに統一)

`process_headers` のエラー経路 (状態遷移後の malformed 検出) で `Err` を直接返す代わりに、`handle_data` と同じ `reset_stream` / `reset_stream_internal` による処理へ変換し、RST_STREAM 送信・`closed_streams` 登録・`streams` 削除・`Event::StreamReset` 生成を一貫させる。

ただし、状態遷移前のエラー (ヘッダー検証エラー等) は既存挙動 (Err を返す) を維持する。状態遷移後のみを変換対象とする。

判断が割れる場合は停止してユーザーに確認する。

## 完了条件

- `process_headers` の状態遷移後エラー経路 (1xx + END_STREAM / Content-Length 不一致) で、ストリームが `streams` から削除され、RST_STREAM が送信される
- ストリームエラーが接続エラーとして伝播せず、接続が維持される
- 既存の `test_data_discarded_on_closed_stream_in_map` (マップ内 Closed への遅延 DATA 破棄、0102 で追加) が維持される (エラー経路でストリームが削除されるため、マップ内 Closed が発生しなくなる場合はテストの前提を実態に合わせて修正する)
- 上記を検証する単体テストが追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 解決方法

1. `src/connection/headers.rs` の `process_headers` の状態遷移後エラー経路を `reset_stream_internal` による処理に変換する (0101 の `handle_data` と同じパターン)
2. `src/connection.rs` の `reset_stream_internal` を再利用可能な形で公開範囲を調整する (必要に応じて `pub(super)` 等)
3. 単体テストを追加する (1xx + END_STREAM malformed / Content-Length 不一致で RST_STREAM 送信 + `streams` 削除 + 接続維持を検証)
4. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する

## 参照

- `refs/rfc9113.txt` — Section 5.1 (Stream States) / Section 5.4.2 (Stream Errors) / Section 8.1 (Informational Responses) / Section 8.1.1 (Malformed Messages)
- `src/connection/headers.rs` — `Connection::handle_headers` / `Connection::process_headers`
- `src/connection.rs` — `Connection::reset_stream` / `Connection::reset_stream_internal` / `Connection::handle_data`
- `issues/closed/0101-bug-fix-handle-data-stream-error.md` — `handle_data` のストリームエラーを `reset_stream` に変換した先行対応
- `issues/closed/0102-change-connection-window-exhaustion.md` — マップ内 Closed への遅延 DATA 破棄 (`Event::DataDiscarded`) を確立した対応
