# PBT ファイルから単体テストを除去する

Created: 2026-05-14
Model: deepseek-v4-pro

## 根拠

CLAUDE.md: 「pbt 以下に unittest を書かないこと」

## 対象と内容

### 1. `pbt/tests/prop_connection.rs:813-885`

`#[cfg(test)] mod tests` 内に以下の単体テストが存在する:

- `test_continuation_without_headers_is_error`
- `test_rst_stream_on_idle_is_error`
- `test_client_rejects_enable_push_from_server`

これらのテスト内容は `prop_connection.rs` 内の `proptest!` ブロックの `prop_*` テストですでにカバーされている。

### 2. `pbt/tests/prop_event.rs:222-278`

`#[cfg(test)] mod tests` 内に以下の単体テストが存在する:

- `test_priority_update_is_stream_level`
- `test_all_connection_level_events`

いずれも PBT で既に検証済みのプロパティ。

### 3. `pbt/tests/prop_error.rs:236-281`

`#[cfg(test)] mod tests` 内に以下の単体テストが存在する:

- `test_known_error_codes_mapping`
- `test_gap_values_are_unknown`

PBT の `prop_known_error_code_from_u32` および `prop_unknown_error_code_preserves_value` でカバー済み。

## 修正方針

該当する `#[cfg(test)] mod tests` ブロックとその中の単体テストを削除する。必要に応じて対応する `tests/test_<module>.rs` に移動する。
