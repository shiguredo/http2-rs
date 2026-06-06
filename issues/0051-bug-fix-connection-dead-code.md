# connection/data.rs と connection/settings.rs のデッドコードを削除する

- Priority: High
- Created: 2026-06-06
- Model: DeepSeek V4 Pro
- Polished: 2026-06-06

## 目的

`src/connection/mod.rs:21` には `mod headers;` のみ宣言されており、`mod data;` / `mod settings;` が欠落している。そのため `src/connection/data.rs` (289 行) と `src/connection/settings.rs` (203 行) の全コードがコンパイル対象外のデッドコードになっている。`CHANGES.md:133` には分割完了と記載されているが実態は未了であり、管理上の問題。

## 優先度根拠

- `settings.rs` の `handle_settings()` には RFC 8441 §3 の `SETTINGS_ENABLE_CONNECT_PROTOCOL` ダウングレード拒否チェック（1→0 MUST NOT）が欠落している（`mod.rs:1089-1100` には実装済み）
- 将来誰かが `mod data;` / `mod settings;` を追加しようとした場合、以下の理由で直ちにコンパイルエラーになる:
  - メソッド重複定義（`send_data`, `handle_data`, `initiate`, `handle_settings` 等が両ファイルで定義されている）
  - `Settings` の private フィールドへの他モジュールからの直接アクセス（全 5 箇所、後述）
  - `MaxFrameSize` newtype に対する無効な `as usize` キャスト（`data.rs:108`）
- コンパイル対象外の別実装が残っている状態は、`mod.rs` 側の実装を修正する際に `data.rs` / `settings.rs` 側の更新漏れを引き起こす二重管理リスクがある

## 現状

`mod.rs` にのみ `mod headers;` が宣言されており、全メソッド実装は `mod.rs` の `impl Connection` ブロック内に存在する。`data.rs` / `settings.rs` は同名メソッドの別実装を保持しているがコンパイル対象外。

### `data.rs` と `mod.rs` の重複メソッド

| メソッド | `mod.rs` 行 | `data.rs` 行 | 差分 |
|----------|------------|-------------|------|
| `send_data` | 569 | 14 | `data.rs:108` の `max_frame_size as usize` が不正 |
| `queue_data` | 613 | 58 | 概ね同一 |
| `flush_stream_data` | 638 | 83 | `data.rs` のみ `pub(super)`（`mod.rs` は `fn`） |
| `flush_all_stream_data` | 735 | 180 | `data.rs` のみ `pub(super)` |
| `handle_data` | 925 | 198 | `data.rs` のみ `pub(super)` |

### `settings.rs` と `mod.rs` の重複メソッド

| メソッド | `mod.rs` 行 | `settings.rs` 行 | 差分 |
|----------|------------|-----------------|------|
| `initiate` | 263 | 16 | 概ね同一 |
| `send_settings` | 352 | 53 | 概ね同一 |
| `send_initial_connection_window_update` | 375 | 76 | 概ね同一 |
| `handle_settings` | 1042 | 90 | **RFC 8441 §3 ダウングレードチェック欠落** |
| `update_stream_windows` | 1160 | 195 | 概ね同一 |

### `settings.rs` の private フィールド直接アクセス（全 5 箇所）

いずれも `crate::connection::settings` モジュールから `crate::settings::Settings` の private フィールドにアクセスしておりコンパイル不可:

| `settings.rs` 行 | アクセス | `mod.rs` での正しいアクセス |
|------------------|---------|--------------------------|
| 109 | `self.remote_settings.initial_window_size` | `.initial_window_size().get()` (line 1061) |
| 112 | `self.remote_settings.header_table_size` | `.header_table_size()` (line 1064) |
| 152 | `self.remote_settings.header_table_size` | `.header_table_size()` (line 1117) |
| 162 | `self.remote_settings.initial_window_size` | `.initial_window_size().get()` (line 1127) |
| 169 | `self.remote_settings.header_table_size` | `.header_table_size()` (line 1133-1134) |

`data.rs:108` の `self.remote_settings.max_frame_size` も同様の private フィールドアクセスだが、加えて `MaxFrameSize` newtype への無効な `as usize` キャストも含むため、上記表では分離して扱う（詳細は重複メソッド表の `send_data` 行を参照）。

### issue 0015 の状況

issue 0015 (`issues/closed/0015-refactor-split-connection-module.md`) は「分割完了」としてクローズされているが、実際には `headers` のみ分割され `data` / `settings` は未了。本 issue は 0015 の未了部分を**削除で決着**させる。

### PBT ファイルへの影響

`pbt/tests/prop_connection/data.rs` / `pbt/tests/prop_connection/settings.rs` はコンパイル対象の `mod.rs` 内実装に対する PBT であり、削除対象の `src/connection/data.rs` / `src/connection/settings.rs`（デッドコードの別実装）とは無関係。これらの PBT ファイルは修正不要。

## 設計方針

削除案を採用する。分割案は以下が必要で修正量が大きく、移行の価値に見合わない:

- `settings.rs` 5 箇所の private フィールドアクセス修正
- `data.rs:108` の `max_frame_size as usize` 修正
- `settings.rs` への RFC 8441 §3 ダウングレード拒否チェック移植
- `mod.rs` から重複メソッド削除
- 可視性不一致（`pub(super)` vs `fn`）の調整

## 対応手順

1. 作業ブランチ `feature/fix-remove-connection-dead-code` を作成する
2. `src/connection/data.rs` と `src/connection/settings.rs` を削除する
3. `CHANGES.md:132-133` の該当エントリ（2 行）を修正する。PBT 分割は完了しているため `pbt/tests/prop_connection.rs` の記述は独立した `### misc` エントリに分離する:
   - 修正前:
     ```
     - [UPDATE] `src/connection/mod.rs` を headers / settings / data サブモジュールに分割し、
       `pbt/tests/prop_connection.rs` をディレクトリモジュール形式に分割する (issue 0015)
     ```
   - 修正後:
     ```
     - [UPDATE] `src/connection/mod.rs` のヘッダー関連処理を headers サブモジュールに分割する (issue 0015)
     - [UPDATE] `pbt/tests/prop_connection.rs` をディレクトリモジュール形式に分割する (issue 0015)
     ```
4. `cargo check --workspace --all-targets` で、削除対象ファイルが元々コンパイル対象外であることの裏付けとして、削除前後でコンパイル結果が同一であることを確認する
5. `cargo test --workspace` で全テスト通過を確認する
6. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する

## 完了条件

- `src/connection/data.rs` と `src/connection/settings.rs` が削除されている
- `CHANGES.md` の記述が実態と一致している
- `cargo check --workspace --all-targets` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
