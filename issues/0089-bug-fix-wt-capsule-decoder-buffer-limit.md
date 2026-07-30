# CapsuleDecoder::feed にバッファサイズ上限を追加して DoS を防止する

- Created: 2026-07-30
- Completed: {Completed}
- Branch: feature/fix-wt-capsule-decoder-buffer-limit
- Polished: 2026-07-30

## 目的

`CapsuleDecoder::feed` にバッファサイズ上限がなく、ピアが不完全なカプセルを送り続けることでメモリを無制限に消費できる DoS 攻撃を防止する。

## 現状

`src/webtransport/capsule.rs` の `CapsuleDecoder::feed` は `self.buffer.extend_from_slice(data)` で無制限にバッファを拡張する。ピアが Length フィールドで巨大値を宣言し Payload を送らない場合、`decode()` は `Ok(None)` を返し続け、バッファは解放されない。

## 設計方針

バッファ上限（例: 16 MiB）を設け、超過時は `WtError` を返す。上限値は `CapsuleDecoder::new` の引数または定数で設定可能にする。

## 完了条件

- バッファ上限超過時にエラーが返ること
- 上限値が定数またはコンストラクタ引数で設定可能であること
- 上限超過の単体テストが追加されていること

## 解決方法

`CapsuleDecoder` に `max_buffer_size: usize` フィールドを追加し、`feed` 内で `self.buffer.len() + data.len() > self.max_buffer_size` のチェックを追加する。超過時は `WtError::invalid_input` を返す。
