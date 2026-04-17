# shiguredo_http2 の tests/ に WebTransport 単体テストを追加する

- Created: 2026-04-17
- Completed: 2026-04-17
- Model: Opus 4.7

## 概要

`tests/test_webtransport.rs` を新規追加し、`src/webtransport/mod.rs` の `WtSession` に対する意図的なエラーパス・境界値テストを整備する。

## 背景

現状の WebTransport sans I/O テスト配置は以下のみ:

- `src/webtransport/mod.rs` の `#[cfg(test)] mod tests`: 基本ユニット (initiate, open_bidi 等)
- `pbt/tests/prop_webtransport.rs`: PBT (Capsule ラウンドトリップなど)

issue 0002 / 0006 で追加した以下の公開 API について、PBT で拾いにくい境界値と意図的なエラーパスのユニットテストが不足している:

- `WtSession::send_max_data` / `send_max_stream_data` / `send_max_streams`
- `WtSession::grow_recv_window` / `grow_stream_recv_window` / `grow_max_streams`
- `WtSession::flow_control` / `flow_control_mut` / `stream` / `config`

CLAUDE.md の命名規則「単体テストのファイル名は `tests/test_<module>.rs` とし、`src/<module>.rs` に対応させること」に従い、`tests/test_webtransport.rs` を用意する。

## 根拠

- サンプル (`examples/wt_server`) が RFC 準拠で動作することを担保するには、下回りの境界値を確認する必要がある
- PBT は正常系のラウンドトリップに強いが、「既存値より小さい maximum を `grow_*` に渡した時の挙動」「存在しない stream_id での呼び出し」などは明示的に書く方が意図が伝わる
- `WtFlowControl::update_send_max` / `WT_MAX_STREAMS` 減少のエラーなど、仕様で MUST と定義されている挙動を単体テストで固定する

## 対応内容

### `tests/test_webtransport.rs` 新規追加

以下を目安にテストを書く (PBT と重複しないこと):

1. `grow_recv_window` で `WT_MAX_DATA` capsule がエンコードされて出力される
2. `grow_stream_recv_window` で存在しない stream_id を指定すると `invalid_stream_id`
3. `grow_max_streams(bidi=true)` で `WT_MAX_STREAMS (bidi)` が出力される
4. peer からの `WtMaxData` 受信で送信ウィンドウが増える / 減少値は `flow_control_error`
5. `close` 後の `send_stream_data` / `open_bidi_stream` / `send_datagram` はエラーになる
6. `open_bidi_stream` をストリーム上限まで呼ぶと `flow_control_error`
7. `send_max_data` の直接呼び出しで capsule 出力が得られる
8. `config()` / `flow_control()` / `stream()` getter の動作

## 完了条件

- `tests/test_webtransport.rs` が追加されている
- `cargo test --test test_webtransport` が green
- `cargo fmt` / `cargo clippy -D warnings` が通る

## 依存

- 0002, 0006 でクローズ済みの API を利用

## 解決方法

- `tests/test_webtransport.rs` を新規追加し 8 ケースを実装:
  - `grow_recv_window_emits_wt_max_data`: 受信ウィンドウ拡張が `WT_MAX_DATA` を emit
  - `grow_stream_recv_window_unknown_stream_errors`: 存在しない stream_id → `InvalidStreamId`
  - `grow_max_streams_bidi_emits_capsule`: bidi 上限拡張が `WT_MAX_STREAMS (bidi)` を emit
  - `received_wt_max_data_decrease_errors`: ピアから受信した `WT_MAX_DATA` の減少値が `FlowControlError`
  - `send_after_close_errors`: `close` 後の `send_stream_data` / `send_datagram` が `SessionStateError`
  - `open_bidi_stream_over_limit_errors`: ローカル上限超過が `FlowControlError`
  - `send_max_data_emits_capsule`: `send_max_data` 直接呼び出しで capsule 出力
  - `getters_return_expected_state`: `stream()` / `flow_control()` / `config()` の動作
- `cargo test --test test_webtransport` が 8/8 pass
- `cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings` が通る
