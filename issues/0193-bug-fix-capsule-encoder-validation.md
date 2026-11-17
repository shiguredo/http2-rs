# 公開 `CapsuleEncoder::encode` が varint 超過で panic し、仕様違反 capsule を生成できる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-capsule-encoder-validation
- Polished: 2026-11-17

## 目的

`CapsuleEncoder::encode` は `Result` を返さず、内部の `encode_varint` が `varint::encode(...).expect(...)` する。`Capsule::WtMaxData { maximum: u64::MAX }` などを渡すと panic する。また WT_CLOSE_SESSION の reason 長は `debug_assert!` のみで release では 1,024 バイト超をそのままエンコードし、WT_MAX_STREAMS / WT_STREAMS_BLOCKED の 2^60 超、WT_RESET_STREAM / WT_STOP_SENDING の 0xffffffff 超も検証しない。`Capsule::Padding` の巨大な `length` は varint 上限超過で panic し、上限以内でも `length` バイトのゼロ埋めで過大なメモリを消費する。公開 API の値域検証を追加する。

## 現状

`src/webtransport/capsule.rs` の `CapsuleEncoder::encode_varint` は `varint::encoded_len` でバッファを確保し、`varint::encode` が `Err` を返すと `expect` で panic する。`Capsule::WtMaxData { maximum: u64::MAX }` のエンコードで `varint value 18446744073709551615 exceeds maximum 4611686018427387903` の panic を確認済みである。64-bit 環境では `Capsule::Padding { length: varint::MAX_VALUE + 1 }` と `Capsule::Unknown { capsule_type: varint::MAX_VALUE + 1, .. }` も同じ経路で panic する。`CapsuleEncoder::encode` の rustdoc には `# Panics` がなく、panic 条件が読み取れない。

`Capsule::WtCloseSession` の reason 長は `MAX_CLOSE_REASON_LEN` を `debug_assert!` でしか検査しない。debug ビルドでは 1,025 バイトで panic し、release ビルドでは 1,025 バイトをそのままエンコードする。

`Capsule::WtMaxStreams { maximum }` と `Capsule::WtStreamsBlocked { maximum }` は 2^60 超を検証しない。`Capsule::WtResetStream { error_code, .. }` と `Capsule::WtStopSending { error_code, .. }` は 0xffffffff 超を検証しない。いずれも panic はしないが、受信側が WT_FLOW_CONTROL_ERROR / WT_ERROR でセッションを閉じる仕様違反の capsule を生成する。

`fuzz/fuzz_targets/fuzz_capsule_encoder.rs` は `clamp_varint` で varint フィールドの不正値を避け、`Capsule::Padding` の `length` を u16 に制限しているため、varint 超過の panic 経路と Padding の巨大長を検証していない。`WtCloseSession` の reason は clamp していないため、debug ビルドでは 1,024 バイト超で `debug_assert!` が panic し得る。

受信側は `src/webtransport/capsule.rs` の `CapsuleDecoder::decode_payload` が WT_RESET_STREAM / WT_STOP_SENDING の error code と WT_CLOSE_SESSION の reason 長を、`src/webtransport.rs` の `WtSession::handle_capsule` が WT_MAX_STREAMS / WT_STREAMS_BLOCKED の 2^60 を検証している。`WtSession` のフロー制御・エラーコードを扱う送信 API (`send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` / `send_max_streams`) も varint 上限・error code 上限・2^60 を事前に検証している。公開 `CapsuleEncoder` を直接使う利用者にはこれらの検証がない。

closed 0127 は `WtSession` の送信 API 側で varint 超過を事前拒否したが、公開 `CapsuleEncoder` 自体の panic 経路は残っている。closed 0061 は `WtSession::close` 側の reason 長を扱ったが、現在の `close` は UTF-8 境界で切り詰める方針であり、公開エンコーダを直接使う利用者には検証がない。本 issue はエンコーダの値域検証を対象とする。

## 設計方針

