# RecvBuffer::push の整数オーバーフローでバッファ制限をバイパスできる問題を修正する

- Priority: Medium
- Created: 2026-06-06
- Model: DeepSeek V4 Pro
- Branch: feature/fix-recv-buffer-overflow
- Polished: 2026-06-06

## 目的

`src/stream/buffer.rs:91` の `RecvBuffer::push` で `self.data.len() + data.len() > self.max_size` の加算がオーバーフローしうる。release ビルドでは wrap-around によりバッファサイズ制限をバイパスできる。

## 優先度根拠

- release ビルドで `self.data.len() + data.len()` が `usize::MAX` を超えると wrap-around し、`> self.max_size` が偽になって制限をバイパスする
- ただし `RecvBuffer::push()` は現在の本番コードパスから呼び出されていない。`RecvBuffer` は `Stream` のフィールドとしてインスタンス化され `recv_buffer()` / `recv_buffer_mut()` で公開されているが、`push()` を呼び出すコードは存在しない
- 将来の使用に備えて修正すべきであり、優先度 Medium

## 現状

`src/stream/buffer.rs:91`:

```rust
pub fn push(&mut self, data: &[u8]) -> bool {
    if self.data.len() + data.len() > self.max_size {
        return false;
    }
    self.data.extend_from_slice(data);
    true
}
```

対照的に、同ファイルの `RecvBuffer::remaining()` (line 124) と `SendBuffer::push()` (line 29) では以下のように `saturating_sub` を用いた安全な検査が既に行われている:

```rust
// RecvBuffer::remaining (line 124)
pub fn remaining(&self) -> usize {
    self.max_size.saturating_sub(self.data.len())
}

// SendBuffer::push (line 29-30)
let available = self.max_size.saturating_sub(self.data.len());
let to_push = data.len().min(available);
```

`RecvBuffer::push` のみが `saturating_add` を使用しておらず、同一構造体内で不整合が生じている。

## 設計方針

`saturating_add` を使用する。`SendBuffer::push` の `saturating_sub` パターンに合わせても良いが、`RecvBuffer::push` は「push 可能かどうか」の判定に加算が必要なため `saturating_add` の方が直接的でコード意図が明確。

```rust
if self.data.len().saturating_add(data.len()) > self.max_size {
    return false;
}
```

## 対応手順

1. 作業ブランチ `feature/fix-recv-buffer-overflow` を作成する
2. `src/stream/buffer.rs:91` の `self.data.len() + data.len()` を `self.data.len().saturating_add(data.len())` に変更する
3. `tests/test_stream/buffer.rs` にオーバーフロー境界のテストを追加する（`max_size = usize::MAX - 1` のバッファに長さ 2 のデータを push して `false` が返ることを確認する等）
4. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する
5. `cargo test --workspace` で全テスト通過を確認する
6. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する

## 完了条件

- `RecvBuffer::push` が `saturating_add` を使用するように修正されている
- `RecvBuffer` の 3 メソッド（`push` / `remaining` / `SendBuffer::push`）の検査方法が一貫して飽和演算を使用している
- オーバーフロー境界のテストが追加されている
- `CHANGES.md` の `## develop` にエントリが追加されている
- `cargo test --workspace` が通過する
