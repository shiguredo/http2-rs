# Huffman デコーダを O(1) の木構造に置き換える

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/perf-huffman-decoder-o1
- Polished: {YYYY-MM-DD}

## 目的

Huffman デコーダの線形探索を O(1) の木構造またはルックアップテーブルに置き換え、デコード性能を改善する。

## 現状

`src/hpack/huffman.rs` のデコードループ内で `HUFFMAN_TABLE.iter().enumerate()` により 257 エントリを毎回線形探索している。RFC 7541 Appendix B の Huffman 符号は正準 Huffman 符号であり、O(1) の木構造デコーダで実装可能。

現在の実装は最悪ケースで O(257 × 入力バイト数) となり、DoS 耐性の観点でも改善の余地がある。

## 設計方針

- RFC 7541 Appendix B の Huffman 符号テーブルを基に、O(1) のルックアップテーブルまたは二分木を構築する
- 正準 Huffman 符号の性質を利用し、コード長とシンボルのマッピングによる高速デコードを行う
- 既存の EOS パディング検出ロジックは維持する
- 既存のテストが全件通過することを確認する

## 完了条件

- デコードが O(1) で動作すること
- 既存の全テストが通過すること
- `cargo test -p shiguredo_http2` が全件通過すること
- 性能改善が確認できること（ベンチマークは任意）
