# connection/mod.rs をサブモジュールに分割する

- Priority: Low
- Created: 2026-05-14
- Completed: 2026-05-26
- Model: deepseek-v4-pro
- Branch: feature/refactor-split-connection-module

## 目的

`src/connection/mod.rs` が 2,272 行に肥大化しており、HEADERS 送受信、SETTINGS 処理、DATA 送受信という独立した関心事が 1 ファイルに混在している。新機能追加やバグ修正時にファイル内の位置特定が困難であり、コードレビュー時の認知負荷が高い。AGENTS.md の「テストが長くなるのはモジュール自体が大きすぎるサインなので `src/<module>.rs` 側の分割を検討すること」に沿い、サブモジュールに分割する。

## 優先度根拠

機能や正確性に直接影響しないリファクタリングであり、既存テストの通過に問題はない。ただし `pbt/tests/prop_connection.rs` も 1,043 行に達しており、AGENTS.md の分割基準に該当する。今すぐの対応は不要だが、今後の機能追加（WebTransport 拡張等）で connection module がさらに肥大化する前に実施すべきため Low とする。

## 関連 issue

- `issues/0016-refactor-improve-stream-cohesion.md`: Stream 構造体のサブ構造体抽出。両者は独立して実装可能（0015 はファイル分割のみで構造体定義を変更しない、0016 は Stream 構造体の内部構造変更）。実装順序の制約はない
- `issues/closed/0036-refactor-move-mod-tests-to-tests-dir.md`: `src/` 内の `#[cfg(test)] mod tests` を `tests/` に移管した。本 issue でも同じ原則に従う

## 現状

`src/connection/mod.rs` (2,272 行) に以下の関心事が混在している:

- 構造体定義・コンストラクタ・アクセサ
- 入出力バッファ管理 (`feed`, `process`, `poll_event`, `poll_output`)
- フレームルーティング (`handle_frame`)
- HEADERS 送受信 (`handle_headers`, `process_headers`, `send_response`, `send_trailers`, `send_header_block`, `handle_continuation`)
- SETTINGS 送受信 (`initiate`, `send_settings`, `handle_settings`, `update_stream_windows`, `send_initial_connection_window_update`)
- DATA 送受信 (`send_data`, `queue_data`, `flush_stream_data`, `flush_all_stream_data`, `handle_data`)
- GOAWAY / PING / RST_STREAM / WINDOW_UPDATE / PRIORITY_UPDATE 送受信
- ストリーム管理 (`start_stream`, `try_remove_closed_stream`, `is_idle_stream`, `is_stream_closed`, `check_not_idle_stream`, `check_concurrent_streams_limit`)
- ヘルパー関数 (`extract_content_length`, `calculate_header_list_size`, `concatenate_cookies`)

## 設計方針

### 分割後の構成

```
src/connection/
  mod.rs       — Connection 構造体定義、コンストラクタ、アクセサ、
                 入出力バッファ管理、handle_frame (ルーティング)、
                 GOAWAY / PING / RST_STREAM / WINDOW_UPDATE / PRIORITY_UPDATE 送受信、
                 ストリーム管理、ヘルパー関数、send_frame
  headers.rs   — HEADERS 送受信、ヘッダー継続処理
  settings.rs  — SETTINGS 送受信、initiate、設定反映
  data.rs      — DATA 送受信、フラッシュ処理
```

### メソッド割り当て

#### `mod.rs` に残すもの

| カテゴリ | メソッド |
|---|---|
| コンストラクタ・アクセサ | `new`, `client`, `server`, `role`, `state`, `local_settings`, `remote_settings`, `is_active`, `is_closed` |
| 入出力 | `feed`, `mark_preface_received`, `mark_preface_sent`, `process`, `poll_event`, `poll_output`, `has_output` |
| フレームルーティング | `handle_frame` |
| 共通送信 | `send_frame` |
| PING | `send_ping`, `handle_ping` |
| GOAWAY | `send_goaway`, `handle_goaway` |
| RST_STREAM | `reset_stream`, `handle_rst_stream` |
| WINDOW_UPDATE | `send_window_update`, `handle_window_update` |
| PRIORITY_UPDATE | `handle_priority_update` |
| ストリーム管理 | `start_stream`, `try_remove_closed_stream`, `is_idle_stream`, `is_stream_closed`, `check_not_idle_stream`, `check_concurrent_streams_limit` |
| ヘルパー | `extract_content_length`, `calculate_header_list_size` |

`concatenate_cookies` は `impl Connection` の外側にあるモジュールレベルの `pub(crate) fn` であり、`mod.rs` に残す。

#### `headers.rs` に移動するもの

| メソッド |
|---|
| `handle_headers` |
| `process_headers` |
| `send_header_block` |
| `send_response` |
| `send_trailers` |
| `handle_continuation` |

#### `settings.rs` に移動するもの

| メソッド |
|---|
| `initiate` |
| `send_settings` |
| `send_initial_connection_window_update` |
| `handle_settings` |
| `update_stream_windows` |

#### `data.rs` に移動するもの

| メソッド |
|---|
| `send_data` |
| `queue_data` |
| `flush_stream_data` |
| `flush_all_stream_data` |
| `handle_data` |

### 分割粒度の判断基準

headers / settings / data の 3 つに分割する理由:

