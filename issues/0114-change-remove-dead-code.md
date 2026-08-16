# 死にコードを削除する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-remove-dead-code
- Polished: {YYYY-MM-DD}

## 目的

生産コードから全く使用されていない `RecvBuffer` 構造体と、テストでのみ使用されている公開メソッド群を削除し、コードベースを整理する。

## 現状

以下の死にコードが存在する:

1. `RecvBuffer` 構造体 (`src/stream/buffer.rs` の `RecvBuffer` 型) — `Stream` のフィールドとして存在し getter も定義されているが、生産コードのどこからも呼び出されていない
2. `Stream` の `recv_buffer` フィールドおよび `recv_buffer()` / `recv_buffer_mut()` getter (`src/stream.rs` の `Stream` 型)
3. `FrameDecoder::clear()` (`src/frame/decoder.rs` の `FrameDecoder` 型) — テストでのみ使用
4. `FrameEncoder::take()` (`src/frame/encoder.rs` の `FrameEncoder` 型) — 使用箇所なし
5. `CapsuleEncoder::buffer()` / `clear()` (`src/webtransport/capsule.rs` の `CapsuleEncoder` 型) — テストでのみ使用
6. `CapsuleDecoder::remaining()` / `clear()` / `with_max_buffer_size()` (`src/webtransport/capsule.rs` の `CapsuleDecoder` 型) — テストでのみ使用
7. `varint::encode_to_vec()` (`src/webtransport/varint.rs` の `encode_to_vec` 関数) — 生産コードで使用されていない
8. `SettingsFrame::from_settings()` (`src/frame.rs` の `SettingsFrame` 型) — PBT でのみ使用

`CHANGES.md` の `## develop` セクションには「未使用の公開 API を削除する」という方針が既に宣言されている（`CHANGES.md:103-107`）。

## 設計方針

- `RecvBuffer` 構造体と `Stream` の関連フィールド・getter を完全に削除する
- テストのみで使用されている公開メソッドは、テストコードを修正して公開メソッドに依存しない形に書き換えるか、削除する
- テストが依存している場合は、テストを `tests/` ディレクトリに移動し、`pub(crate)` や `#[cfg(test)]` で対応する
- 影響範囲を確認し、削除後に全テストが通過することを確認する

## 完了条件

- `RecvBuffer` 構造体が削除されていること
- 上記の全未使用メソッドが削除されていること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
