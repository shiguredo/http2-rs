# connection/mod.rs をサブモジュールに分割する

Created: 2026-05-14
Priority: Low
Model: deepseek-v4-pro

## 対象

- `src/connection/mod.rs` (1,974 行) — 分割元
- `src/connection/headers.rs` — 新設
- `src/connection/settings.rs` — 新設
- `src/connection/data.rs` — 新設
- `pbt/tests/prop_connection.rs` → `pbt/tests/prop_connection/main.rs` に移行して分割
- `pbt/tests/prop_connection/headers.rs` — 新設
- `pbt/tests/prop_connection/settings.rs` — 新設
- `pbt/tests/prop_connection/data.rs` — 新設

## 内容

`connection/mod.rs` が 1,974 行と肥大化しており、以下の独立した関心事が 1 ファイルに詰め込まれているため、サブモジュールに分割する。

### 分割後の構成

`src/connection/` ディレクトリモジュールとし、Rust の `impl Connection` ブロックを複数ファイルに分散するパターンを使用する。

```
src/connection/
  mod.rs         — Connection 構造体定義、handle_frame (ルーティング)、
                   GOAWAY / PING / RST_STREAM 送受信、ストリーム管理、
                   接続レベルのフロー制御、send_frame (全サブモジュール共通)
  headers.rs     — HEADERS 送受信、process_headers、send_header_block、
                   send_response、send_trailers、ヘッダー継続処理
  settings.rs    — SETTINGS 送受信、initiate、設定反映
  data.rs        — DATA 送受信、send_data、handle_data、
                   フラッシュ処理 (flush_stream_data、flush_all_stream_data)
```

### メソッドの割り当て詳細

#### `mod.rs` に残るもの (構造体定義 + コアロジック + 残存メソッド)

| カテゴリ | メソッド | およその行 |
|---|---|---|
| 構造体定義 | `Connection` struct, `new`, `client`, `server` | L46-171 |
| アクセサ | `role`, `state`, `local_settings`, `remote_settings`, `is_active`, `is_closed` | L186-226 |
| 入出力 | `feed`, `mark_preface_received`, `mark_preface_sent` | L264-309 |
| イベント/出力 | `process`, `poll_event`, `poll_output`, `has_output` | L337-388 |
| フレームルーティング | `handle_frame` | L907-982 |
| 共通 | `send_frame` (全サブモジュールから呼ばれる中核メソッド) | L1929-1935 |
| PING | `send_ping`, `handle_ping` | L691-695, L1596-1609 |
| GOAWAY | `send_goaway`, `handle_goaway` | L698-704, L1612-1622 |
| RST_STREAM | `reset_stream`, `handle_rst_stream` | L679-688, L1445-1464 |
| WINDOW_UPDATE | `send_window_update`, `handle_window_update` | L707-733, L1624-1667 |
| PRIORITY_UPDATE | `handle_priority_update` | L1725-1755 |
| ストリーム管理 | `start_stream`, `validate_stream_id_parity`, `is_idle_stream`, `is_stream_closed`, `check_not_idle_stream`, `check_concurrent_streams_limit`, `try_remove_closed_stream` | L391-448, L1761-1862, L665-676 |
| ヘルパー | `extract_content_length`, `calculate_header_list_size`, `concatenate_cookies` | L1416-1442, L1922-1928, L1942-1973 |

#### `src/connection/headers.rs`

| メソッド | 行 |
|---|---|
| `handle_headers` | L1076-1165 |
| `process_headers` | L1168-1414 |
| `send_header_block` | L1864-1920 |
| `send_response` | L738-838 |
| `send_trailers` | L844-904 |
| `handle_continuation` | L1669-1723 |
| `header_continuation_stream` フィールド管理 | L1669-1689 |

#### `src/connection/settings.rs`

| メソッド | 行 |
|---|---|
| `initiate`, `send_settings` | L232-333 |
| `handle_settings` (update_stream_windows 含む) | L1467-1593 |

#### `src/connection/data.rs`

| メソッド | 行 |
|---|---|
| `send_data` | L496-524 |
| `queue_data` | L527-549 |
| `flush_stream_data` | L552-640 |
| `flush_all_stream_data` | L643-658 |
| `handle_data` | L985-1073 |

### 実装パターン

`impl Connection` ブロックは Rust の同一クレート内であれば複数ファイルに分散できる。各サブモジュールは `use super::*` または `use super::Connection` で `Connection` 型をインポートする。

```rust
// src/connection/mod.rs
pub(crate) mod headers;
pub(crate) mod settings;
pub(crate) mod data;

use headers::*;
use settings::*;
use data::*;

pub struct Connection { ... }

impl Connection {
    // コアメソッド (send_frame, handle_frame 等)
}
```

```rust
// src/connection/headers.rs
use super::*;

impl Connection {
    pub(crate) fn handle_headers(&mut self, frame: HeadersFrame) -> Result<()> { ... }
    pub fn send_response(...) -> Result<()> { ... }
}
```

各サブモジュールから `mod.rs` の `send_frame` メソッドを呼ぶ必要があるが、`impl Connection` のメソッドは同一型の全 `impl` ブロックからアクセス可能なため、循環参照の問題は発生しない。

### 可視性設計

- 外部公開メソッド (`send_response`, `send_data`, `send_trailers`, `send_ping`, `send_goaway`, `send_window_update`, `send_settings`, `start_stream`, `reset_stream`, `initiate`, `feed`, `process`, `poll_event`, `poll_output`) は `pub fn` のままとする
- `handle_*` 系の内部メソッドは `pub(crate) fn` または `fn` に変更する
- `send_frame` は `pub(crate)` または `fn` とする
- 公開 API は `mod.rs` の `pub use` で一括 re-export する (現在の `src/lib.rs` の再エクスポートが機能し続けるため)

## テストの再構成

`pbt/tests/prop_connection.rs` を `pbt/tests/prop_connection/main.rs` に移行し、`src/connection/` のサブモジュール分割に対応して以下に分割する:

- `pbt/tests/prop_connection/main.rs` — 接続レベルの PBT (GOAWAY, PING, ステートマシンラウンドトリップ等)
- `pbt/tests/prop_connection/headers.rs` — HEADERS 関連 PBT
- `pbt/tests/prop_connection/settings.rs` — SETTINGS 関連 PBT
- `pbt/tests/prop_connection/data.rs` — DATA フレーム関連 PBT

`src/connection/` の各サブモジュール (`headers.rs`, `settings.rs`, `data.rs`) には、モジュール固有の `#[cfg(test)] mod tests` を配置する。

`fuzz/fuzz_targets/fuzz_connection.rs` は分割後もビルド可能であることを確認する。

## CHANGES.md (実装時に追記)

- `## develop` の `### misc` に以下を追加する:
  - `[UPDATE]` `connection/mod.rs` を headers/settings/data のサブモジュールに分割する
    - @voluntas

## 受け入れ基準

- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
- `cargo +nightly fuzz` ターゲット (`fuzz_connection`) がビルドできる
- 分割前後で公開 API に変更がない (`pub use` re-export が正しく機能している)