- `CapsuleEncoder::encode` を `WtResult<()>` に変更する。後方互換のない変更だが、完了条件の「panic しないこと」を満たすには戻り値でエラーを通知する必要があるため、`encode` を維持して `# Panics` を書く代替案は採らない。
- 値域検証は `CapsuleEncoder::encode` の冒頭で capsule 全体に対して行い、検証後に既存の書き込み処理を実行する。`Err` 時は `CapsuleEncoder` のバッファを変更しない。`encode_varint` の `expect` は、検証済みの値と事前確保済みバッファに対する不変条件として残すが、メッセージは値域検証済みであることも分かる内容にする。
- すべての varint フィールドを検証する。対象は `Capsule::Datagram` の `data.len()`、`Capsule::Padding` の `length`、`Capsule::WtResetStream` の `stream_id` / `error_code` / `reliable_size`、`Capsule::WtStopSending` の `stream_id` / `error_code`、`Capsule::WtStream` の `stream_id` と `encoded_len(stream_id) + data.len()`、`Capsule::WtMaxData` の `maximum`、`Capsule::WtMaxStreamData` の `stream_id` / `maximum`、`Capsule::WtMaxStreams` の `maximum`、`Capsule::WtDataBlocked` の `maximum`、`Capsule::WtStreamDataBlocked` の `stream_id` / `maximum`、`Capsule::WtStreamsBlocked` の `maximum`、`Capsule::Unknown` の `capsule_type` / `data.len()` である。`Capsule::WtCloseSession` の `error_code` は u32 なので値域検証は不要である。
- プロトコル上限を検証する。WT_RESET_STREAM / WT_STOP_SENDING の `error_code` は 0xffffffff 以下 (draft-ietf-webtrans-http2-16 Section 6.2 / 6.3)、WT_MAX_STREAMS / WT_STREAMS_BLOCKED の `maximum` は 2^60 以下 (同 Section 6.7 / 6.10)、WT_CLOSE_SESSION の `reason` は 1,024 バイト以下 (同 Section 6.12) とする。`reason` は切り詰めず、超過時は `Err` を返す (切り詰めは `WtSession::close` の責務)。
- `Capsule::Padding` は `length` バイトのゼロを実際に書き込むため、エンコード後サイズ (type + length + payload) が `DEFAULT_MAX_BUFFER_SIZE` (16 MiB) を超える場合は `InvalidInput` を返す。これにより、`length` が varint 上限以下でも 1 TiB などの過大なメモリ確保を防ぐ。16 MiB は `CapsuleDecoder::new()` の既定バッファ上限と同じ値であり、0182 の 1 capsule 16 MiB 上限とも整合する。`length` は `usize` なので、エンコード後サイズの計算はオーバーフローしないように行う (まず `length > DEFAULT_MAX_BUFFER_SIZE` を比較するなど)。
- 値域検証の失敗は `WtErrorKind::InvalidInput` を返す。これはローカルな入力エラーであり、受信した capsule のセッションエラー (`SessionStateError` / `FlowControlError`) とは区別する。reason には違反したフィールド名、値、上限を明記する。
- `src/webtransport.rs` の `WtSession` の既存の事前検証は維持する。`CapsuleEncoder::encode` の戻り値は内部呼び出し側で `?` により伝播するが、事前検証済みの入力では `Ok` になる。`WtSession` の既存の事前検証経路のエラー種別 `FlowControlError` と既存テストは変更しない。`src/webtransport.rs` の「CapsuleEncoder 内で panic するため」というコメントと doc は、事前検証の目的 (WtSession の公開 API のエラー種別を `FlowControlError` に統一する) に合わせて更新する。
- `CapsuleEncoder` の doc に値域の概要、`CapsuleEncoder::encode` の doc に `# Errors` を追加し、上記の値域とエラー条件、`Err` 時にバッファを変更しないことを明記する。
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` は `encode` の戻り値に追従する (`let _ = encoder.encode(&capsule);` など)。`clamp_varint` の除去と Padding の u16 制限の見直し、値域境界とエラー値域の fuzz 検証は 0197 で行う。
- `tests/test_webtransport/capsule.rs` に各 variant の境界値テストを追加する。上限ちょうどは `Ok` で往復でき、上限 + 1 は `InvalidInput` になり、`Err` 時にバッファが変化しないことを検証する。境界値は次のとおりである。
  - `WtMaxData` / `WtDataBlocked` / `WtMaxStreamData` / `WtStreamDataBlocked` の `maximum`: `MAX_VALUE` と `MAX_VALUE + 1`
  - `WtMaxStreams` / `WtStreamsBlocked` の `maximum`: `1 << 60` と `(1 << 60) + 1`
  - `WtResetStream` / `WtStopSending` / `WtStream` / `WtMaxStreamData` / `WtStreamDataBlocked` の `stream_id`、`WtResetStream` の `reliable_size`: `MAX_VALUE` と `MAX_VALUE + 1`
  - `WtResetStream` / `WtStopSending` の `error_code`: `0xffffffff` と `0x1_0000_0000`
  - `WtCloseSession` の `reason`: 1,024 バイトと 1,025 バイト
  - `Padding` のエンコード後サイズ: 16 MiB と 16 MiB + 1
  - `Unknown` の `capsule_type`: `MAX_VALUE` と `MAX_VALUE + 1`
  - `Datagram` / `WtStream` / `Unknown` の `data.len()` の varint 上限は、実装上 4 EiB の Vec を作れないため防御的な検証とし、境界テストはスカラー値で代替する。
- 既存の `CapsuleEncoder::encode` 呼び出し側を更新する。`src/webtransport.rs` は `?`、`tests/test_webtransport/capsule.rs` / `tests/test_webtransport/integration.rs` / `pbt/tests/prop_webtransport/main.rs` / `pbt/tests/prop_webtransport/stream.rs` / `crates/tokio-http2/tests/test_webtransport.rs` は `.expect(...)`、`fuzz/fuzz_targets/fuzz_capsule_encoder.rs` は戻り値の無視に変更する。`tests/test_webtransport/integration.rs` の「CapsuleEncoder 内で panic するため」という既存テストのコメントも新しい前提に合わせて更新する。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しない。

## 変更対象

- `src/webtransport/capsule.rs` — `CapsuleEncoder::encode` / `encode_varint` / `encode_header` / 定数 / rustdoc
- `src/webtransport.rs` — `CapsuleEncoder::encode` の呼び出し 9 箇所、`WtSession` の事前検証コメントと doc
- `tests/test_webtransport/capsule.rs` — 既存呼び出しの更新、境界値テストの追加
- `tests/test_webtransport/integration.rs` — 既存呼び出しとコメントの更新
- `pbt/tests/prop_webtransport/main.rs` — 既存呼び出しの更新
- `pbt/tests/prop_webtransport/stream.rs` — 既存呼び出しの更新
- `crates/tokio-http2/tests/test_webtransport.rs` — 既存呼び出しの更新
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` — `encode` の戻り値への追従 (`clamp_varint` の除去は 0197)

