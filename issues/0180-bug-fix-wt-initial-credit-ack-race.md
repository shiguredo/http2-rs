# SETTINGS ACK 前の CONNECT で WebTransport 初期 credit が 0 のまま停止する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-initial-credit-ack-race
- Polished: {YYYY-MM-DD}

## 目的

`WtServerRequest::accept` は ACK 済みの自広告 SETTINGS からローカル初期値を決める。クライアントがサーバー SETTINGS を ACK する前に CONNECT を送ると、初期値がすべて 0 になり、driver の自動拡張も `initial == 0` で停止する。さらに `WtServerSession` / `WtSessionHandle` には WT_MAX_DATA / WT_MAX_STREAMS を送る API がないため、セッションがデータを流せないまま停止する。draft-ietf-webtrans-http2-16 Section 4.3.1 / Section 11.2 に沿って回復経路を用意する。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept` は `config.apply_advertised_settings(conn.acked_settings())` でアプリ指定の `WtConfig` を ACK 済み SETTINGS のみに置き換える。ACK がまだ処理されていない場合、全項目が 0 になる。

`DriverState::maybe_grow_session_window` は `should_send_max_data(0)` が常に false、`maybe_grow_max_streams` は `initial == 0` で即 `Ok(())`、`maybe_grow_stream_window` も初期値 0 では発火しない。`WtServerSession` / `WtSessionHandle` には `send_max_data` / `send_max_streams` / `send_max_stream_data` がない。

draft-16 Section 4.3.1 は SETTINGS と ACK が in-flight である場合を明示的に扱い、ACK 前は Initial Value 0 を使う。Section 11.2 は初期値 0 の場合、ピアがデータを送る前に WT_MAX_DATA を送る必要があるとしている。クライアントが SETTINGS を処理する前に CONNECT を送ることは禁止されていない。

## 設計方針

- 2xx 応答後に、アプリ指定の受信上限または ACK 済み上限から WT_MAX_DATA / WT_MAX_STREAMS を送る経路を追加する。
- `WtServerSession` / `WtSessionHandle` にフロー制御 capsule の送信 API を追加する。
- 0 初期値のセッションを拒否する方針を採る場合は、`accept` のエラー条件と doc を明記する。
- SETTINGS ACK を CONNECT より後に送るクライアントを使った統合テストを追加する。既存の `perform_connect` は必ず SETTINGS 受信後に CONNECT を送るため、この経路を通らない。

## 完了条件

- SETTINGS ACK が CONNECT より後になっても、2xx 後に WT_MAX_DATA / WT_MAX_STREAMS が送られ、ピアがストリームデータを送れること。
- フロー制御 capsule の送信 API が `WtServerSession` / `WtSessionHandle` に追加されていること。
- ACK 順序を入れ替えた統合テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
