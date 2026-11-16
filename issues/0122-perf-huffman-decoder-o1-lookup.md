# Huffman デコーダの線形探索を O(1) のルックアップに置き換える

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-huffman-decoder-o1-lookup
- Polished: 2026-11-16

## 目的

Huffman デコーダのシンボルごとの線形探索を、入力長に依存しない固定回数のルックアップ（ルックアップテーブルまたは二分木）に置き換え、デコード性能を改善する。全体のデコード時間は入力長に比例する O(n) のままとする。

## 現状

`src/hpack/huffman.rs` の `decode` 関数内で `HUFFMAN_TABLE.iter().enumerate()` により 257 エントリを毎回線形探索している。RFC 7541 Appendix B の Huffman 符号は正準 Huffman 符号であり（RFC 7541 Appendix B 冒頭に明記されている）、入力長に依存しない固定回数の探索（ルックアップテーブルまたは二分木）でデコード可能。

現在の実装は最悪ケースで O(257 × 入力バイト数) となり、入力長に対して線形ではあるものの定数倍が大きく、DoS 耐性の観点でも改善の余地がある。

既存のテスト (`tests/test_hpack/huffman.rs` / `tests/test_hpack/rfc7541.rs` / `pbt/tests/prop_hpack/main.rs`) は正常系の往復とエンコード長が中心で、EOS 記号・不正パディング・7 ビットを超えるパディングのエラー経路を検証していない。`fuzz/fuzz_targets/fuzz_huffman_decoder.rs` もパニック安全性のみを検証している。また、プロジェクトのテストは `cargo test --workspace` (Makefile / CI) で実行しており、`cargo test -p shiguredo_http2` だけでは `pbt` パッケージの Huffman 往復テストが実行されない。

## 設計方針

- プロダクションコードの変更は `src/hpack/huffman.rs` の `decode` 関数と、そのための探索構造の追加に限る。テストは `tests/test_hpack/huffman.rs` に追加する。エンコーダ (`encode` / `encoded_len` / `encode_to_vec`) のロジックは変更しない。
- RFC 7541 Appendix B の Huffman 符号テーブル（既存の `HUFFMAN_TABLE`）を基に、入力長に依存しない固定回数の探索（ルックアップテーブルまたは二分木）を構築する。最大符号長は 30 ビットなので、二分木なら最大 30 段、ルックアップテーブルなら多段化するなどして固定回数に収める。
- 正準 Huffman 符号の性質を利用し、コード長とシンボルのマッピングによる高速デコードを行う。
- 探索構造はコンパイル時定数として構築し、デコードごとの初期化やロックを避ける。
- 出力 `Vec` は既存どおり `Vec::new()` で構築する（`Vec::with_capacity` を使わない。入力データ由来のサイズで事前確保しない）。
- 公開 API (`huffman::decode` / `encode` / `encoded_len` / `encode_to_vec`) のシグネチャは変更しない。
- 既存の EOS パディング検出と EOS 記号の拒否ロジック（RFC 7541 Section 5.2 の MUST）と、`Error::hpack_error` に渡すエラーメッセージを維持する。

## 完了条件

- デコード処理から `HUFFMAN_TABLE.iter().enumerate()` による 257 エントリの線形探索が消えていること。性能改善の定量評価は必須としない（ベンチマークを追加する場合は criterion を使う）。
- 1 シンボルのデコードが入力長に依存しない固定回数の探索で行われること（全体のデコードは入力長に比例する O(n) のまま）。
- 既存の EOS パディング検出と EOS 記号の拒否ロジックが維持されていること。`tests/test_hpack/huffman.rs` に次の 3 ケースを追加して検証すること:
  - EOS 記号を含む入力がエラーになること
  - EOS の最上位ビットに一致しないパディングがエラーになること
  - 7 ビットを超えるパディングがエラーになること
- `cargo fmt --all -- --check` が通過すること。
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること。
- 既存のテストケースを変更せず、`cargo test --workspace` が全件通過すること。
- `cargo check --manifest-path fuzz/Cargo.toml` が通過すること。
- `CHANGES.md` は CODEBASE.md の指示に従い変更しない。
