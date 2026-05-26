# 未使用コードを削除する

- Priority: Low
- Created: 2026-05-14
- Model: deepseek-v4-pro
- Completed: 2026-05-26
- Branch: feature/change-remove-dead-code

## 目的

コードベース内に呼び出しのない公開関数・メソッドが残存している。`#[expect(dead_code)]` 等の抑制なしにコンパイルが通る状態（公開 API のため lint 対象外）だが、メンテナンスコストと混乱を生むため削除する。

## 優先度根拠

機能に影響しない純粋なコード整理。削除しなくても動作に支障はないが、不要な公開 API が残ると利用者を誤導する可能性がある。

## 現状

以下の 6 件が呼び出し元なしで残存している:

| # | ファイル | 関数/メソッド | 備考 |
|---|---|---|---|
| 1 | `src/frame/encoder.rs:326` | `pub fn encode_header(buf, header)` | スタンドアロン関数。`FrameEncoder::encode_header` メソッドが全エンコードに使用されており、この関数の呼び出し元はない |
| 2 | `src/frame/encoder.rs:353` | `pub fn encode_frame(buf, frame)` | スタンドアロン関数。呼び出し元なし |
| 3 | `src/frame/encoder.rs:363` | `pub fn encode_frame_to_vec(frame)` | スタンドアロン関数。呼び出し元なし |
| 4 | `src/frame/decoder.rs:44` | `FrameDecoder::set_max_frame_size` | 呼び出し元なし |
| 5 | `src/frame/flags.rs:68` | `FrameFlags::clear` | 呼び出し元なし |
| 6 | `src/stream/state.rs:81` | `StreamState::is_idle` | 呼び出し元なし |

## 設計方針

上記 6 件を削除する。テストコード (`tests/`, `pbt/`) からの呼び出しも存在しないことを確認済み。

#1-3 はいずれも `FrameEncoder` 構造体の登場以前に作られた旧 API と思われる。#4 は Settings 適用経路が変わった際に孤立したメソッド。#5, #6 は将来のために残されたが使われないまま放置されたもの。

### 削除時の注意

- #1-3 は `pub fn` であり外部クレートが参照している可能性がある。ただし `lib.rs` で re-export されておらず、`src/frame/encoder.rs` の module-level 関数が `pub` であっても `pub use` されていない限り外部からアクセス不可。`src/lib.rs` と `src/frame/mod.rs` の re-export を確認して削除すること
- #4 (`set_max_frame_size`) は `pub fn` だが `FrameDecoder` 自体が `pub use` されている。外部利用者がこのメソッドを使っている可能性があるため、これは `[CHANGE]` に分類する

## 変更対象ファイル

- `src/frame/encoder.rs`: #1, #2, #3 削除
- `src/frame/decoder.rs`: #4 削除
- `src/frame/flags.rs`: #5 削除
- `src/stream/state.rs`: #6 削除
- `CHANGES.md`: エントリ追加

## 完了条件

- 上記 6 件が全て削除されている
- `cargo test --workspace` が通る
- `cargo clippy --workspace --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
- fuzz ターゲットがビルドできる (`cargo check --manifest-path fuzz/Cargo.toml`)

## 備考: 既に解決済みの項目

本 issue は元々 17 件を対象としていたが、以下の 11 件は他の issue (0026, 0029 等) で既に削除済みのため除外した:

- `check_buffer_size` (issue 0029 で削除)
- `WtInitialSettings::apply`, `WtInitialSettings::new` (issue 0026 で削除)
- `SendBuffer::remaining`, `SendBuffer::clear`, `RecvBuffer::remaining`, `RecvBuffer::clear` (削除済み)
- `Stream::is_open`, `Stream::is_closed`, `Stream::headers` (削除済み)
- `StateMachine::sent_end_stream`, `StateMachine::received_end_stream` (削除済み)
- `Event::stream_id`, `Event::is_connection_level` (削除済み)

## 解決方法

以下の 6 件の未使用公開 API を削除した:

1. `src/frame/encoder.rs`: スタンドアロン関数 `encode_header`, `encode_frame`, `encode_frame_to_vec` を削除。不要になった `DecodeError` と `FRAME_HEADER_SIZE` の import も除去
2. `src/frame/decoder.rs`: `FrameDecoder::set_max_frame_size` メソッドを削除
3. `src/frame/flags.rs`: `FrameFlags::clear` メソッドを削除
4. `src/stream/state.rs`: `StreamState::is_idle` メソッドを削除

全テスト・clippy・fmt・fuzz ビルドが通ることを確認済み。