## 完了条件

- 公開 `CapsuleEncoder` が不正な値域で panic しないこと。すべての値域外入力が `InvalidInput` の `Err` を返し、`Err` 時にバッファが変化しないこと。
- WT_CLOSE_SESSION の reason 長が 1,024 バイト以下、WT_MAX_STREAMS / WT_STREAMS_BLOCKED の `maximum` が 2^60 以下、WT_RESET_STREAM / WT_STOP_SENDING の `error_code` が 0xffffffff 以下であることが検証されること。
- `Capsule::Padding` のエンコード後サイズが 16 MiB を超える場合に `InvalidInput` を返し、`length` バイトのゼロ埋めを書き込まないこと。
- `Capsule::Unknown` の `capsule_type`、`Capsule::WtStream` の `stream_id` / payload 長、`Capsule::Datagram` の payload 長など、varint フィールドの上限が検証されること。
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` が `encode` の戻り値に追従し、`cargo check --manifest-path fuzz/Cargo.toml` が通ること。`clamp_varint` の除去と不正値域の fuzz 検証は 0197 で行うこと。
- `tests/test_webtransport/capsule.rs` に各 variant の上限境界テストが追加され、上限ちょうどは `Ok`、上限 + 1 は `InvalidInput`、`Err` 時にバッファが変化しないことが検証されていること。
- `WtSession` の既存の事前検証と `FlowControlError` の契約が維持され、既存テストが通ること。
- `CapsuleEncoder` の doc に値域の概要、`CapsuleEncoder::encode` の doc に `# Errors` と値域・エラー条件が明記されていること。
- `CHANGES.md` が変更されていないこと (`CODEBASE.md`)。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。

