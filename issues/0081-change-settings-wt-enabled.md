# SETTINGS_WT_ENABLED を実装し WebTransport サポート合図を draft-15 に合わせる

- Priority: High
- Created: 2026-07-20
- Polished: 2026-07-20
- Model: Grok 4.5
- Branch: feature/change-settings-wt-enabled

## 目的

draft-ietf-webtrans-http2-15 Section 3.1 / Section 11.2 で新設された `SETTINGS_WT_ENABLED` (0x2b60) を実装し、サーバーの WebTransport サポート合図とクライアント側の CONNECT 開始条件を仕様に合わせる。draft-14 では `SETTINGS_WT_MAX_SESSIONS > 0` でサポートを示していたが、draft-15 では `SETTINGS_WT_ENABLED` に置き換わった。未対応のままでは draft-15 ピアと相互運用できない。

## 優先度根拠

- draft-15 Section 3.1: サーバーは `SETTINGS_WT_ENABLED=1` でサポートを示す。クライアントは当該 SETTINGS 受信まで `webtransport` CONNECT を試みてはならない (MUST NOT)
- クライアントは値 `>1` を接続エラー `PROTOCOL_ERROR` として扱わなければならない (MUST)
- wire 上のサポート合図が変わるため、draft-15 ピアとの相互運用に直結する High 優先度

## 現状

- `src/settings.rs` の `Setting` に `WtEnabled` バリアントはない。`0x2b60` は `Setting::Unknown` として MUST ignore される
- `Settings::to_settings_list` (`src/settings.rs`) は WT 有効時でも `SETTINGS_ENABLE_CONNECT_PROTOCOL` と `SETTINGS_WT_INITIAL_*` (0x2b61–0x2b66) のみを送る
- `src/limits.rs` に `wt_enabled` 相当のフィールドはない
- `src/connection/mod.rs` はピアの `SETTINGS_ENABLE_CONNECT_PROTOCOL` をゲートするが、`SETTINGS_WT_ENABLED` の受信状態は持たない
- draft-14 の `SETTINGS_WT_MAX_SESSIONS` は本リポジトリでは実装されていない（削除作業不要）

## 設計方針

### Setting / Settings / SettingError

- `Setting::WtEnabled(bool)` を追加し、wire ID `0x2b60` を `from_wire` / `as_wire` で扱う。初期値 / 未送信は仕様どおり「0 = 非サポート」
- 値は 0 または 1 のみ。`>1` は `SettingError::WtEnabledNotBoolean { value: u32 }` → 接続エラー `PROTOCOL_ERROR`（既存の `EnableConnectProtocolNotBoolean` パターンに揃える）
- `Settings` に `wt_enabled: bool` フィールドを追加し、`apply()` に `Setting::WtEnabled` のマッチアーム、`from_limits()` に `limits.wt_enabled()` のマッピング、`to_settings_list()` に出力条件を追加する
- `to_settings_list` では `enable_connect_protocol` と同様に `wt_enabled == true` のときのみ `Setting::WtEnabled(true)` を含める（`false` = デフォルト 0 は送信しない）。出力順序は `NoRfc7540Priorities` (0x09) の後、`wt_initial_max_*` (0x2b61-) の前（wire ID 昇順 0x09 → 0x2b60 → 0x2b61... に従う）
- `Setting` enum は `#[non_exhaustive]` ではないため、バリアント追加は下流の exhaustive match を破壊する。CHANGES.md に `[CHANGE]` として記載する

### Limits / LimitsBuilder

- `Limits` / `LimitsBuilder` に `wt_enabled: bool` を追加する
- `LimitsBuilder::build()` / `build_static()` の整合性チェックを二段構えにする:
  - `wt_enabled=true` + `enable_connect_protocol=false` → `LimitsError::WebtransportRequiresConnectProtocol`（draft-15 Section 3.1: `SETTINGS_ENABLE_CONNECT_PROTOCOL=1` はサーバーに MUST。`SETTINGS_WT_ENABLED` は MUST ではないが、WT サーバーを構成するならサポート表明の唯一の手段であり、両方が揃って意味を持つ）
  - `wt_initial_max_*` あり + `wt_enabled=false` → `LimitsError::WebtransportRequiresWtEnabled`（新規追加）
  - `wt_initial_max_*` あり + `enable_connect_protocol=false` → `LimitsError::WebtransportRequiresConnectProtocol`（既存）
  - 複数違反が同時に成立する場合は `WebtransportRequiresConnectProtocol` を先にチェックする（既存コードの順序に揃える）
- `LimitsBuilder::webtransport()` メソッドは `wt_initial_max_*` の一括設定のみを担い、`wt_enabled` の自動設定は行わない。呼び出し側が明示的に `.wt_enabled(true)` を呼ぶ（`enable_connect_protocol(true)` と同様の明示性）

### Connection / WT CONNECT ゲート

