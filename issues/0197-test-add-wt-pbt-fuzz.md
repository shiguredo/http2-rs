# WebTransport セッションとヘッダーパーサーの PBT / fuzz を追加する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/add-wt-pbt-fuzz
- Polished: {YYYY-MM-DD}

## 目的

現在の WT 系 fuzz は `WtFlowControl` と `WtInit::parse`、capsule の encoder/decoder に限られ、`WtSession::feed` / `process` / `poll_event` を任意の capsule 列で動かす target がない。`WtAvailableProtocols::parse` と `serialize_wt_protocol` の fuzz / PBT もない。`fuzz_capsule_encoder.rs` は `clamp_varint` で不正値を避けており、公開エンコーダの panic 経路を検証できない。WT の入力処理とエンコーダの境界を PBT / fuzz で補強する。

## 現状

- `fuzz/fuzz_targets/fuzz_wt_flow_control.rs` は `WtFlowControl` の操作列のみ。
- `fuzz/fuzz_targets/fuzz_wt_init.rs` は `WtInit::parse` のみ。
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` は `clamp_varint` で値を丸め、WT_CLOSE_SESSION の reason 長も制限している。
- `pbt/tests/prop_webtransport/*` は capsule roundtrip、フロー制御、セッション状態、stream ID を扱うが、`WtSession` の capsule 列処理と `WtAvailableProtocols` は対象外。
- `WtAvailableProtocols::parse` / `serialize_wt_protocol` は手書きの RFC 9651 パーサーで、単体テストのみ。

## 設計方針

- `WtSession` に任意の capsule 列を feed / process させる fuzz target を追加し、panic と無限ループを検証する。
- `WtAvailableProtocols::parse` の fuzz target を追加する。
- `serialize_wt_protocol` の出力を `WtAvailableProtocols::parse` が読めることの PBT を追加する。
- `fuzz_capsule_encoder.rs` の `clamp_varint` を外し、値域境界とエラー値域を分けて検証する。
- 追加した target を `fuzz/Cargo.toml` と CI の対象に含める。

## 完了条件

- `WtSession` の capsule 列 fuzz target が追加されていること。
- `WtAvailableProtocols` の fuzz / PBT が追加されていること。
- capsule encoder の不正値域が fuzz で検証されていること。
- 追加 target が `cargo check --manifest-path fuzz/Cargo.toml` を通ること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
