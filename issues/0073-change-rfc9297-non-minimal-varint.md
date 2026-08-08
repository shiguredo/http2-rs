# RFC 9297 準拠で varint デコーダーの非最小エンコーディング拒否を解除する

- Priority: Medium
- Created: 2026-06-11
- Polished: 2026-08-08
- Model: deepseek-v4-pro
- Branch: feature/change-rfc9297-allow-non-minimal-varint

## 目的

`src/webtransport/varint.rs` の QUIC 可変長整数デコーダーが非最小エンコーディングを拒否する独自方針を取りやめ、RFC 9297 Section 1.1 に従って非最小エンコーディングも受け入れるようにする。

## 優先度根拠

- `varint::decode` は canary.0 から公開済みの公開 API であり、RFC 9297 が許容する非最小エンコーディングを拒否する根拠のない独自の厳格化 (仕様上正当な入力との相互運用性問題) を残したまま正式リリースすると、相互運用性問題を抱えたまま外部に出ることになる
- RFC 9297 Section 1.1 は非最小エンコーディングを明示的に許容しており、RFC 9297 には QUIC の Frame Type に対する RFC 9000 Section 12.4 のような「長いエンコーディングを受信したら拒否してよい (MAY)」という条項が存在しない。つまり非最小エンコーディングの受信は仕様上正当な入力であり、本実装の拒否は RFC 9297 に根拠のない独自の厳格化である (他実装の具体的な挙動に依存しない仕様上の根拠)
- 「Premature Optimization is the Root of All Evil」(CLAUDE.md) の方針上、根拠の薄い「独自方針」「厳格化」は採用しない
- 修正コストは低い (検査ブロック 11 行 + doc コメント数行の削除、既存テスト 1 件の挙動反転)

## 現状の問題

`src/webtransport/varint.rs` の `decode` 関数内で非最小エンコーディングを拒否している:

```rust
// RFC 9000 Section 16 は Frame Type を除き最小エンコーディングを要求しないが、
// 本実装は独自方針として非最小エンコーディングを拒否する
if encoded_len(value) != len {
    return Err(WtError::with_reason(
        WtErrorKind::InvalidInput,
        format!(
            "non-minimal varint encoding: value {value} encoded in {len} bytes, minimum is {}",
            encoded_len(value)
        ),
    ));
}
```

加えて `src/webtransport/varint.rs` の `decode` 関数 doc コメントにも同趣旨が書かれている:

```rust
/// - 非最小エンコーディングの場合 (`InvalidInput`)
///   - RFC 9000 Section 16 はこれを要求しないが、本実装独自の厳格化として拒否する
```

問題点:

- RFC 9297 Section 1.1 は明示的に非最小エンコーディングを許容する。原文:

  > Where this document defines protocol types, the definition format uses the notation from Section 1.3 of [QUIC]. Where fields within types are integers, they are encoded using the variable-length integer encoding from Section 16 of [QUIC]. Integer values do not need to be encoded on the minimum number of bytes necessary.

  これは RFC 9297 で定義される protocol type (Capsule Type / Capsule Length 等) に適用される。WT_RESET_STREAM の stream_id 等の draft-ietf-webtrans-http2-15 が定義するフィールドは RFC 9000 Section 16 の varint エンコーディングで定義されており、RFC 9000 Section 16 が「Values do not need to be encoded on the minimum number of bytes necessary, with the sole exception of the Frame Type field」と非最小を許容するため、`varint::decode` を経由する全フィールドで非最小エンコーディングが正当な入力となる
- RFC 9000 Section 12.4 は QUIC の Frame Type に対してのみ最小エンコーディングを MUST 要求し、「An endpoint MAY treat the receipt of a frame type that uses a longer encoding than necessary as a connection error of type PROTOCOL_VIOLATION」と受信側の拒否権 (MAY) まで規定しているが、これは QUIC 内部の frame の話であって RFC 9297 Capsule Type には適用されない。RFC 9297 にはこのような受信側拒否を許可する条項が存在しない (RFC 9297 Section 3.3 の「redundant length encodings MUST be verified to be self-consistent」は長さ値の自己整合性要件であり、最小エンコーディング要求ではない)
- コードコメントは「本実装独自の厳格化」と書いているが、その根拠 (DoS 耐性なのかコード簡略化なのか) が明示されていない。git log を遡ると、初期実装の doc コメントは「RFC 9000 Section 16: 値は最小バイト数でエンコードされなければならない」という RFC 9000 の誤読が由来であり、後に現在の「本実装独自の厳格化」表記に修正されている
- 結果として RFC 9297 の仕様上正当な非最小エンコーディングを不正として拒否する相互運用性問題が残る

## 設計方針

- 方針: **RFC 9297 Section 1.1 に従い、非最小エンコーディングを受け入れる**
- DoS 耐性は受信バッファ上限 (`CapsuleDecoder::with_max_buffer_size`、デフォルト 16 MiB) 等の別レイヤで確保する。varint レイヤで非最小拒否を行う必要はない (非最小 varint による増幅はカプセルヘッダーあたり最大 7 バイトで DoS リスクは限定的)
- encode 側は引き続き常に最小バイト数でエンコードする (送信側として最小を選ぶことに問題はなく、他実装の挙動と同じ)
- `WtErrorKind::InvalidInput` バリアントは削除しない (他で使用中)
- `WtErrorKind::Incomplete` の使用は維持する (varint 入力不足は引き続きエラー)

