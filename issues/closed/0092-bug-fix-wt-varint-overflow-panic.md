# WtFlowControl::add_recv_max のオーバーフローで varint エンコードが panic する問題を修正する

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/fix-wt-varint-overflow-panic
- Polished: 2026-07-30

## 目的

`WtFlowControl::add_recv_max` が `saturating_add` で `u64::MAX` に到達した際、後続の `WT_MAX_DATA` エンコードで `CapsuleEncoder::encode_varint` の `expect` が panic する問題を修正する。

## 現状

`src/webtransport/flow_control.rs` の `add_recv_max` は `self.recv_max.saturating_add(increment)` で上限なく加算する。`recv_max` が varint の最大値（2^62 - 1）を超えると、`src/webtransport/capsule.rs` の `encode_varint` 内で `varint::encode` がエラーを返し、`.expect("buffer is pre-sized to encoded_len")` で panic する。

## 完了条件

- `recv_max` が varint MAX_VALUE を超えないこと
- 上限到達時に適切なエラーが返ること
- 境界値の単体テストが追加されていること

## 解決方法

`add_recv_max` 内で `new_max > varint::MAX_VALUE` のチェックを追加し、超過時は `WtError::flow_control_error` を返す。`WtStream::update_recv_max` は現状 `()` を返すが、上限チェックのために `WtResult<()>` へシグネチャを変更し、呼び出し側の `grow_stream_recv_window` でエラーを伝播させる。
