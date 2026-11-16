# END_STREAM の切り詰め capsule と malformed capsule を RST_STREAM で処理する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-malformed-capsule-error
- Polished: 2026-11-17

## 目的

RFC 9297 Section 3.3 は、clean に終端したストリームの最後の capsule が切り詰められていた場合、malformed/incomplete として扱うことを MUST とする。また RFC 9113 Section 8.1.1 は malformed を `PROTOCOL_ERROR` のストリームエラーとする。実装は END_STREAM 時の切り詰めを検出せず、malformed capsule の一部を RST_STREAM なしで接続破棄にしているため、仕様に合わせる。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `DriverState::handle_event` は END_STREAM で `Error::ConnectionClosed` を返すだけで、`WtSession` の capsule デコーダに未処理バイトが残っているか確認しない。`CapsuleDecoder::remaining()` は用意されているが使用されていない。

`src/webtransport/capsule.rs` の `decode_payload` は、payload は揃っているが必須フィールドが短い場合に `varint::decode` の `Incomplete` をそのまま返す。`wt_http2_error_code` は `StreamStateError` / `FlowControlError` / `SessionStateError` だけを RST_STREAM に変換し、`Incomplete` / `CapsuleDecode` / `InvalidInput` は `None` になる。driver の `abort_session_with_wt_error` は `wt_http2_error_code` が `Some` を返す場合だけ `reset_stream` を呼ぶため、これらでは RST_STREAM が送られない。その結果、driver は RST_STREAM を送らずに TCP 接続を落とす。

## 設計方針

- `WtSession` に、capsule デコーダに未処理バイトが残っているかを検査する `pub fn` を追加する。未処理バイト数を返し、0 なら未処理なし、0 より大きければ未処理ありとする。`CapsuleDecoder::remaining()` に委譲する。rustdoc に「ストリーム終端時に呼ぶ」ことと戻り値の契約を明記する (CODEBASE.md の公開 API 規約に従い、テストで使用する)。
- `DriverState::handle_event` の END_STREAM 受信時に、`process()` 完了後かつ WT_CLOSE_SESSION 応答の早期 return より前に、`WtSession::state()` が `Closed` でない場合に限り未処理バイトを検査する。未処理バイトがあれば CONNECT ストリームへ RST_STREAM (`PROTOCOL_ERROR`) を送り、`Error::WebTransport` (`CapsuleDecode`) を返して driver を終了する。`Error::ConnectionClosed` を返すと、呼び出し側が malformed capsule を識別できないため。RST_STREAM 送信後に driver は終了し、HTTP/2 接続も閉じる。接続の再利用は本 issue の範囲外とする。
- WT_CLOSE_SESSION を受信して既に END_STREAM を返信したストリームは closed になる。この経路では未処理バイトの検査を省略する (RFC 9113 Section 5.1: closed ストリームへ送れるのは PRIORITY のみ)。
- `src/webtransport/capsule.rs` の `decode_payload` が返す `Incomplete` を、`decode()` の `decode_payload` 呼び出し箇所で一括して `CapsuleDecode` に変換する。各 `varint::decode` 呼び出しに個別対応しない。payload が必須フィールドより短い capsule は malformed として扱う (RFC 9297 Section 3.3)。`decode_payload` の他のエラー (`SessionStateError` / `CapsuleDecode`) と `decode()` の `Ok(None)` (ヘッダーまたは payload の不足) は変更しない。
- `wt_http2_error_code` で `CapsuleDecode` と `InvalidInput` を `PROTOCOL_ERROR` に対応付ける。RFC 9297 Section 3.3 は、capsule protocol のエラーを malformed/incomplete HTTP message として扱うことを MUST とする。RFC 9113 Section 8.1.1 は malformed を `PROTOCOL_ERROR` のストリームエラーとする。`WT_ERROR` は WebTransport のセッションエラー (draft-ietf-webtrans-http2-16 Section 3.4) であり、capsule の malformed には使わない。`InvalidInput` は受信経路では `CapsuleDecoder::feed` のバッファ上限超過だけなので、これも `PROTOCOL_ERROR` とする。`Incomplete` は `decode_payload` で `CapsuleDecode` に変換されるため、`wt_http2_error_code` には追加しない。
- `wt_http2_error_code` と `abort_session_with_wt_error` の doc コメントを更新し、capsule protocol の malformed を `PROTOCOL_ERROR` とする根拠と、既存の `StreamStateError` / `FlowControlError` / `SessionStateError` の対応を維持することを明記する。
- テストを追加する。
  - `tests/test_webtransport/capsule.rs`: 短い payload の WT_RESET_STREAM / WT_STOP_SENDING、長さ 0 の WT_MAX_DATA、trailing bytes の WT_MAX_DATA が `CapsuleDecode` になること。END_STREAM の切り詰め capsule は `decode()` が `Ok(None)` を返し、`remaining()` が 0 より大きくなること。
  - `tests/test_webtransport/`: 追加する `WtSession` の API が未処理バイトあり/なしを正しく返すこと。
  - `crates/tokio-http2/tests/test_webtransport.rs`: 上記の malformed capsule を CONNECT ストリームへ送り、クライアントが RST_STREAM (`PROTOCOL_ERROR`) を受信し、サーバーの driver が `Error::WebTransport` (`CapsuleDecode`) で終了すること。
  - `crates/tokio-http2/src/webtransport.rs` の `#[cfg(test)]`: `wt_http2_error_code` が `CapsuleDecode` / `InvalidInput` を `PROTOCOL_ERROR`、`StreamStateError` / `FlowControlError` / `SessionStateError` を既存のコード (`WT_STREAM_STATE_ERROR` / `WT_FLOW_CONTROL_ERROR` / `WT_ERROR`) に変換し、`Incomplete` は `None` のままであることを検証する。

## 完了条件

- END_STREAM を受信し、capsule デコーダに未処理バイトが残っている場合、CONNECT ストリームへ RST_STREAM (`PROTOCOL_ERROR`) を送り、driver が `Error::WebTransport` (`CapsuleDecode`) で終了すること。ただし、WT_CLOSE_SESSION を受信して既に END_STREAM を返信したストリームを除く (HTTP/2 接続の再利用は本 issue の範囲外)。
- 短い payload の WT_RESET_STREAM / WT_STOP_SENDING、長さ 0 の WT_MAX_DATA、trailing bytes の WT_MAX_DATA、END_STREAM の切り詰め capsule が `CapsuleDecode` として扱われ、RST_STREAM (`PROTOCOL_ERROR`) になること。
- `CapsuleDecode` / `InvalidInput` が `wt_http2_error_code` で `PROTOCOL_ERROR` に対応付けられ、`Incomplete` は `None` のままであり、既存の `StreamStateError` / `FlowControlError` / `SessionStateError` の対応が維持されていること。
- 追加する `WtSession` の公開 API が rustdoc を持ち、未処理バイトあり/なしの両方をテストで検証していること。
- 上記の malformed capsule 5 経路 (END_STREAM の切り詰め、短い WT_RESET_STREAM、短い WT_STOP_SENDING、長さ 0 の WT_MAX_DATA、trailing bytes の WT_MAX_DATA) のテストが追加されていること。RST_STREAM の検証は driver の E2E テストで行うこと。
- `wt_http2_error_code` と `abort_session_with_wt_error` の doc コメントに、capsule protocol の malformed を `PROTOCOL_ERROR` とする根拠と既存対応の維持が明記されていること。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
