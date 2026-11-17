# WebTransport セッション・ヘッダーパーサー・capsule エンコーダーの PBT / fuzz を追加する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/add-wt-pbt-fuzz
- Polished: 2026-11-17

## 目的

現在の WT 系 fuzz は `WtFlowControl` と `WtInit::parse`、capsule の encoder/decoder に限られ、`WtSession::feed` / `process` / `poll_event` を任意の capsule 列で動かす target がない。`WtAvailableProtocols::parse` と `serialize_wt_protocol` の fuzz / PBT もない。`fuzz_capsule_encoder.rs` は varint フィールドを `clamp_varint` で丸め、`Capsule::Padding` の `length` を u16 に制限しているため、varint 超過の panic 経路と 16 MiB 上限の境界を検証できない。`Capsule::WtCloseSession` の reason は制限していないため、debug ビルドでは 1,024 バイト超で `debug_assert!` が panic し得る。WT の入力処理とエンコーダの境界を PBT / fuzz で補強する。

`CapsuleEncoder::encode` の `WtResult<()>` 化と値域検証は 0193 で行う。本 issue は 0193 完了後に実施する。

## 現状

- `fuzz/fuzz_targets/fuzz_wt_flow_control.rs` は `WtFlowControl` の操作列のみ。
- `fuzz/fuzz_targets/fuzz_wt_init.rs` は `WtInit::parse` のみ。
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` は varint フィールドを `clamp_varint` で丸め、`Capsule::Padding` の `length` を u16 に制限している。`Capsule::WtCloseSession` の reason 長は制限しておらず、debug ビルドでは 1,024 バイト超で `debug_assert!` が panic し得る。
- `src/webtransport/capsule.rs` の `CapsuleEncoder::encode` は現在 `WtResult` を返さず、`encode_varint` の `expect` により varint 上限超過で panic する。0193 が `WtResult<()>` 化と値域検証を追加する予定である。
- `pbt/tests/prop_webtransport/*` は capsule roundtrip、フロー制御、セッション状態、stream ID を扱うが、`WtSession` の任意 capsule 列処理と `WtAvailableProtocols` は対象外。単一 capsule の `WtSession::feed` / `process` を呼ぶテストはあるが、任意列ではない。
- `WtAvailableProtocols::parse` / `serialize_wt_protocol` は手書きの RFC 9651 実装 (List of String パーサーと sf-string シリアライザー) で、`tests/test_webtransport/protocols.rs` の単体テストのみ。PBT / fuzz はない。

## 設計方針

- 0193 の `CapsuleEncoder::encode` の `WtResult<()>` 化と値域検証が完了していることを前提とする。0193 未完了の状態で `clamp_varint` を外すと、varint 超過では panic し、reason 1,024 バイト超では debug ビルドで panic する (release ビルドでは仕様違反のままエンコードされる)。
- `WtSession` の client / server 両ロールを入力で選び、`WtConfig::default()` で構築する。`initiate()` 後に任意の capsule 列を `feed` / `process` し、`poll_event` を `None` になるまで呼ぶ fuzz target を追加する。`feed` / `process` の `Err` は許容し、panic と無限ループを検証する。
- `WtAvailableProtocols::parse` に任意のバイト列を渡す fuzz target を追加し、panic しないことを検証する。
- `serialize_wt_protocol` が受理する任意のバイト列 (0x20..=0x7E) について、出力を `WtAvailableProtocols::parse` がパースでき、`protocols` が入力値 1 件と一致することの PBT を追加する。0x20 / 0x7E / `"` / `\` / `,` / `;` を含む境界入力を生成する。
- `fuzz_capsule_encoder.rs` の `clamp_varint` と `Capsule::Padding` の u16 制限を外す。varint 値は境界値 (0 / 63 / 64 / 16,383 / 16,384 / 1,073,741,823 / 1,073,741,824 / 2^62-1) と上限超過値 (2^62 / u64::MAX) を明示的に生成する。WT_MAX_STREAMS / WT_STREAMS_BLOCKED の 2^60 境界、WT_RESET_STREAM / WT_STOP_SENDING の 0xffffffff 境界、WT_CLOSE_SESSION の reason 1,024 バイト境界、Padding のエンコード後サイズが 16 MiB 以下/超になる境界も入力に含める。
- fuzz target は panic しないことのみを検証し、`CapsuleEncoder::encode` の `Err` は許容する (値域の正しさは単体テスト / PBT の役割。既存 fuzz target の役割分担に従う)。
- `fuzz/Cargo.toml` に追加 target の `[[bin]]` を追加する。CI は既に `cargo check --manifest-path fuzz/Cargo.toml` で fuzz crate 全体を検査しているため、CI の変更は不要。
- `pbt/tests/prop_webtransport/main.rs` に `mod protocols;` を追加する。
- 変更履歴は `CODEBASE.md` の指示に従い `CHANGES.md` に残さない。

## 変更対象

- `fuzz/fuzz_targets/fuzz_wt_session.rs` (新規)
- `fuzz/fuzz_targets/fuzz_wt_protocols.rs` (新規)
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs`
- `fuzz/Cargo.toml`
- `pbt/tests/prop_webtransport/protocols.rs` (新規)
- `pbt/tests/prop_webtransport/main.rs`

## 完了条件

- `WtSession` の client / server 両ロールについて、任意の capsule 列を `feed` / `process` / `poll_event` する fuzz target が追加されていること。
- `WtAvailableProtocols::parse` の fuzz target と、`serialize_wt_protocol` の出力を `WtAvailableProtocols::parse` がパースでき、`protocols` が入力値 1 件と一致することの PBT が追加されていること。
- `fuzz_capsule_encoder.rs` から `clamp_varint` と `Capsule::Padding` の u16 制限が除かれ、varint・2^60・0xffffffff・reason 1,024 バイト・Padding 16 MiB の境界値と上限超過値が fuzz 入力に含まれ、panic しないこと。
- 追加 target が `fuzz/Cargo.toml` に登録され、`cargo check --manifest-path fuzz/Cargo.toml` を通ること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
- `CHANGES.md` が変更されていないこと (`CODEBASE.md`)。

## 参照

- `src/webtransport.rs` — `WtSession::client` / `server` / `initiate` / `feed` / `process` / `poll_event`
- `src/webtransport/protocols.rs` — `WtAvailableProtocols::parse` / `serialize_wt_protocol`
- `src/webtransport/capsule.rs` — `CapsuleEncoder::encode` / `Capsule` / `MAX_CLOSE_REASON_LEN` / `DEFAULT_MAX_BUFFER_SIZE`
- `src/webtransport/varint.rs` — `MAX_VALUE`
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` — `clamp_varint` / `Padding` の u16 制限
- `pbt/tests/prop_webtransport/main.rs` — 既存の PBT 構成
- `tests/test_webtransport/protocols.rs` — 既存の単体テスト
- `issues/0193-bug-fix-capsule-encoder-validation.md` — `CapsuleEncoder::encode` の `WtResult<()>` 化と値域検証
- `refs/rfc9651.txt` — Structured Field Values for HTTP
- `refs/draft-ietf-webtrans-http2-16.txt` — WebTransport over HTTP/2
- `fuzz/Cargo.toml` — fuzz target の登録
