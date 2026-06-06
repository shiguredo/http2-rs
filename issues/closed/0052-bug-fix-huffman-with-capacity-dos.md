# Huffman デコーダの Vec::with_capacity(data.len() * 2) が規約違反かつ DoS リスクを修正する

- Priority: High
- Created: 2026-06-06
- Completed: 2026-06-06
- Model: DeepSeek V4 Pro
- Branch: feature/fix-huffman-with-capacity
- Polished: 2026-06-06

## 目的

`src/hpack/huffman.rs:1120` の `Vec::with_capacity(data.len() * 2)` が AGENTS.md の以下の規約に違反しており、かつ攻撃者によるメモリ枯渇 DoS のリスクがある。

## 優先度根拠

- AGENTS.md:122-125 に明記された禁止事項に違反している
- 攻撃者が Huffman エンコードの長大文字列（実データが長い Huffman 符号化バイト列）を注入することで、`data.len() * 2` バイトのメモリを事前確保させられる
- 同じファイル内の `encode_to_vec` では `vec![0u8; len]` を使用しており、デコード側だけが規約から逸脱している

## 現状

`src/hpack/huffman.rs:1120`:

```rust
pub(crate) fn decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut result = Vec::with_capacity(data.len() * 2);
```

AGENTS.md:122-125:

> 入力バイナリデータをデコードする際には `Vec::with_capacity()` などのメモリを事前に割り当てるメソッドを原則として使用しないこと
> 入力データが破損している場合などに、サイズやカウントを示す値のデコード結果が極端に大きくなり、メモリを大量に消費してしまうリスクがあるため
> このケースでも `Vec::new()` を使っておけば、メモリ消費量のオーダーは実際の入力データのサイズから大きく乖離することはない

## 設計方針

`Vec::with_capacity(data.len() * 2)` を `Vec::new()` に置き換える。

## 対応手順

1. 作業ブランチ `feature/fix-huffman-with-capacity` を作成する
2. `src/hpack/huffman.rs:1120` の `Vec::with_capacity(data.len() * 2)` を `Vec::new()` に変更する
3. `CHANGES.md` の `## develop` セクション `### misc` に `[FIX]` エントリを追加する
4. `cargo test --workspace` を実行して全テスト通過を確認する
5. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する

## 完了条件

- `src/hpack/huffman.rs:1120` が `Vec::new()` に置き換えられている
- `CHANGES.md` の `## develop` に `[FIX]` エントリが追加されている
- `cargo test --workspace` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する

## 解決方法

1. `src/hpack/huffman.rs:1120` の `Vec::with_capacity(data.len() * 2)` を `Vec::new()` に変更した。
2. `CHANGES.md` の `### misc` セクションに `[FIX]` エントリを追加した。
3. `cargo test --workspace` と `cargo clippy --workspace --all-targets -- -D warnings` の通過を確認した。
