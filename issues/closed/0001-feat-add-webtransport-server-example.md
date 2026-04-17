# WebTransport サーバーサンプルを追加する (親 issue)

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

`examples/wt_server/` に WebTransport over HTTP/2 (draft-ietf-webtrans-http2-14) のエコーサーバーサンプルを追加する。
http3-rs の `examples/wt_server/` に相当する位置付けで、draft-14 準拠の双方向/単方向ストリームと WT DATAGRAM capsule をエコーする。

## 背景

- `src/webtransport/` に Sans I/O レベルの `WtSession` / Capsule / ストリーム状態管理は実装済み
- `crates/tokio-http2/` は Extended CONNECT (`:protocol=webtransport`) を `HeadersReceived` イベントで受理できるが、`WtSession` との統合 API が存在しない
- サンプルコードが「お手本」として要求されるため、tokio-http2 に高レベル API を追加したうえでサンプルを書く必要がある
- http3-rs の wt_server と対になる HTTP/2 版のサンプルを用意することで、WebTransport を HTTP/2 側でも検証可能にする

## 根拠

- shiguredo_http2 は WebTransport over HTTP/2 の下回りを既に実装済みだが、統合層と実動作するサンプルがないため、仕様の網羅性と実装の妥当性を検証できていない
- サンプル (`canary.py` 相当) を書くことで draft-14 の相互運用性確認が可能になる

## 対応内容 (子 issue)

- 0002: shiguredo_http2 の WebTransport 統合層 (SETTINGS / Connection / Stream / Event 公開 API)
- 0003: tokio-http2 の WebTransport Server API 基本 (`WtServerRequest`, `WtServerSession`, bidi/uni ストリームハンドル)
- 0004: CONNECT ストリーム ↔ `WtSession` の glue (DATA frame ルーティング, `poll_output` 送信ループ)
- 0005: WT DATAGRAM capsule の送受信 API
- 0006: 動的フロー制御 (WT_MAX_DATA / WT_MAX_STREAM_DATA / WT_MAX_STREAMS の自動発行)
- 0007: 統合テスト (`crates/tokio-http2/tests/` に bidi/uni/datagram/reject のエコー統合テスト)
- 0008: `examples/wt_server/` の実装 (CLI, TLS, エコー処理, README)
- 0009: `CHANGES.md` 更新と仕上げ

## 完了条件

- `cargo run -p wt_server` でサーバーが起動し、双方向/単方向ストリーム、WT DATAGRAM が動作する
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が全て通る
- `CHANGES.md` に `[ADD]` エントリが記載されている
- 子 issue 0002〜0009 が全て `issues/closed/` に移動している

## 対象 RFC / draft

- draft-ietf-webtrans-http2-14 (WebTransport over HTTP/2)
- RFC 9297 (HTTP Datagrams and the Capsule Protocol)
- RFC 8441 (Extended CONNECT)
- RFC 9113 (HTTP/2)
- RFC 9000 Section 2, 3 (Stream Types and States)

## 解決方法

子 issue 0002〜0009 をすべて `issues/closed/` で完了した。

1. 0002: `shiguredo_http2` の WebTransport 統合層 (SETTINGS / Connection / Stream / Event)
2. 0003: `tokio-http2` の WebTransport サーバー API 外形
3. 0004: CONNECT ストリーム ↔ `WtSession` の glue (actor pattern の driver task)
4. 0005: WT DATAGRAM capsule の送受信
5. 0006: 動的フロー制御 (`WT_MAX_DATA` / `WT_MAX_STREAM_DATA` / `WT_MAX_STREAMS` の自動発行)
6. 0007: WebTransport 統合テスト (`test_webtransport.rs` に bidi_echo / reject)
7. 0008: `examples/wt_server/` (draft-14 対応のエコーサーバーサンプル)
8. 0009: `CHANGES.md` 整備と全 issue クローズ

結果:

- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` がすべて green
- `cd examples/wt_server && cargo run` で HTTP/2 WebTransport サーバーが起動できる