## スコープ外

- `WtSession::close` の UTF-8 文字境界での reason 切り詰めは変更しない。
- `CapsuleDecoder` の受信側検証 (`decode_payload` の error code / reason 長) と `WtSession::handle_capsule` の 2^60 検証は変更しない。
- `WtSession` の既存の事前検証とエラー種別 `FlowControlError` は変更しない。
- 0182 が扱う `WtSession` の送信 API の 1 capsule 16 MiB 上限は本 issue の対象外とし、`CapsuleEncoder` の Padding の上限だけを扱う。
- `CapsuleEncoder` の `take` / `buffer` / `clear` は変更しない。
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` の `clamp_varint` 除去、Padding の u16 制限の見直し、値域境界とエラー値域の fuzz 検証は 0197 の範囲とする。

## 参照

- `refs/draft-ietf-webtrans-http2-16.txt` — Section 6.1 (PADDING)、Section 6.2 / 6.3 (WT_RESET_STREAM / WT_STOP_SENDING の 0xffffffff 上限)、Section 6.7 / 6.10 (WT_MAX_STREAMS / WT_STREAMS_BLOCKED の 2^60 上限)、Section 6.12 (WT_CLOSE_SESSION の 1,024 バイト上限)
- `refs/rfc9297.txt` — Section 3.2 (Capsule Protocol)、Section 3.3 (malformed capsule)
- `refs/rfc9000.txt` — Section 16 (可変長整数)
- `src/webtransport/capsule.rs` — `CapsuleEncoder::encode` / `encode_varint` / `encode_header` / `CapsuleDecoder::decode_payload` / `MAX_CLOSE_REASON_LEN` / `MAX_APPLICATION_ERROR_CODE` / `DEFAULT_MAX_BUFFER_SIZE`
- `src/webtransport/varint.rs` — `MAX_VALUE` / `encoded_len` / `encode`
- `src/webtransport.rs` — `WtSession::send_max_data` / `send_max_stream_data` / `reset_stream` / `stop_sending` / `send_max_streams` / `close` / `handle_capsule`
- `src/webtransport/flow_control.rs` — `MAX_STREAMS_LIMIT` / `check_max_streams_increase`
- `tests/test_webtransport/capsule.rs` — 既存の encode/decode テスト
- `tests/test_webtransport/integration.rs` — `CapsuleEncoder` の呼び出しと `WtSession` のエラー種別テスト
- `pbt/tests/prop_webtransport/main.rs` / `pbt/tests/prop_webtransport/stream.rs` — Capsule 往復テスト
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` — `clamp_varint` / Padding の u16 制限
- `issues/closed/0127-bug-wt-send-api-varint-panic.md` — `WtSession` 送信 API の varint 超過対策
- `issues/closed/0061-bug-fix-wt-close-session-reason-truncation.md` — `WtSession::close` の reason 長の経緯
- `issues/0182-bug-fix-wt-capsule-size-limit.md` — 1 capsule 16 MiB 上限 (本 issue のスコープ外)
- `issues/0197-test-add-wt-pbt-fuzz.md` — `fuzz_capsule_encoder.rs` の `clamp_varint` 除去と値域 fuzz (本 issue のスコープ外)
