# SETTINGS_ENABLE_CONNECT_PROTOCOL の 1→0 ダウングレードを拒否する

- Priority: High
- Created: 2026-05-24
- Completed: 2026-05-26
- Model: Opus 4.7
- Branch: feature/fix-enable-connect-protocol-downgrade

## 目的

RFC 8441 §3 は「A sender MUST NOT send a SETTINGS_ENABLE_CONNECT_PROTOCOL parameter
with the value of 0 after previously sending a value of 1.」と定めている。
現在の `handle_settings` にはこのチェックが存在せず、リモートピアが 1→0 の
ダウングレードを送信しても受け入れてしまう。

## 優先度根拠

RFC 8441 §3 の MUST NOT 違反。Extended CONNECT セッション（WebSocket / WebTransport）
確立後にダウングレードされると、既存セッションとの不整合が発生する。

## 現状

`handle_settings` 内の SETTINGS ループに `NoRfc7540Priorities` に対する
初回値変更禁止チェックが実装されているが、`EnableConnectProtocol` には
同等のチェックがない。

## RFC 根拠

- RFC 8441 §3: 送信側の MUST NOT（1 を送信した後に 0 を送信してはならない）
- RFC 9113 §5.4.1: 受信側のエラー処理根拠。RFC 8441 §3 自体は受信側のエラーコードを
  規定していないが、拡張仕様の MUST NOT 違反は RFC 9113 §5.4.1 の汎用エラーコード
  PROTOCOL_ERROR に該当する
- RFC 9113 §6.5: SETTINGS の基本モデルは「各パラメータが既存値を置き換える」であり、
  受信側は現在値以外の状態を保持する必要がないとされている。しかし RFC 8441 §3 の
  MUST NOT 要件を検出するには「過去に true を受信したか」の追加状態が不可避であり、
  RFC 8441 §3 がこの基本モデルの例外として追加状態を要求する
- `NoRfc7540Priorities` (RFC 9218 §2.1) は「初回値から変更禁止」だが、
  `ENABLE_CONNECT_PROTOCOL` は `0→1` が許可され `1→0` のみ禁止という非対称な制約。
  この違いは RFC 8441 §3 と RFC 9218 §2.1 の規定の差異に由来する

## 設計方針

`Connection` に `peer_sent_enable_connect_protocol: bool` フィールドを追加する
（初期値 `false`、`Connection::new()` で初期化）。

`handle_settings` のループ内で `self.remote_settings.apply(*setting)` の **前に**
以下のチェックを実施する（`NoRfc7540Priorities` のチェックと同じ位置）:

1. `Setting::EnableConnectProtocol(true)` を受信 → `peer_sent_enable_connect_protocol = true`
2. `Setting::EnableConnectProtocol(false)` を受信かつ `peer_sent_enable_connect_protocol == true`
   → PROTOCOL_ERROR（接続エラー）。エラーメッセージ:
   `"SETTINGS_ENABLE_CONNECT_PROTOCOL cannot be set to 0 after sending 1"`

同一 SETTINGS フレーム内に `EnableConnectProtocol(true)` と `EnableConnectProtocol(false)` が
両方含まれる場合は、ループの出現順に従い処理する（true を先に処理した後に false が来れば
PROTOCOL_ERROR となる）。RFC 9113 §6.5 は「the value of a SETTINGS parameter is the last
value that is seen by a receiver」と規定しており、同一パラメータの重複出現を暗黙的に許容している。
`FrameDecoder` は 6 バイトチャンクを順次 `Setting::from_wire` して `Vec` に push するため、
重複があっても出現順どおりに `handle_settings` のループに渡される。

`0→1` は正当な変更であり拒否しない。`0→0`、`1→1` もエラーにしない。

`NoRfc7540Priorities` にある「最初の SETTINGS に含まれなかった場合デフォルト値で確定」する
ロジックは `EnableConnectProtocol` には不要。RFC 8441 §3 は最初の SETTINGS での送信を
強制しておらず、任意のタイミングでの 0→1 有効化が許可されるため。

ローカル送信側のチェックは不要。`Settings::to_settings_list()` は
`enable_connect_protocol == true` の場合のみ `EnableConnectProtocol(true)` を出力し、
`false` 時は設定自体を含めないため、API 経由で 1→0 の送信は構造的に不可能。

## 影響範囲

- `src/connection/mod.rs`: `Connection` 構造体にフィールド追加、`handle_settings` にチェック追加
- `pbt/tests/prop_connection.rs`: ダウングレード拒否の PBT 追加
- `CHANGES.md`: `[FIX]` エントリ追記

## 完了条件

- リモートが `ENABLE_CONNECT_PROTOCOL = 1` 送信後に `0` を送信した場合、
  PROTOCOL_ERROR として接続エラーが返される
- リモートが `ENABLE_CONNECT_PROTOCOL = 0` 送信後に `1` を送信した場合は受け入れる
- `0→0`（false の再送）はエラーにならない
- `1→1`（true の再送）はエラーにならない
- `EnableConnectProtocol` を含まない SETTINGS を途中で受信しても追跡フラグは維持される
- 既存テストが通る
- PBT で以下のプロパティを検証する:
  - 任意の SETTINGS シーケンスで `true→false` が出現した場合にのみ PROTOCOL_ERROR が返る
  - `true→false` が出現しないシーケンス（`false→false`、`false→true`、`true→true`）ではエラーにならない
  - 複数 SETTINGS フレームにまたがるケースでも正しく検出される
  - 他の SETTINGS 種別（HeaderTableSize、MaxFrameSize 等）が混在するシーケンスでもプロパティが成立する

## 解決方法

`Connection` 構造体に `peer_sent_enable_connect_protocol: bool` フィールドを追加し、`handle_settings` のループ内で `EnableConnectProtocol(true)` 受信時にフラグを `true` に設定、`EnableConnectProtocol(false)` 受信時にフラグが `true` であれば PROTOCOL_ERROR を返すようにした。

### 変更ファイル

- `src/connection/mod.rs`: `Connection` にフィールド追加、`handle_settings` にダウングレードチェック追加
- `pbt/tests/prop_connection/settings.rs`: PBT 5 テスト追加
  - `prop_enable_connect_protocol_sequence`: 任意シーケンスで true→false のみエラー（クライアントロール）
  - `prop_enable_connect_protocol_sequence_server`: 同上（サーバーロール）
  - `prop_enable_connect_protocol_tracking_across_frames`: 異種 SETTINGS 混在でもフラグ維持
  - `prop_enable_connect_protocol_intra_frame_duplicate`: 同一フレーム内重複の出現順処理
- `CHANGES.md`: `[FIX]` エントリ追記
