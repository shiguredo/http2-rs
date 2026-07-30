# shiguredo_nghttp2::Session::recv / send の isize から i32 への切り詰めを修正する

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/fix-nghttp2-recv-isize-truncation
- Polished: 2026-07-30

## 目的

`Session::recv` が `nghttp2_session_mem_recv` の戻り値（`ssize_t` = 64 bit）を `as i32` で切り詰めており、`data.len()` が `i32::MAX` を超える場合に誤動作する問題を修正する。

## 現状

`crates/shiguredo_nghttp2/src/session.rs` の `Session::recv` は `check_nghttp2_with_value(result as i32)` で `ssize_t` を `i32` に切り詰めている。`Session::send` の `nghttp2_session_mem_send` 戻り値も同様に `len as i32` で切り詰めている。

## 完了条件

- `ssize_t` の戻り値が `i32` に切り詰められないこと
- `i32::MAX` を超える入力でも正しく動作すること

## 解決方法

`check_nghttp2_with_value` を `isize`（または `i64`）で受けるように変更するか、`recv` / `send` 内で `isize` のままエラー判定を行ってから `usize` に変換する。
