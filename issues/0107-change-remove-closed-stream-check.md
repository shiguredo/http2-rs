# マップ内 Closed ストリーム判定が到達不能になったため削除する

- Created: 2026-08-10
- Completed: {YYYY-MM-DD}
- Branch: feature/remove-closed-stream-check
- Polished: 2026-08-28

## 目的

`Connection::is_stream_closed` による「マップ内 Closed」判定 (3 箇所) は到達不能になったため削除する。0105 と 0106 の修正で `process_headers` の状態遷移後エラー経路がすべて `reset_stream_internal` による `streams` 削除に変換され、Closed へ遷移したストリームは同一呼び出し内で必ずマップから削除される。到達不能な判定とその説明文を削除してコードを整理する。

## 現状

- `src/connection.rs` の `is_stream_closed` (stream_id がマップ内で Closed 状態かどうかを判定する private ヘルパー)
- 使用箇所 3 箇所:
  - `src/connection.rs` の `handle_data` のクローズ済み破棄判定 (`is_stream_closed(sid) || !self.streams.contains_key(&sid)`)
  - `src/connection/headers.rs` の `handle_headers` の遅延 HEADERS 破棄判定 (`is_previously_closed || self.is_stream_closed(sid)`)
  - `src/connection/headers.rs` の `handle_continuation` の遅延 HEADERS 破棄判定 (`is_previously_closed || self.is_stream_closed(expected_stream_id)`)
- 0105 の修正前は、`process_headers` のエラー経路 (状態遷移後に Err を返して削除処理に到達しない) でマップ内 Closed が発生し、`handle_data` の破棄判定や遅延 HEADERS の破棄に寄与していた
- マップ内 Closed を生み出す経路は 0105 と 0106 の修正で消滅している
  - `process_headers` の状態遷移後エラー経路 (1xx + END_STREAM / Content-Length 不一致) は `reset_stream_internal` 経由で `streams` から削除される (0105)
  - `recv_headers` の状態遷移エラー経路も `reset_stream_internal` 経由で `streams` から削除されるため、HalfClosedRemote も含めてマップ内にストリームは残らない (0106)
- Closed へ遷移するその他の経路も、遷移後に同一呼び出し内で削除へ到達する
  - `handle_data` の END_STREAM 処理、`reset_stream_internal`、`handle_rst_stream`、`try_remove_closed_stream`、`src/connection/headers.rs` の `send_response` / `send_trailers` の `is_closed` 判定
  - 削除の直前に失敗し得る処理は `send_frame` のみであり、Closed 遷移直後に送信する DATA / HEADERS / CONTINUATION / RST_STREAM のエンコードは `src/frame/encoder.rs` で失敗分支を持たない
  - `send_headers` の ReservedLocal 遷移と `recv_headers` の ReservedRemote 遷移は、PUSH_PROMISE を接続エラーとして拒否するため発生しない
- 以上のとおりマップ内 Closed は到達不能であり、判定は常に偽である

## 設計方針

- 3 箇所の「マップ内 Closed」判定を削除し、`is_stream_closed` ヘルパーごと削除する
- `handle_data` の破棄判定は `!self.streams.contains_key(&sid)` のみにする
- `handle_headers` / `handle_continuation` の遅延 HEADERS 破棄判定は `is_previously_closed` のみにする
- 削除に伴い実態と合わなくなるコメントを併めて更新する
  - `src/connection.rs` の `handle_data` 冒頭コメント: 「現在はストリームエラーが `reset_stream_internal` による RST_STREAM 送信 + streams 削除に変換されるため実質的に到達不能である。ヘッダー状態の追跡の一部として防御的に維持する (判定自体の削除は別途検討)」という維持理由の記述を、判定削除後の実態に合わせる
  - `tests/test_connection.rs` の `assert_delayed_data_discarded` の doc コメント: `is_stream_closed` を名指しして「Closed 状態でマップに残存するストリームにも `Event::DataDiscarded` を生成するため、本検証はマップから削除されたことの厳密な証明にはならない」と述べている箇所は、判定削除後は意味が変わるため、残存する削除済みシンボルへの言及として残さない
- マップ内 Closed の非発生を直接検証する新規テストは追加しない。`streams` フィールドと `is_stream_closed` は private であり、公開 API 経由では有限個のテストで「経路が存在しないこと」を証明できないため、専用の検証手段は設けない。代わりに以下で担保する
  - 旧マップ内 Closed 生成シナリオ (1xx + END_STREAM / Content-Length 不一致) が RST_STREAM 送信 + `streams` 削除として振る舞うことは、0105 と 0106 の既存テストが検証済みである
  - 判定削除後も `is_previously_closed` 側の破棄が壊れていないことを、`handle_headers` と `handle_continuation` の両経路のテストで検証する

## 完了条件

- `is_stream_closed` と 3 箇所の「マップ内 Closed」判定が削除されている
- `src/connection.rs` の `handle_data` の維持理由コメントと、`tests/test_connection.rs` の `assert_delayed_data_discarded` の doc コメントが削除後の実態と一致している (存在しないシンボル名への言及が残っていない)
- `handle_headers` の `is_previously_closed` による遅延 HEADERS 破棄を検証する既存テストが通ること
- `handle_continuation` の `is_previously_closed` による遅延破棄 (HPACK 状態更新後に `Event::HeadersReceived` を生成しないこと) を検証するテストが `tests/test_connection.rs` に追加されている
- クローズ済みストリームへの遅延 DATA の破棄挙動 (`Event::DataDiscarded` 通知と `Event::StreamReset` 非再発) が変わらないこと (既存テストが通ること)
- `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` がすべて通る

## 参照

- `src/connection.rs` — `Connection::is_stream_closed` / `Connection::handle_data` / `Connection::reset_stream_internal` / `Connection::try_remove_closed_stream`
- `src/connection/headers.rs` — `Connection::handle_headers` / `Connection::handle_continuation` / `Connection::process_headers`
- `issues/closed/0105-bug-fix-orphaned-closed-stream.md` — マップ内 Closed の実質到達不能化を残課題として記録した先行対応
- `issues/closed/0106-bug-fix-header-error-paths.md` — `recv_headers` の状態遷移エラー経路を `reset_stream_internal` による削除に変換した先行対応
