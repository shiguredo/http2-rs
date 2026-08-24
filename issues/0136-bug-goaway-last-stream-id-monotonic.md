# send_goaway の複数回呼び出しで last-stream-id が増加する (RFC 9113 Section 6.8 違反)

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-goaway-last-stream-id-monotonic
- Polished: {YYYY-MM-DD}

## 目的

`Connection::send_goaway` (`src/connection.rs`) が複数回呼び出された場合、`last_successful_stream_id` の増加に伴い last-stream-id が増加し、RFC 9113 Section 6.8 の「Endpoints MUST NOT increase the value they send in the last stream identifier」に違反する問題を修正する。

## 現状

`send_goaway` は毎回現在の `last_successful_stream_id` を `LastStreamId` に載せて GOAWAY を送信する。`last_successful_stream_id` は `Connection::process_headers` / `handle_continuation` 等のヘッダー処理成功時に単調増加する (最後に成功したストリーム ID を更新)。

したがって、1 回目の `send_goaway` 送信後に新規ストリームの処理が進むと `last_successful_stream_id` が伸び、2 回目の `send_goaway` は 1 回目より大きい last-stream-id を送ることになる。既送信の last-stream-id を保持して非増加を強制する仕組みがない。

RFC 9113 Section 6.8 (refs/rfc9113.txt) の原文: "Endpoints MUST NOT increase the value they send in the last stream identifier, since the peers might already have retried unprocessed requests on another HTTP connection."

## 設計方針

- GOAWAY 送信時に使用した last-stream-id を記録し、以後の `send_goaway` は記録値以下に制限する
- 2 回目以降の GOAWAY で last-stream-id が増加しないことを検証するテストを追加する

## 完了条件

- 複数回の `send_goaway` 呼び出しで last-stream-id が増加しないこと
- テストが追加され、`cargo test --all` が通過すること
