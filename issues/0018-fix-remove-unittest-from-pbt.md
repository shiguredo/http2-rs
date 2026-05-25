# PBT ファイル内の単体テストを tests/ に移動する

- Priority: Low
- Created: 2026-05-14
- Model: deepseek-v4-pro
- Branch: feature/fix-move-unittest-from-pbt

## 目的

AGENTS.md の「pbt 以下に unittest を書かないこと」に違反して、`pbt/tests/` 配下の 3 ファイルに `#[cfg(test)] mod tests` ブロックが残存している。これらを適切な `tests/` ディレクトリに移動する。

なお、これらのテストは PBT では実現できないケース（意図的なエラーパス、境界値、網羅的列挙）であり、AGENTS.md の「単体テスト: 意図的なエラーパス、境界値など PBT で実現できないケース」に該当する。したがって**削除ではなく移動**が正しい対応。

## 優先度根拠

テストの正確性・CI に影響しないテスト配置のリファクタリング。規約違反は軽微だが、テストファイルの場所が規約と一致しないと将来の開発者が混乱する。

## 現状

### 1. `pbt/tests/prop_connection.rs` (L918-1043)

5 件の単体テスト:

| テスト名 | 内容 | PBT で代替不可の理由 |
|---|---|---|
| `test_initiate_does_not_emit_window_update_when_default` | `connection_window_size == DEFAULT` で WINDOW_UPDATE が送信されないこと | PBT strategy は `connection_window_size > DEFAULT` のみ生成するため、DEFAULT ケースは PBT で到達しない |
| `test_send_settings_does_not_emit_window_update_when_default` | 同上 (`send_settings` 経路) | 同上 |
| `test_continuation_without_headers_is_error` | HEADERS なしで CONTINUATION を送信するとエラー | 意図的なエラーパス |
| `test_rst_stream_on_idle_is_error` | idle ストリームへの RST_STREAM がエラー | 意図的なエラーパス |
| `test_client_rejects_enable_push_from_server` | サーバーからの ENABLE_PUSH=1 がエラー | 意図的なエラーパス |

### 2. `pbt/tests/prop_event.rs` (L222-278)

2 件の単体テスト:

| テスト名 | 内容 | PBT で代替不可の理由 |
|---|---|---|
| `test_priority_update_is_stream_level` | PriorityUpdateReceived が stream_id を持つことの検証 | 単一バリアントの分類正確性を直接確認するテストであり、PBT のランダム生成では「このバリアントが必ずテストされる」保証がない |
| `test_all_connection_level_events` | 全接続レベルイベントの網羅的列挙テスト | PBT は全バリアントの網羅を保証しない |

### 3. `pbt/tests/prop_error.rs` (L233-277)

2 件の単体テスト:

| テスト名 | 内容 | PBT で代替不可の理由 |
|---|---|---|
| `test_known_error_codes_mapping` | 既知エラーコード全件の正引き・逆引き検証 | PBT はランダム選択のため全件カバーを保証しない |
| `test_gap_values_are_unknown` | 0x0e..0x100 および 0x103..0x110 範囲の網羅テスト | 特定範囲の境界値テスト |

## 設計方針

### 移動先

| 移動元 | 移動先 |
|---|---|
| `pbt/tests/prop_connection.rs` 内の 5 テスト | `tests/test_connection.rs` (新設) |
| `pbt/tests/prop_event.rs` 内の 2 テスト | `tests/test_event.rs` (新設) |
| `pbt/tests/prop_error.rs` 内の 2 テスト | `tests/test_error.rs` (既存) |

### 手順

1. 各 PBT ファイルから `#[cfg(test)] mod tests { ... }` ブロック全体を削除する
2. テスト内容を移動先ファイルに追加する
3. 共有ヘルパーの処理（下記参照）

### ヘルパー関数・定数の扱い

PBT 本体と単体テストの両方から使用されるヘルパーがあるため、以下のとおり対処する:

| ヘルパー | 定義場所 | PBT 使用 | 対処 |
|---|---|---|---|
| `encode_frame` | prop_connection.rs L22 | あり (L72, L103 等多数) | PBT に残し、`tests/test_connection.rs` に同一関数を**複製**する（FrameEncoder を呼ぶ 5 行程度の関数） |
| `create_continuation` | prop_connection.rs L54 | あり (L77, L114) | PBT に残し、`tests/test_connection.rs` に**複製**する |
| `KNOWN_ERROR_CODES` | prop_error.rs L9 | なし（`known_error_code_value()` は値を直接列挙しており参照していない） | `tests/test_error.rs` に**移動**する。prop_error.rs からは削除する |

### import の置き換え

`#[cfg(test)] mod tests` 内は `use super::*;` で import しているが、移動先の `tests/` ファイルでは外部クレートとして参照する:

```rust
use shiguredo_http2::{Connection, Limits, ErrorCode, CONNECTION_PREFACE_LEN};
use shiguredo_http2::frame::{
    Frame, FrameDecoder, FrameEncoder, SettingsFrame, NonZeroStreamId,
    ContinuationFrame, RstStreamFrame, WindowUpdateFrame,
};
use shiguredo_http2::settings::{Setting, MAX_MAX_FRAME_SIZE};
```

`encode_frame` / `create_continuation` はクレート外に存在しないため、テストファイル内にローカル定義する。

## 変更対象ファイル

- `pbt/tests/prop_connection.rs`: `#[cfg(test)] mod tests` ブロック削除
- `pbt/tests/prop_event.rs`: `#[cfg(test)] mod tests` ブロック削除
- `pbt/tests/prop_error.rs`: `#[cfg(test)] mod tests` ブロック削除 + `KNOWN_ERROR_CODES` 定数削除
- `tests/test_connection.rs`: 新設（5 テスト追加）
- `tests/test_event.rs`: 新設（2 テスト追加）
- `tests/test_error.rs`: 追記（2 テスト追加）

## 完了条件

- `pbt/tests/` 配下に `#[cfg(test)] mod tests` ブロックが存在しない
- 移動した 9 件のテストが `tests/` 配下で通る
- `cargo test --workspace` が通る
- `cargo clippy --workspace --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
