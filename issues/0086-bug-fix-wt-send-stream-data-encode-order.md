# WtSession::send_stream_data のカプセルエンコード順序を修正する

- Created: 2026-07-30
- Completed: {Completed}
- Branch: feature/fix-wt-send-stream-data-encode-order
- Polished: 2026-07-30

## 目的

`WtSession::send_stream_data` でカプセルエンコードがフロー制御チェックより前に実行されているため、フロー制御違反時に送信バッファが汚染されるバグを修正する。

## 現状

`src/webtransport/mod.rs` の `send_stream_data` メソッド内で、`capsule_encoder.encode()` + `output_buffer.extend()` が先に実行され、その後に `stream.send_data()` と `flow_control.consume_send()` のチェックが行われている。

フロー制御違反でエラーを返した場合、`output_buffer` には既に送信すべきでないカプセルバイトが残っている。後続の `poll_output()` でこの不正データがピアに送信される。

## 設計方針

`stream.send_data()` と `flow_control.consume_send()` の呼び出しを `capsule_encoder.encode()` より前に移動する。

`stream.send_data()` と `flow_control.consume_send()` は純粋なチェックではなく状態変更（`send_offset` の加算・`send_state` の遷移）を伴う。移動後に `send_data` 成功 → `consume_send` 失敗の部分失敗が起き得るが、draft-ietf-webtrans-http2-15 Section 6.5 はフロー制御違反をセッションエラー（MUST close）と規定しているため、部分状態変更はセッション終了で破棄され、不整合は残らない。

## 完了条件

- `send_stream_data` でフロー制御違反時に `output_buffer` にデータが残らないこと
- ストリームレベル違反（`stream send limit exceeded`）とセッションレベル違反（`send window exhausted`）の両方の単体テストが `tests/test_webtransport/integration.rs` に追加されていること
- 違反時に `poll_output()` が `None` を返すことをアサーションに含めること

## 解決方法

`src/webtransport/mod.rs` の `send_stream_data` メソッド内で、`stream.send_data()` と `flow_control.consume_send()` の呼び出しを `capsule_encoder.encode()` + `output_buffer.extend()` より前に移動する。移動後の順序:

1. `stream.can_send()` チェック（既存）
2. `stream.send_data(data.len(), fin)` — ストリームレベルフロー制御 + 状態遷移
3. `flow_control.consume_send(data.len())` — セッションレベルフロー制御
4. `capsule_encoder.encode()` + `output_buffer.extend()` — カプセルエンコード
