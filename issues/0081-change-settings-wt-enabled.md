# SETTINGS_WT_ENABLED を実装し WebTransport サポート合図を draft-15 に合わせる

- Priority: High
- Created: 2026-07-20
- Polished: {Polished}
- Model: Grok 4.5
- Branch: feature/change-settings-wt-enabled

## 目的

draft-ietf-webtrans-http2-15 Section 3.1 / Section 11.2 で新設された `SETTINGS_WT_ENABLED` (0x2b60) を実装し、サーバーの WebTransport サポート合図とクライアント側の CONNECT 開始条件を仕様に合わせる。draft-14 の `SETTINGS_WT_MAX_SESSIONS > 0` によるサポート合図は廃止されており、未対応のままでは相互運用できない。

## 優先度根拠

- draft-15 Section 3.1: サーバーは `SETTINGS_WT_ENABLED=1` でサポートを示す。クライアントは当該 SETTINGS 受信まで `webtransport` CONNECT を試みてはならない (MUST NOT)
- 値 `>1` は接続エラー `PROTOCOL_ERROR` (MUST)
- wire 上のサポート合図が変わるため、draft-15 ピアとの相互運用に直結する High 優先度

## 現状

- `src/settings.rs` の `Setting` に `WtEnabled` バリアントはない。`0x2b60` は `Setting::Unknown` として MUST ignore される
- `Settings::to_settings_list` (`src/settings.rs`) は WT 有効時でも `SETTINGS_ENABLE_CONNECT_PROTOCOL` と `SETTINGS_WT_INITIAL_*` (0x2b61–0x2b66) のみを送る
- `src/limits.rs` に `wt_enabled` 相当のフィールドはない
- `src/connection/mod.rs` はピアの `SETTINGS_ENABLE_CONNECT_PROTOCOL` をゲートするが、`SETTINGS_WT_ENABLED` の受信・ACK 状態は持たない
- draft-14 の `SETTINGS_WT_MAX_SESSIONS` 自体も本リポジトリでは実装されていない（セッション上限は HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS` に依存）

## 設計方針

- `Setting::WtEnabled(bool)` を追加し、wire ID `0x2b60` を扱う。初期値 / 未送信は仕様どおり「0 = 非サポート」
- 値は 0 または 1 のみ。`>1` は `SettingError` → 接続エラー `PROTOCOL_ERROR`
- `Limits` / `LimitsBuilder` に `wt_enabled: bool` を追加する。WT 初期 SETTINGS (`wt_initial_max_*`) を設定する場合は `enable_connect_protocol=true` と同様に `wt_enabled=true` を要求する（既存の整合性チェックに揃える）
- サーバーが WT を有効にしているとき `to_settings_list` が `SETTINGS_WT_ENABLED=1` を含める
- 接続状態にピアの `SETTINGS_WT_ENABLED`（最終 ACK 済み値）を保持し、クライアント側で WT CONNECT 開始前に `==1` を確認する
- SETTINGS 更新後の新規セッションのみに影響する（draft-15 Section 3.1）。既存セッションは変更しない
- セッション同時数上限は draft-15 Section 4.1 どおり `SETTINGS_MAX_CONCURRENT_STREAMS` 側。新規 WT SETTINGS は追加しない

## スコープ外

- WT_STREAM FIN 極性・Reliable Size・エラーコード名変更・CLOSE/Origin 挙動（0082–0085）
- ソースコメントの draft 番号機械置換（0074）
- 0065 TLS exporter / 0066 サブプロトコル交渉

## 他 issue との関係

- **0074**: コメントの draft 番号同期。本 issue 着手前または並行で揃えると参照が楽
- **0085**: Origin / CLOSE 等。本 issue の `SETTINGS_WT_ENABLED` ゲートが前提になる箇所がある
- **0065 / 0066**: 本群（0081–0085）の後が安全

推奨順: **0074 → 0081 → (0082 / 0083) → 0084 → 0085 → 0065/0066**

## 変更対象ファイル一覧

- `src/settings.rs` — `Setting::WtEnabled`、`from_wire` / `to_wire`、`Settings`、検証エラー
- `src/limits.rs` — `wt_enabled` フィールドとビルダー、整合性チェック
- `src/connection/mod.rs` — ピア SETTINGS の保持と WT CONNECT ゲート
- 関連テスト: `tests/` / `pbt/` の SETTINGS・接続系
- `CHANGES.md` の `## develop` に `[CHANGE]` 追記

## 完了条件

- `SETTINGS_WT_ENABLED` (0x2b60) の送受信・値検証 (`0`/`1`、`>1` → PROTOCOL_ERROR) が実装されている
- WT 有効サーバーが SETTINGS に `1` を載せる
- クライアントは ACK 済み `SETTINGS_WT_ENABLED=1` なしに WT CONNECT を開始しない
- `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` が通る
- `CHANGES.md` develop にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 3.1 / Section 4.1 / Section 11.2 (`SETTINGS_WT_ENABLED` Code 0x2b60)
- `src/settings.rs` — 既存 `Setting` / `Settings::to_settings_list`
- `src/limits.rs` — WT 初期 SETTINGS と `enable_connect_protocol` の整合性
- `src/connection/mod.rs` — `peer_sent_enable_connect_protocol`
- `issues/0074-change-update-refs-draft-15.md` — コメント同期（意味追従は本 issue 群）