## スコープ外

- encode 側 (`encode` / `encoded_len` 関数) の挙動変更は行わない
- `varint::decode` 呼び出し元 (`src/webtransport/capsule.rs` の 18 箇所等) のロジック変更は行わない (varint デコード結果を受け取った後の値域チェック等は別問題)
- Capsule Length と payload size の整合性チェックは `capsule.rs` の責務で本 issue とは独立
- 値の最大値 (62 ビット = `2^62 - 1`) の上限は `decode` の 8 バイトエンコーディングの構造 (先頭 2 ビットが長さ、残り 62 ビットが値) により保証されており、追加の検査は不要。`encode` 側の `value > MAX_VALUE` 検査は本 issue のスコープ外で変更しない

## 他 issue との関係

- **0068-0071**: いずれも本 issue とは異なるファイル / 異なる関心事を扱う。順序依存なし。ただし 0068 (`[FIX]` 追加) / 0070 (`[CHANGE]` 2 件追加) / 0071 (`[CHANGE]` 追加) は `CHANGES.md` を編集するため、マージ順序によってはコンフリクトの可能性がある (内容は異なる箇所なので 3-way merge で解決できる見込み)
- **0072 (`change-remove-unused-code`)**: 0072 で削除予定の `WtError::incomplete` ヘルパーは `varint.rs` で使用されておらず、本 issue で削除する `InvalidInput` 生成箇所とも独立。`CHANGES.md` を編集するため、コンフリクトの可能性がある (内容は異なる箇所なので 3-way merge で解決できる見込み)
- **0074 (`change-update-refs-draft-15`)**: 0074 で `refs/draft-ietf-webtrans-http2-15.txt` に更新されても、本 issue が依拠する RFC 9297 Section 1.1 は確定済み RFC で内容は不変。順序依存なし (0074 は closed 済み)
- **0076 / 0078 / 0102 / 0103**: それぞれ `CHANGES.md` を編集するため、コンフリクトの可能性がある (内容は異なる箇所なので 3-way merge で解決できる見込み)

## 変更対象ファイル一覧

### 編集するファイル

- `src/webtransport/varint.rs` の `decode` 関数 doc コメント — 「非最小エンコーディングの場合」項目を削除
- `src/webtransport/varint.rs` の `decode` 関数内 — 非最小エンコーディング検査ブロック (`if encoded_len(value) != len { ... }`) を削除
- `src/webtransport/varint.rs` のモジュール冒頭 doc コメント — RFC 9000 Section 16 で定義され RFC 9297 経由で使用される旨を追記 (例: 「RFC 9000 Section 16 で定義され、RFC 9297 Section 1.1 で非最小エンコーディングが許容される。本実装も非最小エンコーディングを受け入れる」)
- `tests/test_webtransport/varint.rs` の `test_decode_non_minimal_encoding` — 6 ケースの「受入テスト」に書き換える (各 `decode(...)` が `Ok((value, len))` を返し、`value` が期待値と一致することを assert する)。テスト関数名は `test_decode_non_minimal_encoding_accepts_rfc9297` 等に変更
- `CHANGES.md` — `varint::decode` の挙動変更を記載した `[CHANGE]` エントリ追加

### 編集不要 (影響なし)

- `src/webtransport/capsule.rs` (`varint::decode` の呼び出し元 18 箇所) — デコード結果の (値, 消費バイト数) にのみ依存して処理しており、エンコーディング長の最小性には依存しないため変更不要 (非最小エンコーディングでも decode は実際の消費バイト数を返すため、パース結果は不変)
- `pbt/tests/prop_webtransport/main.rs` の varint 関連 prop — 最小エンコーディング往復のみを確認しており非最小を扱わないため変更不要 (本 issue の効果範囲外)
- `fuzz/fuzz_targets/fuzz_varint_decoder.rs` — 任意バイト列を `varint::decode` に流すのみで、拒否挙動に依存しないため変更不要

## CHANGES.md の扱い

`varint::decode` は canary.0 から公開済みの公開 API のため、挙動変更は `[CHANGE]` エントリとして `## develop` セクションに記載する。

- `## develop` セクションの既存 `[CHANGE]` 群の先頭 (リポジトリの慣習どおり新しいエントリを上に置く) に以下のエントリを追加する:

   ```markdown
   - [CHANGE] `varint::decode` が非最小エンコーディングを拒否していた独自方針を取りやめ、RFC 9297 Section 1.1 / RFC 9000 Section 16 に従って受け入れるように変更する。`encode` 側は引き続き最小バイト数でエンコードする
     - @voluntas
   ```

## 対応手順

