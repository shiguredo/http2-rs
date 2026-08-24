# WtSession の送信系 API が varint 上限超過入力で panic する

- Created: 2026-08-24
- Completed: 2026-08-24
- Branch: feature/fix-wt-send-api-varint-panic
- Polished: 2026-08-24

## 目的

`WtSession` の送信系 API (`send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending`) が、varint でエンコードできない巨大な値を検証せずに受け取り、panic する問題を修正する。公開 API が不正入力でクラッシュすることはライブラリとして許容できない。

## 現状

`src/webtransport.rs` の以下の公開 API は、引数の範囲を検証せず `CapsuleEncoder::encode` に渡す:

- `WtSession::send_max_data` (`maximum: u64`)
- `WtSession::send_max_stream_data` (`maximum: u64`)
- `WtSession::reset_stream` (`error_code: u64`)
- `WtSession::stop_sending` (`error_code: u64`)

`CapsuleEncoder::encode_varint` (`src/webtransport/capsule.rs`) は `varint::encode(...).expect("buffer is pre-sized to encoded_len")` を呼ぶ。`varint::encode` (`src/webtransport/varint.rs` の `encode`) は `value > MAX_VALUE (2^62-1)` で `Err` を返すため、`send_max_data(1 << 62)` のような入力で `.expect()` が panic する。`varint::encoded_len` は `u64::MAX` でも 8 を返すため、事前割当の `resize` は成功し panic が確実に発生する。

対照的に `WtFlowControl::add_recv_max` (`src/webtransport/flow_control.rs`) は varint 上限 (`super::varint::MAX_VALUE`) を検証済みであり、送信系 API のみ未検証。

さらに `reset_stream` / `stop_sending` の `error_code` は、draft-ietf-webtrans-http2-15 Section 6.2 / 6.3 が「This value MUST NOT exceed 0xffffffff」と定める範囲も未検証である (受信側の `CapsuleDecoder::decode_payload` は 0xffffffff 超を拒否しているため非対称)。0xffffffff 超 2^62-1 以下の値では仕様違反の capsule を送信し、2^62-1 超では panic する。

## 設計方針

- `send_max_data` / `send_max_stream_data` に `maximum > varint::MAX_VALUE` チェックを追加し、超過時は `WtError::flow_control_error` を返す
- `reset_stream` / `stop_sending` に `error_code > 0xffffffff` チェックを追加し、超過時は `WtError::flow_control_error` を返す (draft-15 Section 6.2 / 6.3 の MUST NOT に整合)
- `send_max_streams` の既存の 2^60 チェックと同様のスタイル (`WtErrorKind::FlowControlError`) で統一する
- 各 API に境界値 (`MAX_VALUE`、`MAX_VALUE + 1`、`0xffffffff`、`0xffffffff + 1`) のテストを追加する

## 完了条件

- 上記 4 API に範囲外入力を渡しても panic しないこと
- `send_max_data` / `send_max_stream_data` が `maximum > 2^62-1` で `Err` (WtErrorKind::FlowControlError) を返すこと
- `reset_stream` / `stop_sending` が `error_code > 0xffffffff` で `Err` (WtErrorKind::FlowControlError) を返すこと
- 境界値のテストが追加され、`cargo test --all` が通過すること

## 解決方法

`src/webtransport.rs` の `WtSession` の送信系 API 4 つに引数範囲チェックを追加した。

- `send_max_data` / `send_max_stream_data` に `maximum > varint::MAX_VALUE (2^62-1)` チェックを追加し、超過時は `WtError::flow_control_error` を返す (RFC 9000 Section 16)
- `reset_stream` / `stop_sending` に `error_code > 0xffffffff` チェックを追加し、超過時は `WtError::flow_control_error` を返す (draft-ietf-webtrans-http2-15 Section 6.2 / 6.3 の MUST NOT)。このチェックが varint 上限超過による panic も包含して防ぐ
- 既存の `send_max_streams` の 2^60 チェックと同様に引数検証を関数冒頭に置き、4 API で検証順序を統一した
- `src/webtransport/capsule.rs` の `MAX_APPLICATION_ERROR_CODE` を `pub(crate)` 化し、受信側の検証と定数を共有した
- 公開 doc に新設エラー条件を追記し、エラー種別選択の根拠 (送信 API の引数検証はローカルな入力エラーであり、他の範囲チェックと `flow_control_error` で統一) をコメントに明記した

テスト:

- `tests/test_webtransport/integration.rs` に 4 テストを追加した。各 API について、上限ちょうど (成功)・上限 + 1 (FlowControlError)・varint 上限超過 (u64::MAX、panic しないこと)・32-bit 超成功 (varint 制約のみ) を検証する
- `send_max_data(MAX_VALUE)` の出力をデコードし、8 バイト varint の往復を検証する
