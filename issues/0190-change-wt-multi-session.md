# 1 コネクション 1 セッション固定を解消し、複数 WebTransport セッションを扱えるようにする

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-wt-multi-session
- Polished: {YYYY-MM-DD}

## 目的

draft-ietf-webtrans-http2-16 Section 4.1 / Section 5.1 は、同一 HTTP/2 接続上の複数 WebTransport セッションと pooling を前提としている。実装は `WtServerRequest` が `ServerConnection` を所有し、`accept()` が接続を丸ごと driver に渡すため、1 接続 1 セッション固定になっている。2 本目の CONNECT や通常リクエストは driver に無視され、クライアントはタイムアウトする。接続共有型の API に変更するか、制約を公開 doc に明記する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::from_connection` は `ServerConnection` を受け取り、`accept()` はそれを `DriverState` にムーブする。`DriverState::handle_event` は CONNECT 以外のイベントを `_ => {}` で捨てる。`ServerConnection` を返す API はない。

公開 doc には「1 接続 1 セッション」という制約が書かれていない。

## 設計方針

- 接続レベルの dispatcher が `ServerConnection` を所有し、CONNECT ごとに WebTransport driver を spawn する。
- 既存の単一セッション API を維持する場合は、公開 doc に「同一接続の他ストリームは無視する」と明記し、追加ストリームを `RST_STREAM` / `GOAWAY` で明示拒否する。
- 2 本目の CONNECT と通常リクエストを同時に扱う統合テストを追加する。
- 設計変更が大きい場合は `issues/pending/` へ移動し、API 設計を先行して確定する。

## 完了条件

- 同一 HTTP/2 接続で複数の WebTransport セッションを確立できること、または単一セッション制約が公開 doc とプロトコル応答で明示されていること。
- 2 本目の CONNECT / 通常リクエストの扱いを検証するテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
