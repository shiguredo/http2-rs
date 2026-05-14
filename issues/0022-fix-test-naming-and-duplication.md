# テスト命名規則違反と重複を整理する

Created: 2026-05-14
Model: deepseek-v4-pro

## 根拠

CLAUDE.md: 「単体テストのファイル名は `tests/test_<module>.rs` とし、`src/<module>.rs` に対応させること」、「PBT でカバーできるものを単体テストで書かない」

## 内容

### 1. `tests/rfc7541.rs` — 命名規則違反

`rfc7541.rs` という名前で hpack モジュール (`src/hpack/mod.rs`) に対応していない。`tests/test_hpack.rs` にリネームする。

ただしこのファイルの内容は RFC 7541 の Appendix A テストベクターに基づくものであり、`tests/rfc7541.rs` という名前にも一定の合理性はある。

### 2. PBT と単体テストの重複

#### 2.1 `src/flow_control.rs:196-255` — `#[cfg(test)]` が PBT と重複

以下の正常系テストは `pbt/tests/prop_flow_control.rs` でカバー可能:

- `test_new_flow_control`
- `test_consume_send`
- `test_recv_window_update`
- `test_update_initial_window_size`

エラーパスの `test_consume_send_exhausted`、`test_window_update_overflow`、`test_should_send_window_update` は単体テストとして残す。

#### 2.2 `src/stream/state.rs:281-374` — `#[cfg(test)]` が PBT と重複

以下の正常系テストは `pbt/tests/prop_stream_state.rs` でカバー済み:

- `test_idle_to_open`
- `test_idle_to_half_closed_local`
- `test_open_to_half_closed_local`
- `test_open_to_half_closed_remote`
- `test_half_closed_to_closed`

エラーパスの `test_server_response_from_half_closed_remote`、`test_trailer_headers_from_open`、`test_rst_stream_closes` は単体テストとして残す。

#### 2.3 `src/validation.rs:721-974` — `#[cfg(test)]` が過大で PBT と重複

約 253 行のテストコード。以下の正常系テストは `pbt/tests/prop_validation.rs` でカバー可能:

- `test_valid_get_request`
- `test_valid_connect_request`
- `test_valid_response`
- `test_valid_trailers`
- `test_te_trailers_allowed`
- `test_host_authority_match`

### 3. ディレクトリモジュールの PBT が単一ファイル形式

以下の PBT ファイルは単一ファイルで、CLAUDE.md が要求するディレクトリモジュール形式に準拠していない:

- `pbt/tests/prop_connection.rs` (885 行)
- `pbt/tests/prop_frame.rs` (1,439 行)
- `pbt/tests/prop_hpack.rs` (176 行)
- `pbt/tests/prop_stream_state.rs` (346 行)
- `pbt/tests/prop_webtransport.rs` (717 行)

## 修正方針

1. `tests/rfc7541.rs` を `tests/test_hpack.rs` にリネームする
2. PBT と重複する単体テストを削除する（エラーパスは残す）
3. ディレクトリモジュールの PBT を `pbt/tests/prop_<module>/main.rs` 形式に移行する:
   - `prop_connection.rs` → `prop_connection/main.rs`
   - `prop_frame.rs` → `prop_frame/main.rs`
   - `prop_hpack.rs` → `prop_hpack/main.rs`
   - `prop_stream_state.rs` → `prop_stream_state/main.rs`
   - `prop_webtransport.rs` → `prop_webtransport/main.rs`

## CHANGES.md (実装時に追記)

- `## develop` の `### misc` に以下を追加する:
  - `[UPDATE]` テストの命名規則違反と PBT 重複を整理する
    - @voluntas

## 受け入れ基準

- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る
- PBT ファイル内に `#[cfg(test)] mod tests` が存在しないこと
- ディレクトリモジュールの PBT が `main.rs` 形式になっていること
