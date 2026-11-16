# 公開 `CapsuleEncoder::encode` が varint 超過で panic し、仕様違反 capsule を生成できる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-capsule-encoder-validation
- Polished: {YYYY-MM-DD}

## 目的

`CapsuleEncoder::encode` は `Result` を返さず、内部の `encode_varint` が `varint::encode(...).expect(...)` する。`Capsule::WtMaxData { maximum: u64::MAX }` などを渡すと panic する。また WT_CLOSE_SESSION の reason 長は `debug_assert!` のみで release では 1,024 バイト超をそのままエンコードし、WT_MAX_STREAMS の 2^60 超、WT_RESET_STREAM / WT_STOP_SENDING の 0xffffffff 超も検証しない。公開 API の値域検証を追加する。

## 現状

`src/webtransport/capsule.rs` の `CapsuleEncoder::encode_varint` は `varint::encoded_len` でバッファを確保し、`varint::encode` が `Err` を返すと `expect` で panic する。`Capsule::WtCloseSession` の reason 長は `MAX_CLOSE_REASON_LEN` を `debug_assert!` でしか検査しない。

`WtMaxData { maximum: u64::MAX }` のエンコードで panic することを確認済みである。`fuzz_capsule_encoder.rs` は `clamp_varint` で不正値を避けており、この panic 経路を検証していない。

## 設計方針

closed 0127 は `WtSession` の送信 API 側で varint 超過を事前拒否したが、公開 `CapsuleEncoder` 自体の panic 経路は残っている。closed 0061 は `WtSession::close` 側の reason 長を扱ったが、現在の `close` は UTF-8 境界で切り詰める方針であり、公開エンコーダを直接使う利用者には検証がない。本 issue はエンコーダの値域検証を対象とする。

- `CapsuleEncoder::encode` を `WtResult<()>` に変更し、varint 上限・reason 長・エラーコード上限・最大ストリーム数上限を検証する。
- 互換性のため `encode` を維持する場合は、`# Panics` と事前条件を rustdoc に明記し、内部利用側で必ず検証する。
- `Capsule::Padding` の巨大な長さにも上限を設ける。
- fuzz の `clamp_varint` を外し、値域境界とエラー値域を分けて検証する。
- 各 variant の上限境界テストを追加する。

## 完了条件

- 公開 `CapsuleEncoder` が不正な値域で panic しないこと。
- WT_CLOSE_SESSION の reason 長、WT_MAX_STREAMS の 2^60、WT_RESET_STREAM / WT_STOP_SENDING の 0xffffffff が検証されること。
- fuzz が不正値域の panic を検出できること。
- 境界値テストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
