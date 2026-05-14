# Stream 構造体の凝集度を改善する

Created: 2026-05-14
Model: deepseek-v4-pro

## 対象

`src/stream/mod.rs:16-72`

## 内容

`Stream` 構造体が 19 フィールドを持ち、独立した関心事がフラットに押し込まれている:

- **CONNECT 関連** (4 フィールド): `connect_established`, `request_method`, `has_protocol`, `protocol`
- **Content-Length 管理** (3 フィールド): `expected_content_length`, `received_content_length`, `no_content`
- **送信バッファ管理** (2 フィールド): `pending_end_stream`, `send_buffer`
- **その他** (10 フィールド): `id`, `state`, `flow_control`, `headers`, `recv_buffer`, `initial_headers_received`, `final_response_sent`

各フィールドは `connection/mod.rs` の様々な場所で個別に set/get されており、凝集度が低い。

例: connection/mod.rs:1266 `set_request_method`, L1267 `set_has_protocol`, L1269 `set_protocol`, L1287 `set_connect_established`, L1297 `set_no_content`

## 修正方針

1. CONNECT 関連を `ConnectContext` サブ構造体に抽出する
2. Content-Length 管理を `ContentLengthTracker` サブ構造体に抽出する
3. 管理用のサブ構造体にそれぞれの操作メソッドを集約する
