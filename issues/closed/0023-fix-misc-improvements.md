# コードベースの軽微な改善を実施する

- Priority: Medium
- Created: 2026-05-14
- Model: deepseek-v4-pro
- Completed: 2026-05-26
- Branch: feature/fix-misc-improvements

## 目的

コードベース内の到達不能コード、未検証入力、意図不明な分岐、冗長な定義を整理する。

## 優先度根拠

個々の改善は軽微だが、#5 (ポート番号未検証) は RFC 違反入力を受け入れるバグであり Medium とする。

## 項目一覧

### 1. FlowControl の到達不能分岐

`src/connection/mod.rs:1768-1772` で `recv_window_update` のエラーを `is_connection_error()` で判定して RST_STREAM に変換するパターンがあるが、`recv_window_update` は常に `connection_error` を返すため `return Err(e)` 分岐が到達不能。

修正案: 到達不能分岐を削除し、常に RST_STREAM を送信するロジックに簡略化する。

### 2. ポート番号の範囲未検証

`src/validation.rs` の `is_valid_connect_authority` がポート番号の ASCII 数字チェックのみ行い、数値範囲 (0-65535) を検証していない。`99999` のような無効なポート番号も受理する。

修正案: パース後に `0..=65535` 範囲チェックを追加する。

### 3. recv_headers の不明瞭な分岐

`src/stream/state.rs:180-191` で `end_stream == false` のとき `self.state` を返す（実質 no-op）パターンにコメントがない。

修正案: 「既に Open/HalfClosedLocal であり、end_stream なしのヘッダーでは状態遷移しない」旨のコメントを追加する。

### 4. SettingsFrame の new() と Default の重複

`src/frame/mod.rs` で `SettingsFrame::new()` と `Default for SettingsFrame` の内容が同一。

修正案: `Default` を `Self::new()` への委譲に統一し、重複実装を削除する（既にそうなっている場合はコードを確認して問題なしとする）。

### 5. send_frame の暗黙的契約

`src/connection/mod.rs` の `send_frame` が `encode` 成功時にのみバッファをクリアするという暗黙の契約に依存している。

修正案: コメントで「encode が成功した場合のみ output_buffer にデータが追加される。失敗時は encoder 内部バッファが不変であることを前提とする」旨を明記する。

### 6. calculate_header_list_size と concatenate_cookies のテスト可達性

`calculate_header_list_size` (private) と `concatenate_cookies` (`pub(crate)`) は `tests/` や `pbt/` から直接テスト不可。現在 `src/connection/mod.rs` 内の `#[cfg(test)] mod tests` からのみテストされている。

修正案: `concatenate_cookies` は `pub(crate)` なので `tests/test_connection.rs` から crate 内テストとしてアクセス可能（同一クレート内の integration test）。`calculate_header_list_size` は入力をヘッダーリストのサイズ計算に使うだけの純粋関数であり、HEADERS 送信の PBT で間接的にカバーされている。既存テストを `tests/test_connection.rs` に移動する（issue 0018 と同期する）。

## 変更対象ファイル

- `src/connection/mod.rs`: #1, #5 修正
- `src/validation.rs`: #2 修正
- `src/stream/state.rs`: #3 コメント追加
- `src/frame/mod.rs`: #4 確認・修正
- `tests/test_connection.rs`: #6 テスト移動

## 完了条件

- 到達不能分岐が除去されている (#1)
- ポート番号 0-65535 範囲チェックが追加されている (#2)
- recv_headers の no-op 分岐にコメントがある (#3)
- SettingsFrame::new() と Default に重複がない (#4)
- send_frame に暗黙的契約のコメントがある (#5)
- concatenate_cookies テストが tests/ に存在する (#6)
- `cargo test --workspace` が通る
- `cargo clippy --workspace --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る

## 備考: 既に解決済みの項目

以下 6 件は既に解決済みのため除外した:

- validate_stream_id_parity (関数自体が削除済み)
- DATA フレームの部分ウィンドウ消費コメント (追加済み)
- to_settings_list の Vec::with_capacity (Vec::new() に変更済み)
- Limits::new() (ビルダーパターンに置換済み)
- fuzz_flow_control.rs の欠落 (issue 0046 で追加済み)

## 解決方法

6 項目のうち 4 項目を修正し、2 項目は対応不要と判断した:

1. **#1 到達不能分岐**: `handle_window_update` の `is_connection_error()` チェックと `return Err(e)` 分岐を削除。WindowIncrement 型が非ゼロを保証するためオーバーフローのみ発生する旨のコメントを追加
2. **#2 ポート番号検証**: `is_valid_port` 関数を追加し、ポート番号の 0-65535 範囲チェックを実装。PBT strategy も `1u16..=65535u16` に修正
3. **#3 コメント追加**: `recv_headers` の no-op 分岐に「end_stream なしの HEADERS は情報ヘッダー等であり状態遷移しない」旨のコメントを追加
4. **#4 SettingsFrame**: 既に `Default` が `Self::new()` に委譲済みのため対応不要
5. **#5 send_frame コメント**: encode 成功時のみバッファ追加される暗黙的契約をコメントで明記
6. **#6 テスト移動**: `concatenate_cookies` は `pub(crate)` であり integration test からアクセスできないため、現在の `#[cfg(test)] mod tests` 配置が正しい。移動不要
