# END_STREAM の切り詰め capsule と malformed capsule を RST_STREAM で処理する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-malformed-capsule-error
- Polished: {YYYY-MM-DD}

## 目的

RFC 9297 Section 3.3 は、clean に終端したストリームの最後の capsule が切り詰められていた場合、malformed/incomplete として扱うことを MUST とする。また RFC 9113 Section 8.1.1 は malformed を `PROTOCOL_ERROR` のストリームエラーとする。実装は END_STREAM 時の切り詰めを検出せず、malformed capsule の一部を RST_STREAM なしで接続破棄にしているため、仕様に合わせる。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::handle_event` は END_STREAM で `Error::ConnectionClosed` を返すだけで、`WtSession` の capsule デコーダに未処理バイトが残っているか確認しない。`CapsuleDecoder::remaining()` は用意されているが使用されていない。

`src/webtransport/capsule.rs` の `decode_payload` は、payload は揃っているが必須フィールドが短い場合に `varint::decode` の `Incomplete` をそのまま返す。`wt_http2_error_code` は `StreamStateError` / `FlowControlError` / `SessionStateError` だけを RST_STREAM に変換し、`Incomplete` / `CapsuleDecode` / `InvalidInput` は `None` になる。その結果、driver は RST_STREAM を送らずに TCP 接続を落とす。

## 設計方針

- `WtSession` に「ストリーム終端時に未処理 capsule が残っていないか」を検査する API を追加する。
- END_STREAM 時に未処理バイトがあれば、CONNECT ストリームへ RST_STREAM (`PROTOCOL_ERROR`) を送る。
- `decode_payload` 内の `Incomplete` を `CapsuleDecode` に変換する。
- `CapsuleDecode` / `InvalidInput` を RST_STREAM (`PROTOCOL_ERROR` または `WT_ERROR`) に対応付ける。
- 切り詰め capsule、長さ 0 の WT_MAX_DATA、trailing bytes のテストを追加する。

## 完了条件

- END_STREAM 時に切り詰め capsule を検出し、RST_STREAM (`PROTOCOL_ERROR`) を送ること。
- 短い payload の WT_RESET_STREAM / WT_STOP_SENDING / WT_MAX_DATA が `CapsuleDecode` として扱われ、接続破棄ではなく RST_STREAM になること。
- 上記 3 経路のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
