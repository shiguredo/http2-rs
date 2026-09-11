# ピアが SETTINGS_WT_INITIAL_MAX_* を広告しない場合に WtConfig::default の値が使われる

- Created: 2026-09-09
- Completed: 2026-09-12
- Branch: feature/fix-wt-peer-config-default-initial-value
- Polished: 2026-09-10

## 目的

WebTransport セッション確立時に、ピアが `SETTINGS_WT_INITIAL_MAX_*` を広告しない場合、ピア用 `WtConfig` が `WtConfig::default()` の 256KiB 等を初期値として保持し、draft-ietf-webtrans-http2-15 Section 11.2 が定める Initial Value 0 と乖離する問題を修正する。仕様上、未広告の値は 0 として扱うべきであり、この状態で `apply_init_as_peer` の max マージを行うと、ヘッダー値が 256KiB 未満の場合に採用されない。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept` は、ピア用 config を `WtConfig::default()` から構築してから `overlay_settings(conn.remote_settings())` を呼ぶ。`overlay_settings` は `Some` の値のみ上書きし、`None` (未広告) は既存値を維持する (`src/webtransport.rs` の `WtConfig::overlay_settings`)。

`WtConfig::default()` は各 `initial_max_stream_data_*` に 262144、`initial_max_data` に 1 MiB、`initial_max_streams_bidi` / `initial_max_streams_uni` に 100 を設定するため、ピアが SETTINGS で広告しない項目はこれらの既定値のまま残る。draft-ietf-webtrans-http2-15 Section 11.2 の各 `SETTINGS_WT_INITIAL_MAX_*` (データ系 4 項目とストリーム数系 2 項目) の Initial Value はすべて 0 であり、未広告時は 0 として扱うべきである。

ピア用 config の `initial_max_data` と `initial_max_streams_*` は `WtSession::new` で `WtFlowControl` の `send_max` と `max_streams_*_remote` に渡されるため、0 にしないとピアが未許可のセッション送信・ストリーム開設をローカルが行えてしまう。

## 設計方針

- ピア用 `WtConfig` の初期値を、`SETTINGS_WT_INITIAL_MAX_*` に対応する全 6 フィールド (`initial_max_data`、`initial_max_stream_data_bidi_local` / `bidi_remote` / `uni`、`initial_max_streams_bidi` / `uni`) について仕様の Initial Value (0) にする。`WtConfig::default()` をローカル広告値の既定として残す場合は、ピア用に 0 初期化する別経路 (例: `WtConfig::peer_default()` 相当) を設ける
- `apply_init_as_peer` の max マージは 0 を基準に動作させる
- ピアが SETTINGS を広告しない場合に、ヘッダー値がそのまま採用されることを検証するテストを追加する
- ピア用初期値を 0 にすると、`SETTINGS_WT_INITIAL_MAX_*` を広告しないクライアントで接続する既存の tokio-http2 テスト (`Limits::default()` を使い、サーバー側で `open_uni` / `open_bidi` / データ送信を行うテスト) が失敗する。これらのテストクライアントが `Limits::builder().enable_connect_protocol(true).wt_enabled(true).webtransport(...)` で SETTINGS を広告するか、WT_MAX_* capsule を送るよう改修する

## 完了条件

- ピアが `SETTINGS_WT_INITIAL_MAX_*` を広告しない場合、ピア用 config の初期値が 0 になること
- その状態で `apply_init_as_peer` のヘッダー値が max マージで採用されること
- テストが追加され、`cargo test --all` が通過すること

## 解決方法

- `src/webtransport.rs` の `WtConfig` に `peer_default()` (SETTINGS_WT_INITIAL_MAX_* の Initial Value 全て 0) を追加し、`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept` がピア用 config を `peer_default()` から構築して `overlay_settings` / `apply_init_as_peer` でピア広告値を反映するようにした (draft-ietf-webtrans-http2-15 Section 4.3 / Section 4.3.1 / Section 11.2)
- ピア用 config の初期値が 0 になったことで、セッション送信ウィンドウ枯渇時に `send_stream_data` がストリームの送信済みバイト数・送信状態を更新してから失敗する問題が到達可能になったため、両方の送信上限を状態変更前に検査するよう修正した (Section 6.2 / Section 6.5 / Section 6.6)
- `tests/test_webtransport/init.rs` に `peer_default` の全フィールド 0・`apply_init_as_peer` のヘッダー値採用・0 初期値での送信上限・拒否時の部分状態更新なしを検証するテストを追加した
- `crates/tokio-http2/tests/test_webtransport.rs` に、SETTINGS を広告しないクライアントが WebTransport-Init で通知した小さい `bl` が上限として採用されることを検証する統合テストを追加し、サーバーが送信する既存テスト 3 件のクライアントを WT SETTINGS を広告する Limits に修正した
- `skills/shiguredo-http2/SKILL.md` の `WtConfig` API と accept の説明を更新し、`CHANGES.md` の `## develop` に `[ADD]` と `[FIX]` のエントリを追加した
