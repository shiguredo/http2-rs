# connection/mod.rs を分割する

Created: 2026-05-14
Model: deepseek-v4-pro

## 対象

`src/connection/mod.rs` (1,974 行)

## 内容

`connection/mod.rs` が 1,974 行と肥大化しており、以下の独立した関心事が 1 ファイルに詰め込まれている:

- SETTINGS 処理 (`handle_settings`: 114 行、`send_settings`: 15 行)
- DATA フレーム処理 (`handle_data`: 89 行、`queue_data`、`flush_stream_data`、`flush_all_stream_data`: 約 200 行)
- HEADERS フレーム処理 (`handle_headers` + `process_headers` + `send_header_block` + `send_response` + `send_trailers`: 約 700 行)
- CONNECT 関連処理
- イベント発行、ストリーム管理
- ヘッダー継続 (CONTINUATION) 処理
- GOPAWAY 処理
- フレームルーティング (`handle_frame`)

## 修正方針

以下のサブモジュールに分割する:

1. `src/connection/headers.rs` — HEADERS 送受信、process_headers、send_header_block、send_response、send_trailers
2. `src/connection/settings.rs` — SETTINGS 送受信、handle_settings、send_settings、initiate
3. `src/connection/data.rs` — DATA 送受信、send_data、handle_data、フラッシュ処理
4. `src/connection/mod.rs` — Connection 構造体、handle_frame (ルーティング)、GOAWAY/PING/RST_STREAM、ストリーム管理

## 利点

- 各関心事のコードナビゲーションが容易になる
- テスト対象の分離が容易になる
- 同時編集の競合が減少する
