# SETTINGS_ENABLE_CONNECT_PROTOCOL の 1→0 ダウングレードを拒否する

- Priority: High
- Created: 2026-05-24
- Model: Opus 4.7
- Branch: feature/fix-enable-connect-protocol-downgrade

## 目的

RFC 8441 §3 は「A sender MUST NOT send a SETTINGS_ENABLE_CONNECT_PROTOCOL parameter
with the value of 0 after previously sending a value of 1.」と定めている。
現在の `handle_settings` にはこのチェックが存在せず、リモートピアが 1→0 の
ダウングレードを送信しても受け入れてしまう。

## 優先度根拠

RFC 8441 の MUST NOT 違反。WebTransport セッション確立後にダウングレードされると既存セッションとの不整合が発生する。

## 現状

`handle_settings` には `NoRfc7540Priorities` に対する初回値変更禁止チェック
(1575-1585 行) が実装されているが、`EnableConnectProtocol` には同等のチェックがない。

WebTransport セッション確立後にピアが `ENABLE_CONNECT_PROTOCOL = 0` を送信すると、
既存セッションとの不整合が発生する。

## RFC 根拠

- RFC 8441 §3: MUST NOT 違反
- `NoRfc7540Priorities` のチェックとは異なり、`ENABLE_CONNECT_PROTOCOL` は
  `0→1` は許可される (初回 1 の後の 0 のみ禁止)。そのため
  `initial_no_rfc7540_priorities` と同じ「初回値から変更禁止」ロジックは使えない

## 設計方針

`Connection` に `remote_enable_connect_protocol_seen_true: bool` フィールドを追加し、
`Setting::EnableConnectProtocol(true)` を受信した時点で `true` にセットする。
以降 `Setting::EnableConnectProtocol(false)` を受信した場合に PROTOCOL_ERROR
(接続エラー) を返す。

`0→1` は正当な変更であり拒否しない。

## 影響範囲

- `src/connection/mod.rs`: `Connection` 構造体にフィールド追加、`handle_settings` にチェック追加

## 完了条件

- リモートが `ENABLE_CONNECT_PROTOCOL = 1` 送信後に `0` を送信した場合、
  PROTOCOL_ERROR として接続エラーが返される
- リモートが `ENABLE_CONNECT_PROTOCOL = 0` 送信後に `1` を送信した場合は受け入れる
- 既存テストが通る
- PBT でチェックが正しく動作することを検証する
