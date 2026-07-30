# WebTransport フロー制御の境界値テストと公開 API テストを追加する

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/add-wt-flow-control-boundary-tests
- Polished: 2026-07-30

## 目的

draft-ietf-webtrans-http2-15 の MUST 要件に対応するテストが欠落している公開 API にテストを追加する。

## 現状

以下の公開 API・MUST 要件に対応するテストが存在しない:

1. `WtSession::send_max_streams` の 2^60 上限エラーパス（draft-15 Section 6.7）
2. `WtFlowControl::update_max_streams` の 2^60 超過エラー（draft-15 Section 6.7）
3. `WtConfig::overlay_settings` の SETTINGS 値上書きと `None` 時の既存値維持
4. `WtSession` の Draining 状態でのストリーム開設・データ送信・データグラム送信の許可（draft-15 Section 6.13）
5. `WtStream` の不正状態遷移エラー（`DataSent` 後の `send_data`、`DataRecvd` 後の `recv_data`、`ResetRecvd` 後の `recv_data` 等。なお `SizeKnown` 後の `recv_data` は現行実装で許可されているため不正遷移ではない）

## 完了条件

- 上記 5 項目のテストが `tests/test_webtransport/` 配下に追加されていること
- 全テストが通過すること

## 解決方法

`tests/test_webtransport/flow_control.rs` に 2^60 境界値テストを追加する。`tests/test_webtransport/root.rs` に `overlay_settings` テスト（`WtConfig` のメソッドであるため）と Draining 状態の操作許可テストを追加する。Draining テストは Active 中にストリームを開設してから `drain()` を呼ぶ前提手順を含む。`tests/test_webtransport/stream.rs` に不正状態遷移エラーのテストを追加する。