1. 作業ブランチ `feature/change-rfc9297-allow-non-minimal-varint` を作成する
2. `src/webtransport/varint.rs` の `decode` 関数内にある非最小エンコーディング検査ブロック (`if encoded_len(value) != len { ... }`) を削除する
3. `src/webtransport/varint.rs` の `decode` 関数 doc コメントにある「非最小エンコーディングの場合」項目を削除する
4. `src/webtransport/varint.rs` のモジュール冒頭 doc コメントを、RFC 9000 Section 16 で定義され RFC 9297 経由で使用される旨と非最小エンコーディング許容方針を反映した内容に書き換える (例: 「RFC 9000 Section 16 で定義され、RFC 9297 Section 1.1 で非最小エンコーディングが許容される。本実装も非最小エンコーディングを受け入れる」)
5. `tests/test_webtransport/varint.rs` の `test_decode_non_minimal_encoding` を「受入テスト」に書き換える:
   - 関数名を `test_decode_non_minimal_encoding_accepts_rfc9297` に変更
   - 既存 6 ケース (`assert!(decode(...).is_err())`) を `let (value, len) = decode(...).expect("RFC 9297 は非最小エンコーディングを許容する"); assert_eq!(value, <期待値>); assert_eq!(len, <バイト数>);` 形式に書き換え。各ケースの期待値は以下のとおり:
     - `[0x40, 0x0a]` → (10, 2) (値 10 を 2 バイト、最小は 1 バイト)
     - `[0x40, 0x00]` → (0, 2) (値 0 を 2 バイト)
     - `[0x40, 0x3f]` → (63, 2) (値 63 を 2 バイト)
     - `[0x80, 0x00, 0x00, 0x40]` → (64, 4) (値 64 を 4 バイト、最小は 2 バイト)
     - `[0x80, 0x00, 0x3f, 0xff]` → (16383, 4) (値 16383 を 4 バイト)
     - `[0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00]` → (16384, 8) (値 16384 を 8 バイト、最小は 4 バイト)
   - コメントを「RFC 9297 Section 1.1 に従い非最小エンコーディングを受け入れる」に書き換え
6. `CHANGES.md` の `## develop` セクション内の既存 `[CHANGE]` 群の先頭に「CHANGES.md の扱い」で示した `[CHANGE]` エントリと担当者行を追加する
7. `cargo fmt --all -- --check` で整形違反がないことを確認する
8. `cargo build --workspace` でビルドが成功することを確認する
9. `cargo test --workspace` で全テスト通過を確認する (新規受入テストが通ること、既存 `test_rfc_examples` 等が退行しないこと)
10. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
11. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `src/webtransport/varint.rs` の `decode` 関数内の非最小エンコーディング検査ブロックが削除されている
- `src/webtransport/varint.rs` の `decode` 関数 doc コメントの該当項目が削除されている
- `src/webtransport/varint.rs` のモジュール冒頭 doc コメントが RFC 9297 経由の使用と非最小エンコーディング許容を反映している
- `tests/test_webtransport/varint.rs` の `test_decode_non_minimal_encoding` が `test_decode_non_minimal_encoding_accepts_rfc9297` にリネームされ、6 ケースの受入テストに書き換えられている
- `CHANGES.md` の `## develop` の `[CHANGE]` 群先頭に `varint::decode` の挙動変更を記載した `[CHANGE]` エントリと担当者行が追加されている
- `cargo fmt --all -- --check` が通過する
- `cargo build --workspace` が成功する
- `cargo test --workspace` が成功する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `refs/rfc9297.txt` Section 1.1 — 「Integer values do not need to be encoded on the minimum number of bytes necessary.」(RFC 9297 で定義される protocol type に適用)
- `refs/rfc9297.txt` Section 3.2 — Capsule Type / Capsule Length の varint エンコーディング定義
- `refs/rfc9297.txt` Section 3.3 — 「redundant length encodings MUST be verified to be self-consistent」(長さ値の自己整合性要件であり、最小エンコーディング要求ではない)
- `refs/rfc9000.txt` Section 16 — varint エンコーディング全体の規定。「Values do not need to be encoded on the minimum number of bytes necessary, with the sole exception of the Frame Type field」(draft-15 が定義する WT_* フィールドにも適用)
- `refs/rfc9000.txt` Section 12.4 — QUIC Frame Type のみ最小エンコーディング MUST + 受信側の拒否権 MAY (Capsule Type には適用されない)
- `refs/draft-ietf-webtrans-http2-15.txt` Section 2 / Section 6 — WebTransport over HTTP/2 が Capsule Protocol (RFC 9297) を使用することを規定
- `src/webtransport/varint.rs` の `decode` 関数 — 修正対象の doc コメントと検査ブロック
- `src/webtransport/varint.rs` のモジュール冒頭 doc コメント — 修正対象
- `src/webtransport/capsule.rs` — `varint::decode` の呼び出し元 (Capsule Type / Length / 各種 WT_* フィールド、変更不要)
- `tests/test_webtransport/varint.rs` の `test_decode_non_minimal_encoding` — 書き換え対象
- `pbt/tests/prop_webtransport/main.rs` — varint 関連 prop (影響なし)
