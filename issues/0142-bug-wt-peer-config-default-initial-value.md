# ピアが SETTINGS_WT_INITIAL_MAX_* を広告しない場合に WtConfig::default の値が使われる

- Created: 2026-09-09
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-peer-config-default-initial-value
- Polished: {YYYY-MM-DD}

## 目的

WebTransport セッション確立時に、ピアが `SETTINGS_WT_INITIAL_MAX_*` を広告しない場合、ピア用 `WtConfig` が `WtConfig::default()` の 256KiB 等を初期値として保持し、draft-ietf-webtrans-http2-15 Section 11.2 が定める Initial Value 0 と乖離する問題を修正する。仕様上、未広告の値は 0 として扱うべきであり、この状態で `apply_init_as_peer` の max マージを行うと、ヘッダー値が 256KiB 未満の場合に採用されない。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::accept` は、ピア用 config を `WtConfig::default()` から構築してから `overlay_settings(conn.remote_settings())` を呼ぶ。`overlay_settings` は `Some` の値のみ上書きし、`None` (未広告) は既存値を維持する (`src/webtransport.rs` の `WtConfig::overlay_settings`)。

`WtConfig::default()` は各 `initial_max_stream_data_*` に 262144 を設定するため、ピアが SETTINGS で広告しない項目は 256KiB のまま残る。draft-ietf-webtrans-http2-15 Section 11.2 の各 SETTINGS の Initial Value は 0 であり、未広告時は 0 として扱うべきである。

## 設計方針

- ピア用 `WtConfig` の初期値を仕様の Initial Value (0) にする。`WtConfig::default()` をローカル広告値の既定として残す場合は、ピア用に 0 初期化する別経路 (例: `WtConfig::peer_default()` 相当) を設ける
- `apply_init_as_peer` の max マージは 0 を基準に動作させる
- ピアが SETTINGS を広告しない場合に、ヘッダー値がそのまま採用されることを検証するテストを追加する

## 完了条件

- ピアが `SETTINGS_WT_INITIAL_MAX_*` を広告しない場合、ピア用 config の初期値が 0 になること
- その状態で `apply_init_as_peer` のヘッダー値が max マージで採用されること
- テストが追加され、`cargo test --all` が通過すること
