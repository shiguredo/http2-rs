# RFC 9297 準拠で varint デコーダーの非最小エンコーディング拒否を解除する

- Created: 2026-06-11
- Priority: Medium
- Polished: 2026-06-14
- Model: deepseek-v4-pro
- Branch: feature/change-rfc9297-allow-non-minimal-varint

## 目的

`src/webtransport/varint.rs` の QUIC 可変長整数デコーダーが非最小エンコーディングを拒否する独自方針を取りやめ、RFC 9297 Section 1.1 に従って非最小エンコーディングも受け入れるようにする。

## 優先度根拠

- `shiguredo_http2` クレートは未リリースの状態で develop ブランチで開発中。RFC 違反挙動 (RFC 9297 が許容している非最小エンコーディングを拒否する) を残したままリリースすると、相互運用性問題を抱えたまま外部に出ることになる
- WebTransport over HTTP/2 (draft-ietf-webtrans-http2-14) は RFC 9297 Capsule Protocol を採用しており、他実装 (例: nghttp3, aioquic 系) が RFC 9297 に従って非最小エンコーディングで Capsule Type / Capsule Length を送信した場合に、本実装はそれを正常な入力として拒否してしまう
- 「Premature Optimization is the Root of All Evil」(CLAUDE.md) の方針上、根拠の薄い「独自方針」「厳格化」は採用しない
- 修正コストは低い (検査ブロック 11 行 + doc コメント数行の削除、既存テスト 1 件の挙動反転)

## 現状の問題

`src/webtransport/varint.rs:192-202` で非最小エンコーディングを拒否している:

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

加えて `src/webtransport/varint.rs:139-140` の doc コメントにも同趣旨が書かれている:

```rust
/// - 非最小エンコーディングの場合 (`InvalidInput`)
///   - RFC 9000 Section 16 はこれを要求しないが、本実装独自の厳格化として拒否する
```

問題点:

- `refs/rfc9297.txt:124-127` (Section 1.1) は明示的に非最小エンコーディングを許容する。原文:

  > Where this document defines protocol types, the definition format uses the notation from Section 1.3 of [QUIC]. Where fields within types are integers, they are encoded using the variable-length integer encoding from Section 16 of [QUIC]. Integer values do not need to be encoded on the minimum number of bytes necessary.

  これは少なくとも RFC 9297 で定義される Capsule Type / Capsule Length に直接適用される。
  `varint::decode` は WebTransport over HTTP/2 の各フィールド (WT_RESET_STREAM の stream_id 等) にも使用されるが、RFC 9000 Section 16 ですでに Frame Type 以外の非最小エンコーディングが許容されているため、本 issue では varint レイヤ全体で非最小エンコーディングを受け入れる。
- RFC 9000 Section 12.4 (`refs/rfc9000.txt:3987-3998`) は QUIC の Frame Type に対してのみ最小エンコーディングを MUST 要求しているが、これは QUIC 内部の frame の話であって RFC 9297 Capsule Type には適用されない
- コードコメントは「本実装独自の厳格化」と書いているが、その根拠 (DoS 耐性なのかコード簡略化なのか) が明示されていない。git log を遡っても明確な意思決定の経緯は見つからない
- 結果として RFC 9297 準拠の他実装が送信する非最小エンコーディングを不正として拒否する相互運用性問題が残る

## 設計方針

- 方針: **RFC 9297 Section 1.1 に従い、非最小エンコーディングを受け入れる**
- DoS 耐性は別レイヤ (フロー制御 / 最大 Capsule サイズ等) で確保する。varint レイヤで非最小拒否を行う必要はない
- encode 側は引き続き常に最小バイト数でエンコードする (送信側として最小を選ぶことに問題はなく、他実装の挙動と同じ)
- `WtErrorKind::InvalidInput` バリアントは削除しない (他で使用中)
- `WtErrorKind::Incomplete` の使用は維持する (varint 入力不足は引き続きエラー)

## スコープ外

