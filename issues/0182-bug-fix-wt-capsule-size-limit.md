# 16 MiB 超の WT_STREAM / DATAGRAM が自分の受信側で拒否される

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-capsule-size-limit
- Polished: {YYYY-MM-DD}

## 目的

`CapsuleDecoder` の既定バッファ上限は 16 MiB 固定だが、`WtSession::send_datagram` / `send_stream_data` はそれ以上のデータを 1 個の capsule としてエンコードできる。送信側の API が正常に返しても、受信側は capsule 全量が揃うまでデコードできず `InvalidInput` で失敗する。送受信の上限を一致させ、相互運用可能な契約にする。

## 現状

`src/webtransport/capsule.rs` の `DEFAULT_MAX_BUFFER_SIZE` は 16 MiB。`WtSession::send_datagram` と `send_stream_data` は capsule サイズの上限を検査せず、`Capsule::Datagram` / `Capsule::WtStream` をそのまま生成する。

16 MiB + 1 の DATAGRAM と 17 MiB の WT_STREAM を送信し、受信側の `CapsuleDecoder::feed` が `capsule decoder buffer limit exceeded` になることを確認済みである。

## 設計方針

- 送信 API で capsule サイズ上限を検査し、超過時は `flow_control_error` または専用エラーで拒否する。
- ストリームデータは呼び出し側で複数の WT_STREAM に分割できるため、1 capsule の上限を doc に明記する。
- DATAGRAM は分割できないため、上限超過を明確にエラーにする。
- `WtSession` のデコーダ上限を設定可能にする場合は、送信側の上限と共通の定数・設定にする。
- 16 MiB 境界と超過の送受信テストを追加する。

## 完了条件

- 送信 API が受信側の上限を超える capsule を生成しないこと、または上限超過を明示的にエラーにすること。
- 上限値が rustdoc と README に明記されていること。
- 16 MiB 境界・超過のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