- この 3 グループはそれぞれ 300-500 行のまとまったコードであり、独立した責務を持つ
- mod.rs に残すメソッド群（PING, GOAWAY, RST_STREAM, WINDOW_UPDATE 等）は個々が 10-30 行程度であり、独立サブモジュールにするほどの規模がない
- ストリーム管理とフレームルーティングは全サブモジュールのメソッドから呼ばれるため、mod.rs に残すのが自然

### 可視性設計

- Connection 構造体定義（全フィールド）は mod.rs に留まる。サブモジュールにフィールドは分散しない
- サブモジュール宣言: `mod headers;` / `mod settings;` / `mod data;`（private。クレート外から直接アクセスする必要がないため `pub(crate)` は不要）
- サブモジュール内のメソッド: `pub(super) fn` とする。Rust では親モジュール (mod.rs) は子モジュールの private アイテムを参照できないため、`handle_frame` から `handle_headers` 等を呼ぶには `pub(super)` が必要
- mod.rs 内の private メソッド (`fn`): `send_frame`, `is_idle_stream`, `is_stream_closed`, `check_not_idle_stream`, `check_concurrent_streams_limit` 等はすべて private のまま。Rust のモジュール可視性ルールにより、子モジュールは親の private アイテムを参照可能であるため、サブモジュールの `impl Connection` ブロックから直接呼び出せる
- 公開 API (`pub fn`): `send_response`, `send_data`, `send_trailers`, `initiate`, `send_settings` はサブモジュール内で `pub fn` のまま定義する。`Connection` 型自体が `pub` で re-export されているため、これらのメソッドは外部から利用可能
- `mod.rs` に `use` 文は不要（`impl Connection` ブロック内のメソッドは型に紐づいており、`use` とは無関係に解決される）

### テスト分割

issue 0036 の原則（公開 API 経由でテスト可能なものは `tests/` に配置）に従う。

`pbt/tests/prop_connection.rs` (1,043 行) を `pbt/tests/prop_connection/main.rs` に移行し、サブモジュール対応で分割する:

- `pbt/tests/prop_connection/main.rs` — 接続レベルの PBT (GOAWAY, PING, ステートマシン等)
- `pbt/tests/prop_connection/headers.rs` — HEADERS 関連 PBT
- `pbt/tests/prop_connection/settings.rs` — SETTINGS 関連 PBT
- `pbt/tests/prop_connection/data.rs` — DATA フレーム関連 PBT

`src/connection/` の各サブモジュールには `#[cfg(test)] mod tests` を配置しない（issue 0036 で移管済みの方針に従う）。

現在 mod.rs 末尾に存在する `concatenate_cookies` 用の `#[cfg(test)] mod tests` ブロック（proptest を含む約 190 行）は、`concatenate_cookies` が `impl Connection` 外のモジュールレベル free function であり `pub(crate)` のため、`tests/test_connection.rs` に移動する（公開 API 経由でテスト可能）。

## 完了条件

- `src/connection/mod.rs` から headers / settings / data のメソッドが分離されている
- 分割前後で公開 API に変更がない（`Connection` 型の全 `pub fn` メソッドが外部から同じシグネチャで呼び出せる）
- `pbt/tests/prop_connection/main.rs` + サブモジュールに PBT が分割されている
- `cargo test --workspace` が通る
- `cargo clippy --workspace --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
- fuzz ターゲット (`fuzz_connection`, `fuzz_connection_client`, `fuzz_connection_interactive`, `fuzz_connection_preface`) がビルドできる (`cargo check --manifest-path fuzz/Cargo.toml`)

## 解決方法

### src/connection/ の分割

`src/connection/mod.rs` (2,272 行) から以下の 3 サブモジュールを抽出した:

- `src/connection/data.rs`: DATA 送受信メソッド (send_data, queue_data, flush_stream_data, flush_all_stream_data, handle_data)
- `src/connection/headers.rs`: HEADERS 送受信メソッド (send_response, send_trailers, handle_headers, process_headers, handle_continuation, send_header_block, extract_content_length, calculate_header_list_size)
- `src/connection/settings.rs`: SETTINGS 送受信メソッド (initiate, send_settings, send_initial_connection_window_update, handle_settings, update_stream_windows)

可視性設計:
- 公開 API (`pub fn`): send_response, send_data, send_trailers, initiate, send_settings
- mod.rs から呼ばれるメソッド: `pub(super) fn` (handle_data, handle_headers, handle_continuation, handle_settings, flush_stream_data, flush_all_stream_data)
- サブモジュール内のみで使用: `fn` (queue_data, process_headers, send_header_block, send_initial_connection_window_update, update_stream_windows)

### PBT の分割

`pbt/tests/prop_connection.rs` (1,043 行) をディレクトリモジュールに分割した:

- `pbt/tests/prop_connection/main.rs`: 接続レベル PBT + 共通ヘルパー + mod 宣言
- `pbt/tests/prop_connection/headers.rs`: HEADERS 関連 PBT
- `pbt/tests/prop_connection/settings.rs`: SETTINGS 関連 PBT
- `pbt/tests/prop_connection/data.rs`: DATA フレーム関連 PBT

### 備考

`concatenate_cookies` テストブロック (`#[cfg(test)] mod tests`) は `pub(crate)` の可視性制約により `src/connection/mod.rs` に残した（統合テストからは `pub(crate)` アイテムにアクセスできないため）。