- encode 側 (`encode` / `encoded_len` 関数) の挙動変更は行わない
- `varint::decode` 呼び出し元 (`src/webtransport/capsule.rs` の 18 箇所等) のロジック変更は行わない (varint デコード結果を受け取った後の値域チェック等は別問題)
- Capsule Length と payload size の整合性チェックは `capsule.rs` の責務で本 issue とは独立
- `decode` は可変長整数エンコーディングの形式自体が 62 ビット (2^62 - 1) までの値しか表現できないため、上限超過の入力は発生しない。本 issue ではこの動作を変更しない

## 他 issue との関係

- **0068-0071**: いずれも本 issue とは異なるファイル / 異なる関心事を扱う。順序依存なし
- **0072 (`refactor-remove-unused-code`)**: 0072 で削除予定の `WtError::incomplete` ヘルパーは `varint.rs` で使用されておらず、本 issue で削除する `InvalidInput` 生成箇所とも独立。順序依存なし
- **0074 (`change-update-refs-draft-14`)**: 0074 は `src/webtransport/varint.rs` の `draft-ietf-webtrans-http2-14` コメントを機械置換するため、**本 issue マージ後にマージする**のが安全。RFC 9297 Section 1.1 は確定済み RFC で内容は不変
- **0075-0076**: それぞれ無関係

## 変更対象ファイル一覧

### 編集するファイル

- `src/webtransport/varint.rs:139-140` — `decode` 関数 doc コメントの「非最小エンコーディングの場合」項目を削除
- `src/webtransport/varint.rs:192-202` — 非最小エンコーディング検査ブロックの削除
- `src/webtransport/varint.rs:1-11` (モジュール冒頭 doc コメント) — RFC 9297 経由の使用と非最小エンコーディング受容を反映して以下の内容に書き換える:
  ```rust
  //! QUIC 形式の可変長整数エンコーディング (RFC 9000 Section 16)
  //!
  //! 本モジュールは Capsule Protocol (RFC 9297) および WebTransport over HTTP/2
  //! (draft-ietf-webtrans-http2-14) の各フィールドをデコードするために使用される。
  //! RFC 9297 Section 1.1 によれば、整数値は最小バイト数でエンコードする必要がないため、
  //! 本実装は非最小エンコーディングも受け入れる。
  //! 送信側は引き続き最小バイト数でエンコードする。
  //!
  //! 先頭 2 ビットでエンコード長を示す:
  //!
  //! | 2MSB | Length | Usable Bits | Range                    |
  //! |------|--------|-------------|--------------------------|
  //! | 00   | 1      | 6           | 0-63                     |
  //! | 01   | 2      | 14          | 0-16383                  |
  //! | 10   | 4      | 30          | 0-1073741823             |
  //! | 11   | 8      | 62          | 0-4611686018427387903    |
  ```
- `tests/test_webtransport/varint.rs:103-125` — 既存テスト `test_decode_non_minimal_encoding` を 6 ケースの「受入テスト」に書き換える (各 `decode(...)` が `Ok((value, len))` を返し、`value` が期待値と一致することを assert する)。テスト関数名は `test_decode_non_minimal_encoding_accepts_rfc9297` に変更

### 編集不要 (影響なし)

- `src/webtransport/capsule.rs` (`varint::decode` の呼び出し元 18 箇所) — エンコーディング長に依存しない値比較を行っているため変更不要
- `pbt/tests/prop_webtransport/main.rs` の varint 関連 prop — 最小エンコーディング往復のみを確認しており非最小を扱わないため変更不要 (本 issue の効果範囲外)
- `fuzz/fuzz_targets/fuzz_varint_decoder.rs` — 任意バイト列を `decode` に渡すだけで、返り値に対する検証を行っていないため変更不要

## CHANGES.md の扱い

`varint::decode` は draft-ietf-webtrans-http2 対応として develop で追加された未リリース API のため、`shiguredo-changelog` 規約「変更履歴は派生元ブランチとの最終的な差分のみを記載すること」「開発ブランチ内の中間状態の修正は記載しないこと」に従い、最終差分には含まれない (CHANGES.md 編集不要)。

## 対応手順

