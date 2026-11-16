# テスト未使用の公開 API にテストを追加する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/add-tests-for-unused-public-api
- Polished: 2026-11-16

## 目的

テスト・PBT を含む全コードベースで一度も使用されていない公開 API が残っている。CODEBASE.md の規約（「公開 API は必ず使用箇所とテストを用意すること」）により、未使用・テスト未使用の公開 API は追加してはならず、既存のものはテストを追加して動作を保証するか削除して解消することと定められている。

本 issue の対象 7 件は `Stream`（HTTP/2 コアの公開型）の基本 getter と Capsule デコーダの基本アクセサであり、
削除は破壊的変更になるため、テストを追加して動作を保証する。ここで「テスト未使用」とは、
テスト・PBT を含む全コードベース (`src/` / `crates/` / `tests/` / `pbt/` / `fuzz/` / `examples/`) で
一度も使用されていないことを指す。なお 0113 が削除を選んだのは `WtFlowControl` の内部詳細 getter が
対象だったためであり、コア型の基本 getter を対象とする本 issue とは性質が異なる。

## 現状

以下の公開 API は `src/` / `crates/` / `tests/` / `pbt/` / `fuzz/` / `examples/` のすべてで使用されていない:

1. `Stream::recv_buffer()` / `recv_buffer_mut()` (`src/stream.rs` の `Stream` 型) — 0072 で「将来の受信バッファ経路 API として意図的に維持」と判断されている。ただしテストが存在しない
2. `CapsuleDecoder::remaining()` (`src/webtransport/capsule.rs` の `CapsuleDecoder` 型) — 全コードベースで使用箇所なし
3. `Stream::id()` (`src/stream.rs` の `Stream` 型) — 全コードベースで使用箇所なし
4. `Stream::headers()` (`src/stream.rs` の `Stream` 型) — 全コードベースで使用箇所なし (`set_headers()` は内部で使用されている)
5. `Stream::state_machine()` (`src/stream.rs` の `Stream` 型) — 全コードベースで使用箇所なし (可変版の `state_machine_mut()` のみ使用されている)
6. `Stream::is_open()` (`src/stream.rs` の `Stream` 型) — 全コードベースで使用箇所なし
7. `Stream::is_closed()` (`src/stream.rs` の `Stream` 型) — 全コードベースで使用箇所なし

本 issue の対象は、`src/stream.rs` と `src/webtransport/capsule.rs` の公開関数・メソッドのうち、
全コードベースで未使用のものに限定する。`impl Default` などの trait 実装は、0072 が
`SendBuffer` / `RecvBuffer` の `Default` を意図的に維持した前例に従い対象外とする。

テストでのみ使用されている公開 API（`FrameDecoder::clear()`、`CapsuleEncoder::buffer()` / `clear()`、
`CapsuleDecoder::clear()` / `with_max_buffer_size()`、`SettingsFrame::from_settings()` 等）は、
0113 の方針（テストが動作を保証しているため削除対象としない）に従い本 issue の対象外とする。

## 設計方針

- 以下の API に公開 API 経由のテストを追加する:
  - `Stream::recv_buffer()` / `recv_buffer_mut()`: `Stream::new()` で生成し、`recv_buffer_mut()` で `push` → `recv_buffer()` で `len` / `is_empty` / `pop` を検証するテストを `tests/test_stream/buffer.rs` に追加する
  - `CapsuleDecoder::remaining()`: `feed()` 後に `remaining()` の値が期待どおりになること、`decode()` 後に減少することを検証するテストを `tests/test_webtransport/capsule.rs` に追加する
  - `Stream::id()` / `headers()` / `state_machine()` / `is_open()` / `is_closed()`: `Stream::new()` で生成し、各 getter の返り値と状態遷移を検証するテストを `tests/test_stream/main.rs` に追加する
- テストは公開 API 経由で書く（`tests/` は公開 API に対してだけ書くという shiguredo-rust 規約に従う）
- 追加後、`src/stream.rs` と `src/webtransport/capsule.rs` の公開関数・メソッドで未使用のものが
  本 issue の対象以外にないことを ripgrep (rg) で確認する (`impl Default` などの trait 実装は対象外)

## 完了条件

- `Stream::recv_buffer()` / `recv_buffer_mut()` の動作を検証するテストが `tests/test_stream/buffer.rs` に追加されている
- `CapsuleDecoder::remaining()` の動作を検証するテストが `tests/test_webtransport/capsule.rs` に追加されている
- `Stream::id()` / `headers()` / `state_machine()` / `is_open()` / `is_closed()` の動作を検証するテストが `tests/test_stream/main.rs` に追加されている
- `src/stream.rs` と `src/webtransport/capsule.rs` の公開関数・メソッドのうち、全コードベースから
  呼ばれていないものが本 issue の対象以外に残っていないことが ripgrep (rg) で確認されている
  (`impl Default` などの trait 実装は対象外)
- `CHANGES.md` は CODEBASE.md の現行指示「この指示がなくなるまでは変更履歴を `CHANGES.md` に
  残さないこと」に従い、変更しない (0199 で CHANGES.md の運用と CODEBASE.md の整合を図る)
- `cargo fmt --all -- --check` が通過する
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する
