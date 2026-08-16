# WtConfig のフィールドを private 化する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-privatize-wt-config
- Polished: {YYYY-MM-DD}

## 目的

`WtConfig` の全フィールドを private 化し、`Settings` や `Limits` と同様に getter/builder パターンに統一する。あわせて、`WtConfig` 経由の直接構築による 2^60 超過のストリーム数注入（0116 で検出）をビルダー側の検証で防ぐ。

## 現状

`WtConfig`（`src/webtransport.rs` の `WtConfig` 型）は全 6 フィールド（`initial_max_data`, `initial_max_stream_data_bidi_local`, `initial_max_stream_data_bidi_remote`, `initial_max_stream_data_uni`, `initial_max_streams_bidi`, `initial_max_streams_uni`）が `pub` であり、構築後に個別フィールドを外部から変更可能。

`Settings` や `Limits` は既に private フィールド + getter に移行済み（`CHANGES.md` の issue 0043, 0028 参照）。`WtConfig` はこの移行から取り残されている。

また、`initial_max_streams_bidi` / `initial_max_streams_uni` は `u64` のため、`WtConfig` を手動構築した場合に 2^60 超過の値を注入できる。`Settings` 経由の WT 系パラメータは `u32` で制約されているため現状は安全だが、`WtConfig` の直接構築では防御的検証がない。draft-ietf-webtrans-http2-15 Section 6.7 / 6.10 は Maximum Streams の上限を 2^60 と定めており（"This value cannot exceed 2^60"）、Section 4.3.1 は初期ストリーム数を "WT_MAX_STREAMS via SETTINGS_WT_INITIAL_MAX_STREAMS_UNI and SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI" として定義しているため、初期値にも 2^60 上限が及ぶ。

## 設計方針

- 全フィールドを private 化する
- 各フィールドの getter を追加する（`initial_max_data()` 等）
- ビルダー（`WtConfigBuilder`）を新設し、`initial_max_streams_bidi` / `initial_max_streams_uni` の 2^60 上限検証（`MAX_STREAMS_LIMIT` = `1 << 60` 超過でエラー）を組み込む
- `Default` 実装は維持する（`WtConfig::default()` はビルダー経由に統一する）
- 上限超過のエラー型は `LimitsError` に倣った専用エラー型を追加する（例: `WtConfigError::MaxStreamsExceedsLimit`）
- `examples/wt_server` が `WtConfig::default()` のみを使用しているため、サンプルコードへの影響はない
- 本 issue は 0116（`WtFlowControl::new()` の上限チェック追加）を統合する。`WtFlowControl::new()` のシグネチャを Result 化する大規模な破壊的変更は行わず、`WtConfig` のビルダー側で検証する

## 完了条件

- `WtConfig` の全フィールドが private 化されていること
- getter メソッドが追加されていること
- ビルダーが追加され、`initial_max_streams_bidi` / `initial_max_streams_uni` の 2^60 超過でエラーを返すこと
- 2^60 ちょうどは成功し、2^60 + 1 はエラーになること（境界値テストを `tests/test_webtransport/` 配下に追加）
- 既存の `WtConfig::default()` 使用箇所がビルダー経由に更新されていること
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリと担当者行が追加されていること
- `cargo fmt --all -- --check` が通過すること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
- `cargo check --manifest-path fuzz/Cargo.toml` が通過すること