1. 作業ブランチ `feature/change-rfc9297-allow-non-minimal-varint` を作成する
2. `src/webtransport/varint.rs:192-202` の非最小エンコーディング検査ブロックを削除する
3. `src/webtransport/varint.rs:139-140` の doc コメント `- 非最小エンコーディングの場合 ...` の項目を削除する
4. `src/webtransport/varint.rs:1-11` のモジュール冒頭 doc コメントを、RFC 9297 Section 1.1 経由で使用される旨と非最小エンコーディング許容方針を反映した内容に書き換える
5. `tests/test_webtransport/varint.rs:103-125` の `test_decode_non_minimal_encoding` を「受入テスト」に書き換える:
   - 関数名を `test_decode_non_minimal_encoding_accepts_rfc9297` に変更
   - コメント (line 105) を「RFC 9297 Section 1.1 に従い非最小エンコーディングを受け入れる」に書き換え
   - 既存 6 ケース (`assert!(decode(...).is_err())`) を以下の期待値で受入テスト化する:

     | 入力 | 期待 value | 期待 len |
     |------|------------|----------|
     | `[0x40, 0x0a]` | 10 | 2 |
     | `[0x40, 0x00]` | 0 | 2 |
     | `[0x40, 0x3f]` | 63 | 2 |
     | `[0x80, 0x00, 0x00, 0x40]` | 64 | 4 |
     | `[0x80, 0x00, 0x3f, 0xff]` | 16383 | 4 |
     | `[0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00]` | 16384 | 8 |

     各ケースで `let (value, len) = decode(...).expect("RFC 9297 は非最小エンコーディングを許容する"); assert_eq!(value, <期待値>); assert_eq!(len, <バイト数>);` と検証する
6. `cargo fmt --all -- --check` で整形違反がないことを確認する
7. `cargo build --workspace` でビルドが成功することを確認する
8. `cargo test --workspace` で全テスト通過を確認する (新規受入テストが通ること、既存 `test_rfc_examples` 等が退行しないこと)
9. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
10. `cargo check --manifest-path fuzz/Cargo.toml` で fuzz ターゲットがビルドできることを確認する

## 完了条件

- `src/webtransport/varint.rs:192-202` の非最小エンコーディング検査ブロックが削除されている
- `src/webtransport/varint.rs:139-140` の doc コメント該当項目が削除されている
- `src/webtransport/varint.rs` のモジュール冒頭 doc コメントが RFC 9297 経由の使用と非最小エンコーディング許容を反映している
- `tests/test_webtransport/varint.rs` の `test_decode_non_minimal_encoding` が 6 ケースの受入テストに書き換えられている
- `cargo fmt --all -- --check` が通過する
- `cargo build --workspace` が成功する
- `cargo test --workspace` が成功する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo check --manifest-path fuzz/Cargo.toml` が通過する

## 参照

- `refs/rfc9297.txt:124-127` — RFC 9297 Section 1.1: 「Integer values do not need to be encoded on the minimum number of bytes necessary.」
- `refs/rfc9000.txt:4886-4888` — RFC 9000 Section 16: varint エンコーディング全体の規定 (最小バイト数の要求なし)
- `refs/rfc9000.txt:3987-3998` — RFC 9000 Section 12.4: QUIC Frame Type のみ最小エンコーディング MUST (Capsule Type には適用されない)
- `refs/draft-ietf-webtrans-http2-14.txt` Section 2 (L211-L217) — WebTransport over HTTP/2 が Capsule Protocol (RFC 9297) を使用することを規定
- `src/webtransport/varint.rs:139-202` — 修正対象の doc コメントと検査ブロック
- `src/webtransport/varint.rs:1-11` — モジュール冒頭 doc コメント (修正対象)
- `src/webtransport/capsule.rs` — `varint::decode` の呼び出し元 (Capsule Type / Length / 各種 WT_* フィールド、変更不要)
- `tests/test_webtransport/varint.rs:103-125` — 既存テスト (書き換え対象)
- `pbt/tests/prop_webtransport/main.rs` — varint 関連 prop (影響なし)
