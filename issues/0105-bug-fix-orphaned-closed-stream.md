# process_headers の状態遷移後エラー経路でストリームが streams に残り続ける問題を修正する

- Created: 2026-08-09
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-orphaned-closed-stream
- Polished: 2026-08-09

## 目的

`Connection::handle_headers` (`src/connection/headers.rs`) の `process_headers` は、状態機械 `recv_headers` による状態遷移を完了させた後に、1xx + END_STREAM malformed 検出 (RFC 9113 Section 8.1 / Section 8.1.1) や Content-Length 不一致検出 (RFC 9113 Section 8.1.1) で `Err` を返す経路を持つ。この `Err` は `is_closed` ブロック末尾の `streams.remove` に到達しないため、孤立ストリームが `streams` マップに残り続ける (残る状態は経路により Closed または HalfClosedRemote。詳細は「現状」参照)。

残った Closed ストリームへの遅延 DATA は `Event::DataDiscarded` で破棄される (0102 で対応済み) が、エントリ自体は RST_STREAM 受信まで永続し、以下の問題を引き起こす:

- マップ内に孤立ストリームが蓄積する (リソースリーク)
- このストリームエラーは `process()` から呼び出し側へそのまま伝播し、RST_STREAM が送信されない。RFC 9113 Section 8.1.1 は malformed をストリームエラー (PROTOCOL_ERROR) として処理することを MUST とし、Section 5.4.2 は検出したストリームエラーを RST_STREAM で処理すると定める

## 現状

- `src/connection/headers.rs` の `process_headers` は、`stream.state_machine_mut().recv_headers(end_stream)?` (状態遷移) を完了させた後に以下で `Err` を返す経路を持つ:
  - 1xx 情報レスポンス + END_STREAM (malformed、RFC 9113 Section 8.1 / 8.1.1。クライアントロールのみ判定される)
  - END_STREAM + Content-Length != 0 の不一致 (malformed、RFC 9113 Section 8.1.1。サーバーロールとクライアントロールの両方で判定される)
- 残るストリームの状態は受信時点の状態により異なる: クライアントがリクエストを END_STREAM 付きで送信済み (HalfClosedLocal) の場合、1xx + END_STREAM 経路とクライアントロールの Content-Length 不一致経路は Closed に遷移する。クライアントがリクエストボディ送信中 (Open) の場合とサーバーロールの Content-Length 不一致経路 (Idle) の場合は HalfClosedRemote に遷移する
- これらの `Err` は `process_headers` から `handle_headers` / `handle_continuation` を経由して `handle_frame` → `process()` へ伝播し、`streams` からの削除処理 (`is_closed` ブロック末尾の `streams.remove`) に到達しない
- 残ったストリームへの遅延 DATA は、Closed の場合は `is_stream_closed` 判定で `Event::DataDiscarded` により破棄され、エントリは RST_STREAM 受信まで残る (0102 の対応で既存の `is_stream_closed` 分岐に `Event::DataDiscarded` 通知が追加された) が、HalfClosedRemote の場合は `recv_data` の状態遷移エラーで `reset_stream` (STREAM_CLOSED) に落ちて削除される。いずれの場合も `process_headers` のエラー経路自体はエントリを削除しない
- 一方、`handle_data` のストリームエラー経路は 0101 で `reset_stream` による RST_STREAM 変換 + `streams` 削除に統一済みであり、HEADERS 経路が未統一のまま残っている (0101 残課題記載の CONNECT 確立ストリームへの未知フレーム処理も未変換)

## 設計方針

### 採用: ストリームエラー経路の `reset_stream` 化 (0101 のパターンに統一)

`process_headers` のエラー経路 (状態遷移後の malformed 検出) で `Err` を直接返す代わりに、`handle_data` と同じ `reset_stream` / `reset_stream_internal` による処理へ変換し、RST_STREAM 送信・`closed_streams` 登録・`streams` 削除・`Event::StreamReset` 生成を一貫させる。

- 変換対象は状態遷移後の 2 経路 (1xx + END_STREAM / Content-Length 不一致) のみ。状態遷移前のエラー (ヘッダー検証エラー等) は既存挙動 (Err を返す) を維持する。`recv_headers` 自体の状態遷移エラー (HalfClosedRemote 状態への HEADERS 受信等) も既存挙動を維持する (状態遷移が成立しないエラーであり、本 issue の対象外。残課題参照)
- エラー経路では `Event::HeadersReceived` / `Event::TrailersReceived` を push せず、`Event::StreamReset` (connection_window_consumed: 0) のみを生成する (DATA を破棄しないため)。`reset_stream_internal` は `headers` モジュール (子モジュール) から private のまま呼び出せる。`is_closed` ブロック内の `stream` 借用はエラー経路で NLL により解消され、ブロック内で `reset_stream_internal` を直接呼べる (0101 の `handle_data` と同じパターン)
- 既に Closed に遷移したストリーム (1xx + END_STREAM 経路) への RST_STREAM 送信は、RFC 9113 Section 5.1 の closed 状態へのフレーム送信制限 (MUST NOT) に厳密には抵触しうるが、`src/connection.rs` の `reset_stream_internal` が既に採用している判断 (既存挙動を維持する) に合わせる
- `process_headers` が `Ok` を返すようになるため、`handle_headers` / `handle_continuation` の `last_successful_stream_id` 更新が RST_STREAM 送信済みのストリームにも適用される挙動変化が生じる。RST_STREAM 送信は RFC 9113 Section 6.8 の「sender might have taken some action on」に該当するため、GOAWAY の last-stream-id に含めてよい扱いとする (`src/connection/headers.rs` の該当コメントを更新する)

