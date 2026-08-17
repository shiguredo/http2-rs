# WtConfig のフィールドを private 化する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-privatize-wt-config
- Polished: 2026-08-17

## 目的

`WtConfig` の全フィールドを private 化し、`Settings` や `Limits` と同様に getter/builder パターンに統一する。あわせて、`WtConfig` 経由の直接構築による 2^60 超過のストリーム数注入（0116 で検出）をビルダー側の検証で防ぐ。

## 現状

`WtConfig`（`src/webtransport.rs` の `WtConfig` 型）は全 6 フィールド（`initial_max_data`, `initial_max_stream_data_bidi_local`, `initial_max_stream_data_bidi_remote`, `initial_max_stream_data_uni`, `initial_max_streams_bidi`, `initial_max_streams_uni`）が `pub` であり、構築後に個別フィールドを外部から変更可能。

`Settings` や `Limits` は既に private フィールド + getter に移行済み（`CHANGES.md` の issue 0043, 0028 参照）。`WtConfig` はこの移行から取り残されている。

また、`initial_max_streams_bidi` / `initial_max_streams_uni` は `u64` のため、`WtConfig` を手動構築した場合に 2^60 超過の値を注入できる。2^60 超過の注入経路は構造体リテラルとフィールド代入の 2 つのみで、いずれも private 化により構造的に閉じる。`Settings` 経由の WT 系パラメータは `u32`（最大 2^32-1）で制約され、`WtInit` のキーは `u` / `bl` / `br` のみでストリーム数を含まず（`SF_INTEGER_MAX` = 10^15-1 < 2^60）、`overlay_settings` / `apply_init` / `apply_init_as_peer` も 2^60 未満の値のみをマージするため、直接構築以外の経路は存在しない。

draft-ietf-webtrans-http2-15 Section 6.7 / 6.10 は Maximum Streams の上限を 2^60 と定めており（"This value cannot exceed 2^60"）、Section 4.3.1 は初期ストリーム数を "WT_MAX_STREAMS via SETTINGS_WT_INITIAL_MAX_STREAMS_UNI and SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI" として定義しているため、初期値にも 2^60 上限が及ぶ（Section 6.7 末尾の "Initial values for these limits MAY be communicated by sending non-zero values for SETTINGS_WT_INITIAL_MAX_STREAMS_UNI and SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI" も同旨）。

## 設計方針

- 全フィールドを private 化する
- 各フィールドの getter を追加する（`initial_max_data()` 等）
- `WtConfig::builder()` で生成する `WtConfigBuilder` を新設する。`LimitsBuilder` と同様に現在の `WtConfig::default()` 値（`initial_max_data`: 1_048_576 等）を初期値とし、全 6 フィールドの setter を備える。個別 setter では値範囲検査を行わず、複合制約検査を `build() -> Result<WtConfig, WtConfigError>` に集約する
- `initial_max_streams_bidi` / `initial_max_streams_uni` の 2^60 上限検証（`MAX_STREAMS_LIMIT` = `1 << 60` 超過で `WtConfigError::MaxStreamsExceedsLimit`）を `build()` に組み込む。`initial_max_data` 系 4 フィールドの varint 上限（2^62-1）検証はスコープ外とし、既存の実行時チェック（`WtFlowControl::add_recv_max` 等）に委ねる
- `MAX_STREAMS_LIMIT` は現在 `src/webtransport/flow_control.rs` の private const のため、`pub(crate)` 化して共有する（二重定義しない）
- エラー型は `LimitsError`（`src/limits.rs`）に倣い、`WtConfigError` enum（`MaxStreamsExceedsLimit` バリアント）を追加し、`Display` / `std::error::Error` を実装する。`WtConfigBuilder` と合わせて `webtransport` モジュール内で定義し、`src/lib.rs` で re-export する
- `Default` 実装は維持する（`Limits` と同様に `WtConfig::builder().build().expect(...)` 経由に統一する）。`WtConfig::default()` は 80 箇所以上で使用され（pbt / tests / tokio-http2 / examples）、`WtSession::client` / `server` が `WtConfig` を値で受ける設計のため、維持が必須
- `WtFlowControl::new()` は公開 API のまま残り、外部から直接 2^60 超過の状態を作れるが、本 issue のスコープ外とする（0116 の統合判断）
- `examples/wt_server` が `WtConfig::default()` のみを使用しているため、サンプルコードへの影響はない
- 本 issue は 0116（`WtFlowControl::new()` の上限チェック追加）を統合する。`WtFlowControl::new()` のシグネチャを Result 化する大規模な破壊的変更は行わず、`WtConfig` のビルダー側で検証する。0116 の完了条件（`WtFlowControl::new()` で上限超過時にエラー、上限チェックの単体テスト）は、本 issue の「`build()` で 2^60 超過時にエラー」「境界値テスト」で置き換えられる
- ビルダーと getter は公開 API の追加のため、`skills/shiguredo-http2/SKILL.md` の WtConfig 節に追記する
- `prop_limits_getter_roundtrip`（`pbt/tests/prop_limits.rs`）の前例に倣い、`WtConfig` の 6 つの getter のラウンドトリップ PBT を `pbt/tests/prop_webtransport/` に追加する

## 完了条件

- `WtConfig` の全フィールドが private 化されていること
- getter メソッドが追加されていること
- `WtConfigBuilder` と `build() -> Result<WtConfig, WtConfigError>` が追加され、`initial_max_streams_bidi` / `initial_max_streams_uni` の 2^60 超過で `WtConfigError::MaxStreamsExceedsLimit` を返すこと
- `Default` 実装がビルダー経由で維持されていること
- 2^60 ちょうどは成功し、2^60 + 1 はエラーになること（境界値テストを `tests/test_webtransport/` 配下に追加。0116 の完了条件を引き継ぐ）
- `tests/test_webtransport/integration.rs` の構造体リテラル（10 箇所）がビルダー経由に更新されていること
- フィールド直接アクセスが getter 経由に更新されていること（`tests/test_webtransport/integration.rs`、`tests/test_webtransport/init.rs`、`tests/test_webtransport/root.rs`、`crates/tokio-http2/src/webtransport.rs`）
- `WtConfigBuilder` / `WtConfigError` が `src/lib.rs` で re-export されていること
- `skills/shiguredo-http2/SKILL.md` の WtConfig 節にビルダーと getter が追記されていること
- `WtConfig` の getter ラウンドトリップ PBT が `pbt/tests/prop_webtransport/` に追加されていること
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリ（private 化 + getter、ビルダー + 2^60 上限検証の 2 件）と担当者行が追加されていること
- `cargo fmt --all -- --check` が通過すること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
- `cargo check --manifest-path fuzz/Cargo.toml` が通過すること
