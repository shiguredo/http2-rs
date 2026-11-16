# `CapsuleDecoder::decode` の先頭 drain による O(n²) デコードを解消する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-capsule-decoder-quadratic
- Polished: {YYYY-MM-DD}

## 目的

`src/webtransport/capsule.rs` の `CapsuleDecoder::decode` は capsule を 1 個処理するたびに `Vec::drain(..total_len)` で残り全バイトを memmove する。最小 2 バイトの capsule を大量に並べると入力サイズの二乗で CPU を消費し、リモートから CPU DoS を成立させられるため、線形時間の実装に変更する。

## 現状

`CapsuleDecoder::decode` はバッファ先頭から capsule を読み、`total_len` 分を `drain` で削除する。このため 16 KiB の DATA に 2 バイト capsule を 8192 個入れるだけで約 67 MB の memmove が発生する。一時計測では 16 KiB で約 1.2 ms、64 KiB で約 9.7 ms、256 KiB で約 128 ms であり、入力サイズに対して二次関数的に増加した。

HTTP/2 の DATA フレームは小さくても、ピアは多数のフレームに分割して送れるため、この増幅率は実用的な攻撃面になる。

## 設計方針

- バッファの読み取り offset を保持し、`decode` では offset を進めるだけにする。未処理バイトがなくなった時点で一度だけバッファを詰める。
- または `VecDeque` を使い、先頭から必要分だけ pop する。
- `feed` のバッファ上限 (16 MiB) と整合する形で、offset がバッファ長を超えないことを検査する。
- 2 バイト capsule の大量入力を処理する性能回帰テストを追加する。

## 完了条件

- `CapsuleDecoder::decode` が capsule 数に対して線形時間で動作すること。
- 16 KiB / 64 KiB / 256 KiB の 2 バイト capsule 入力で、処理時間が入力サイズにほぼ比例すること。
- 既存の capsule roundtrip / unknown / padding / 分割 feed のテストが維持されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
