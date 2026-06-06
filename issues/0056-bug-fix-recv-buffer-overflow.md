# RecvBuffer::push の整数オーバーフローでバッファ制限をバイパスできる問題を修正する

- Priority: Medium
- Created: 2026-06-06
- Model: DeepSeek V4 Pro

## 目的

`src/stream/buffer.rs:91` の `RecvBuffer::push` で `self.data.len() + data.len() > self.max_size` の加算がオーバーフローしうる。攻撃者が任意長の `data` スライスを渡すことで `usize::MAX` を超えて wrap-around し、バッファサイズ制限をバイパスできる。

## 優先度根拠

- debug ビルドでは panic、release ビルドでは wrap-around によりバッファ制限突破
- ただし `RecvBuffer` は現在実質的に使用されておらず、実際のデータ受信は `Event::DataReceived` として直接利用者に渡されている
- 攻撃者が直接 `RecvBuffer::push` を呼び出せる経路は現状存在しないが、将来的な使用に備えて修正すべき

## 現状

`src/stream/buffer.rs:91`:

```rust
if self.data.len() + data.len() > self.max_size {
    return false;
}
```

対照的に `SendBuffer::push` (line 30) は安全に実装されている:

```rust
let remaining = data.len().saturating_sub(self.available());
```

## 設計方針

`saturating_add` ベースの検査に統一する:

```rust
if self.data.len().saturating_add(data.len()) > self.max_size {
    return false;
}
```

または `SendBuffer::push` と同様に `saturating_sub` パターンに統一する。

## 完了条件

- `RecvBuffer::push` のオーバーフロー検査が `saturating_add` を使用するように修正されている
- `RecvBuffer::push` と `SendBuffer::push` の検査パターンが一貫している
- `cargo test --all` が通過する

## 解決方法

1. 作業ブランチ `feature/fix-recv-buffer-overflow` を切る
2. `src/stream/buffer.rs:91` の `self.data.len() + data.len()` を `self.data.len().saturating_add(data.len())` に変更する
3. オーバーフロー境界値のテストを追加する
4. `cargo test --all` で全通過を確認する
