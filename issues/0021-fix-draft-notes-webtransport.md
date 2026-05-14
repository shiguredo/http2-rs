# WebTransport draft 注記を充実させる

Created: 2026-05-14
Model: deepseek-v4-pro

## 根拠

CLAUDE.md: 「draft 由来の機能を実装する場合は、根拠資料名、節番号、将来変更される可能性があることをコードコメントで明記すること」

## 対象と内容

### 1. `src/settings.rs:44-66` — WebTransport SETTINGS 定数群に注記が欠落

以下の定数は draft-ietf-webtrans-http2-14 由来の暫定 IANA 値だが、「暫定値であり将来変更される可能性がある」の注記がない:

- `SETTINGS_WT_INITIAL_MAX_DATA` (0x2b61)
- `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI` (0x2b62)
- `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL` (0x2b63)
- `SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE` (0x2b66)
- `SETTINGS_WT_INITIAL_MAX_STREAMS_UNI` (0x2b64)
- `SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI` (0x2b65)

一方、`src/error.rs:49,56,63` の WebTransport エラーコードには同様の注記が存在しており、不整合がある。

### 2. `src/webtransport/` — サブモジュール全体に注記が不足

- `src/webtransport/mod.rs` — モジュールヘッダーに draft であることの言及はあるが、「将来変更される可能性がある」の注記がない
- `src/webtransport/flow_control.rs` — `update_send_max` などの暫定仕様に依存する挙動に注記がない
- `src/webtransport/stream.rs` — `update_send_max` などに注記がない
- `src/webtransport/capsule.rs` — capsule タイプのエンコード/デコードに注記がない
- `src/webtransport/error.rs` — エラーコードに注記はあるが、他の場所にも必要

## 修正方針

1. `src/settings.rs` の各 WT SETTINGS 定数に `draft-ietf-webtrans-http2-14 Section 11.2` の参照と「注: この値は暫定値。IANA 登録後に更新される可能性がある。」の注記を追加する
2. `src/webtransport/mod.rs` のモジュールヘッダーに draft 由来であることと将来変更される可能性があることを明記する
3. 各サブモジュールの該当メソッドに節番号と注記を追加する