## 完了条件

- `process_headers` の状態遷移後エラー経路 (1xx + END_STREAM / Content-Length 不一致) で、ストリームが `streams` から削除され、RST_STREAM (PROTOCOL_ERROR) が送信され、`Event::StreamReset` (connection_window_consumed: 0) が生成される。`Event::HeadersReceived` / `Event::TrailersReceived` / `Event::StreamClosed` は生成されない
- ストリームエラーが `process()` から接続エラーとして伝播せず、接続が維持される
- 既存の `test_data_discarded_on_closed_stream_in_map` (0102 で追加) は、修正後 `process()` が `Err` ではなく `Ok` を返し、マップ内 Closed も発生しなくなるため前提が変わる。新挙動 (RST_STREAM 送信 + `Event::StreamReset` + `streams` 削除 + 遅延 DATA の `Event::DataDiscarded` で再リセットしないこと) を検証する形に書き換える。テスト名も新前提に合わせて変更する (例: `test_malformed_1xx_end_stream_resets_stream` 等)
- 上記を検証する単体テスト (クライアントロールとサーバーロールの両方) が追加され、`cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 解決方法

1. `src/connection/headers.rs` の `process_headers` の状態遷移後エラー経路 (1xx + END_STREAM / Content-Length 不一致) を `reset_stream_internal` による処理に変換する。`Event::HeadersReceived` / `Event::TrailersReceived` は push せず、`connection_window_consumed: 0` で `Event::StreamReset` を生成する
2. `src/connection.rs` の `reset_stream_internal` は `headers` モジュール (子モジュール) から private のまま呼び出せるため、公開範囲の変更は不要
3. `src/connection/headers.rs` の `last_successful_stream_id` 更新コメントを、RST_STREAM 送信済みのストリームも更新対象に含める旨に修正する
4. 単体テストを追加する (1xx + END_STREAM malformed / Content-Length 不一致で RST_STREAM 送信 + `Event::StreamReset` + `streams` 削除 + 接続維持を検証。クライアントロールとサーバーロールをカバーする)
5. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する

## 参照

- `refs/rfc9113.txt` — Section 5.1 (Stream States) / Section 5.4.2 (Stream Error Handling) / Section 6.8 (GOAWAY) / Section 8.1 (HTTP Message Framing) / Section 8.1.1 (Malformed Messages)
- `src/connection/headers.rs` — `Connection::handle_headers` / `Connection::process_headers` / `Connection::handle_continuation`
- `src/connection.rs` — `Connection::reset_stream` / `Connection::reset_stream_internal` / `Connection::handle_data`
- `issues/closed/0101-bug-fix-handle-data-stream-error.md` — `handle_data` のストリームエラーを `reset_stream` に変換した先行対応
- `issues/closed/0102-change-connection-window-exhaustion.md` — マップ内 Closed への遅延 DATA 破棄 (`Event::DataDiscarded`) を確立した対応

## 残課題 (本 issue のスコープ外)

- `recv_headers` 自体の状態遷移エラー経路 (HalfClosedRemote 状態のストリームへの HEADERS 受信等) は、マップ内に HalfClosedRemote ストリームを残しつつ `Err` を伝播させる (本 issue の題目と同種の孤立) が、本 issue では対象外とし既存挙動を維持する (0101 の残課題と同様。別途対応)
- 状態遷移前のエラー経路 (トレーラー検証エラー・`trailers must be sent with END_STREAM`・非初回 HEADERS の疑似ヘッダー等) も、既存ストリームをマップに残しつつ `Err` を伝播させる (0101 の残課題に記載済みのヘッダー検証エラー経路)。本 issue では対象外とし既存挙動を維持する (別途対応)
- 本 issue の修正により、マップ内に Closed ストリームを残す経路がなくなるため、`is_stream_closed` による「マップ内 Closed」判定 (`handle_data` / `handle_headers` / `handle_continuation` の 3 箇所) は実質的に到達不能になる。削除は change カテゴリの別 issue で判断する (本 issue は bug fix のため対象外)
