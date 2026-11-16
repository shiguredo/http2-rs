# `WtServerRequest::accept` のリクエスト検証とエラー応答を統一する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-accept-validation
- Polished: {YYYY-MM-DD}

## 目的

`WtServerRequest::from_connection` は `HeadersReceived` の `protocol` と `end_stream` を保持しないため、`accept()` 単体では Extended CONNECT か、要求が既に half-close されているかを検証できない。さらに `accept()` は Content-Length / Content-Type / Transfer-Encoding を検査せず、`selected_protocol` 不一致時は 4xx も RST_STREAM も送らない。`reject()` は `100..=599` を有効とするため `reject(200)` が 2xx + END_STREAM を送る。受理境界の検証とエラー応答を統一する。Origin の `allowed_origin=None` スキップは closed 0062 の決定を維持し、本 issue の対象外とする。

## 現状

- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest` は `conn` / `stream_id` / `headers` のみを保持する。`HeadersReceived` の `protocol` と `end_stream` は捨てられる。
- `accept()` は `:scheme=https` と TLS 1.3 を検査するが、`:method=CONNECT` / `:protocol=webtransport` / `end_stream=false` を検査しない。
- RFC 9297 Section 3.2 は Capsule Protocol を Content-Length / Content-Type / Transfer-Encoding 付きのメッセージで使うことを MUST NOT とし、違反を malformed として扱う MUST を定める。
- `selected_protocol` が `WT-Available-Protocols` にない場合、`Err(InvalidArgument)` を返すだけで接続を drop する。
- `reject()` の `100..=599` 検査は 2xx を通し、1xx は `send_response` の END_STREAM 検査でエラーになる。

## 設計方針

- `from_connection` に `protocol` と `end_stream` を渡し、`accept()` で `CONNECT` / `webtransport` / `end_stream == false` を検証する。または `from_connection` を `Result` にして Extended CONNECT 以外を構築不能にする。
- Content-Length / Content-Type / Transfer-Encoding の存在を検査し、4xx または RST_STREAM (`PROTOCOL_ERROR`) で拒否する。
- `selected_protocol` 不一致は 400 + END_STREAM を送る。
- `reject()` の有効範囲を最終レスポンス (300..=599 など) に限定し、引数エラーは `InvalidArgument` にする。
- 上記各経路の応答を検証するテストを追加する。

## 完了条件

- Extended CONNECT 以外の要求が `accept()` で拒否されること。
- Content-Length / Content-Type / Transfer-Encoding 付きの WT CONNECT が拒否されること。
- `selected_protocol` 不一致で 4xx が返ること。
- `reject(200)` / `reject(100)` が拒否されること。
- 各エラー経路のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