- `peer_sent_enable_connect_protocol` のような専用フィールドは追加しない。`SETTINGS_WT_ENABLED` は 1→0 ダウングレードが許可されているため（draft-15 Section 3.1: "If a server disables WebTransport after previously accepting it, this does not affect active sessions, it only prevents the creation of new sessions"）、「一度 true になったら永久に true」のトラッキングは不要。`remote_settings.wt_enabled()` で現在値を参照する
- **`ENABLE_CONNECT_PROTOCOL` との差異**: RFC 8441 では `ENABLE_CONNECT_PROTOCOL` の 1→0 は MUST NOT であり、既存コード (`src/connection/mod.rs`) はダウングレードを PROTOCOL_ERROR で拒否している。`SETTINGS_WT_ENABLED` は 1→0 が合法であり、ダウングレード拒否ロジックは不要
- クライアントの WT CONNECT 開始条件は二重ゲート: `SETTINGS_ENABLE_CONNECT_PROTOCOL=1`（RFC 8441、既存）**かつ** `SETTINGS_WT_ENABLED=1`（draft-15 Section 3.1、新規）。`:protocol` の値が `"webtransport"` の場合に `wt_enabled` も確認する。ゲート違反時のエラーは既存の `enable_connect_protocol` チェックと同様に `Error::protocol_error`（PROTOCOL_ERROR）を返す
- サーバーがクライアントから `SETTINGS_WT_ENABLED` を受信した場合は `apply()` による `remote_settings` への格納は行うが、ゲート条件には使用しない（draft-15 Section 2: "the client does not need to send any value"。クライアント→サーバー方向のセマンティクスは定義されていない）
- SETTINGS 更新後の新規セッションのみに影響する（draft-15 Section 3.1）。既存セッションは変更しない
- 「最終 ACK 済み値」の判定は RFC 9113 Section 6.5.3 の ACK セマンティクスに従う

### セッション数上限

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

- `src/settings.rs` — `Setting::WtEnabled` バリアント追加、`from_wire` / `as_wire`、`Settings` 構造体（`wt_enabled` フィールド・`apply`・`from_limits`・`to_settings_list`）、`SettingError::WtEnabledNotBoolean`
- `src/limits.rs` — `wt_enabled` フィールドとビルダーメソッド、`LimitsError::WebtransportRequiresWtEnabled`、`build()` / `build_static()` の整合性チェック
- `src/connection/mod.rs` — `:protocol = webtransport` 時の `wt_enabled` ゲート追加
- `tests/test_settings.rs` — `wt_enabled` のデフォルト値検証、`from_wire(0x2b60, 2)` の拒否テスト
- `tests/test_limits.rs` — `wt_enabled=false` + `wt_initial_max_*` ありで `build()` が失敗するテスト
- `pbt/tests/prop_settings.rs` — `valid_setting()` への `WtEnabled` 追加、`unknown_setting_wire()` の除外リストに `0x2b60` 追加、wire roundtrip
- `pbt/tests/prop_limits.rs` — `wt_enabled` + WT 設定の整合性 PBT。既存の `prop_wt_with_connect_protocol_succeeds` / `prop_wt_without_connect_protocol_fails` は新整合性チェック導入後に `.wt_enabled(true)` の追加が必要
- `pbt/tests/prop_connection/` — `SETTINGS_WT_ENABLED` の 1→0 許可・`>1` 拒否の接続レベル PBT
- `CHANGES.md` の `## develop` に `[CHANGE]` 追記

## 完了条件

- `SETTINGS_WT_ENABLED` (0x2b60) の送受信・値検証 (`0`/`1`、`>1` → PROTOCOL_ERROR) が実装されている
- WT 有効サーバーが SETTINGS に `1` を載せる
- クライアントは ACK 済み `SETTINGS_WT_ENABLED=1` **かつ** `SETTINGS_ENABLE_CONNECT_PROTOCOL=1` なしに WT CONNECT を開始しない
- `SETTINGS_WT_ENABLED` の 1→0 ダウングレードが許可され、新規 WT CONNECT のみ拒否される（既存セッションは影響なし）
- `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` が通る
- `CHANGES.md` develop にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 3.1（SETTINGS_WT_ENABLED 定義・MUST NOT・ダウングレード許容・ENABLE_CONNECT_PROTOCOL との二重ゲート）/ Section 4.1（セッション数上限）/ Section 11.2（Code 0x2b60）
- `refs/rfc9113.txt` Section 6.5.2（SETTINGS 一般規則）/ Section 6.5.3（SETTINGS 更新と ACK セマンティクス）
- `src/settings.rs` — 既存 `Setting` / `Settings::to_settings_list` / `SettingError`
- `src/limits.rs` — WT 初期 SETTINGS と `enable_connect_protocol` の整合性
- `src/connection/mod.rs` — `peer_sent_enable_connect_protocol`
