# ストリーム ID 枯渇時のチェックを追加する

Created: 2026-05-24
Priority: High
Model: Opus 4.7

## 概要

`Connection::start_stream` で `self.next_stream_id += 2` としているが、
`next_stream_id` が `STREAM_ID_MAX` (2^31 - 1) を超えた場合のチェックがない。
debug ビルドではオーバーフロー panic、release ビルドでは wraparound が発生し、
RFC 違反のストリーム ID が送信される。

## 根拠

- RFC 9113 §5.1.1: ストリーム ID は unsigned 31-bit integer (最大 2^31 - 1)
- `StreamId::from_wire` は `debug_assert` のみで release では通過する
- 長時間稼働する接続でストリーム ID が枯渇する可能性がある

## 設計

`start_stream` の先頭で `self.next_stream_id > STREAM_ID_MAX` をチェックし、
枯渇時は GOAWAY を送信して接続を閉じる。
新規ストリームの開始は拒否する (REFUSED_STREAM 相当)。

## 影響範囲

- `src/connection/mod.rs`: `start_stream` に枯渇チェックを追加

## 受け入れ条件

- `next_stream_id` が 31-bit 範囲を超える場合、新規ストリーム開始が拒否される
- 枯渇時に適切なエラーが返される
- 既存テストが通る
