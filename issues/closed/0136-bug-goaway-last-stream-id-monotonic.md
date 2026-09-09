# send_goaway の複数回呼び出しで last-stream-id が増加する (RFC 9113 Section 6.8 違反)

- Created: 2026-08-24
- Completed: 2026-09-10
- Branch: feature/fix-goaway-last-stream-id-monotonic
- Polished: 2026-09-09

## 目的

`Connection::send_goaway` (`src/connection.rs`) が複数回呼び出された場合、`last_successful_stream_id` の増加に伴い last-stream-id が増加し、RFC 9113 Section 6.8 の「Endpoints MUST NOT increase the value they send in the last stream identifier」に違反する問題を修正する。

## 現状

`send_goaway` は毎回現在の `last_successful_stream_id` を `LastStreamId` に載せて GOAWAY を送信する。`last_successful_stream_id` は `src/connection/headers.rs` の `handle_headers` / `handle_continuation` / `reset_refused_concurrent_stream` で単調増加する (最後に成功したストリーム ID を更新。`process_headers` 自体は更新しない)。

GOAWAY 送信後の新規ストリーム HEADERS は `handle_headers` の `GoawaySent` チェックで接続エラーになるため、`last_successful_stream_id` が伸びるのは既存ストリーム側である。例えば、GOAWAY 送信前に受信した `END_HEADERS` なし HEADERS の CONTINUATION 完了 (`handle_continuation`) や、既存ストリームへの HEADERS 受信 (`handle_headers`) で更新される。このため、1 回目の `send_goaway` 後にこれらの処理が進むと `last_successful_stream_id` が伸び、2 回目の `send_goaway` は 1 回目より大きい last-stream-id を送ることになる。既送信の last-stream-id を保持して非増加を強制する仕組みがない。

RFC 9113 Section 6.8 (refs/rfc9113.txt) の原文: "Endpoints MUST NOT increase the value they send in the last stream identifier, since the peers might already have retried unprocessed requests on another connection."

## 設計方針

- GOAWAY 送信時に使用した last-stream-id を接続内に記録し (`Option<u32>` を `None` で初期化)、2 回目以降の `send_goaway` は `last_successful_stream_id` と記録値の小さい方を使う。初回は既存どおり現在の `last_successful_stream_id` を使う
- RFC 9113 Section 6.8 が推奨する 2 段階 GOAWAY (初回を 2^31-1 で送り、その後により小さい値で再送する) は現行 API が初回値に `last_successful_stream_id` を使うため対象外とする。本 issue は既送信値の非増加のみを保証する
- 2 回目以降の GOAWAY で last-stream-id が増加しないことを検証するテストを追加する。テストでは 2 回の `send_goaway` の間で `last_successful_stream_id` を実際に増やす必要があるため、`END_HEADERS` なし HEADERS → `send_goaway` → CONTINUATION (`END_HEADERS`) → `send_goaway` の順で更新させる

## 完了条件

- 複数回の `send_goaway` 呼び出しで last-stream-id が増加しないこと (2 回目の呼び出し前後で `last_successful_stream_id` が増えていても、2 回目の last-stream-id は 1 回目以下であること)
- テストが追加され、`cargo test --all` が通過すること

## 解決方法

- `src/connection.rs` の `Connection` に、直近に GOAWAY で送信した last-stream-id を保持する `last_sent_goaway_stream_id: Option<u32>` を追加した。`send_goaway` は初回に `last_successful_stream_id` を使い、2 回目以降は `last_successful_stream_id` と記録値の小さい方を使って送信し、送信成功後に記録を更新する (RFC 9113 Section 6.8 の MUST NOT increase)
- `send_goaway` の doc に、複数回呼び出しても last-stream-id が既送信値以下に制限されることを追記した
- `tests/test_connection.rs` に、`END_HEADERS` なし HEADERS → `send_goaway` → CONTINUATION で `last_successful_stream_id` を伸ばす → `send_goaway` の順で、2 回目の last-stream-id が 1 回目に固定されることを検証するテストを追加した
- `CHANGES.md` の `## develop` に `[FIX]` エントリを追加した
