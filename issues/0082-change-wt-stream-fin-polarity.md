# WT_STREAM Capsule の FIN 極性を draft-15 に合わせる

- Priority: High
- Created: 2026-07-20
- Completed: 2026-07-20
- Polished: 2026-07-20
- Model: Grok 4.5
- Branch: feature/change-wt-stream-fin-polarity

## 目的

draft-ietf-webtrans-http2-15 Section 6.4 で記述が逆転した WT_STREAM Capsule の FIN 極性（LSB）に実装を合わせ、draft-15 ピアとのストリームデータ送受信を相互運用可能にする。

## 優先度根拠

- draft-14: 「任意個の `0x190B4D3B` の後に終端 `0x190B4D3C`」
- draft-15: 「任意個の `0x190B4D3C` の後に終端 `0x190B4D3B`」
- Capsule Type の数値範囲 `0x190B4D3B..0x190B4D3C` と「LSB = FIN bit」は両 draft で共通。draft-14 は LSB=FIN の記述と capsule 順序の記述が内部的に矛盾していた（LSB=FIN なら 0x3B の LSB=1 で FIN=1 になるが、順序記述では 0x3B が非終端）。draft-15 は capsule 順序の記述を LSB=FIN に揃え、終端 `0x190B4D3B`（LSB=1=FIN=1）に統一した
- 現実装は draft-14 の順序記述どおり `WT_STREAM=0x190B4D3B` (FIN=0) / `WT_STREAM_FIN=0x190B4D3C` (FIN=1) であり、draft-15 ピアとは wire 非互換になる

## 現状

`src/webtransport/capsule.rs`:

```rust
pub const WT_STREAM: u64 = 0x190B4D3B;     // コメント: FIN=0
pub const WT_STREAM_FIN: u64 = 0x190B4D3C; // コメント: FIN=1
```

- encode: `fin == true` のとき `WT_STREAM_FIN` (0x190B4D3C) を送る
- decode: `capsule_type == WT_STREAM_FIN` のとき `fin = true`
- `src/webtransport/mod.rs` の `send_stream_data` / `handle_stream_data` は `Capsule::WtStream` の enum 経由であり、定数入れ替えの影響を受けない

draft-15 Section 6.4 の文面:

> Stream data consists of any number of 0x190B4D3C capsules followed by a terminal 0x190B4D3B capsule.

LSB=FIN と整合させると、終端 `0x190B4D3B` の LSB が 1 であるため **FIN=1 は 0x190B4D3B**、連続用 `0x190B4D3C` は FIN=0。現定数の意味付けが逆。

## 設計方針

- `capsule_type::WT_STREAM` / `WT_STREAM_FIN` の定数値を入れ替える:
  - `WT_STREAM` = 非終端 (FIN=0) = `0x190B4D3C`
  - `WT_STREAM_FIN` = 終端 (FIN=1) = `0x190B4D3B`
- encode/decode の `fin` 判定ロジックは定数名参照のため、定数値入れ替えだけで自動的に追従する。`Capsule::WtStream { fin, ... }` の公開フィールド意味（`fin: true` = 終端）は維持
- 既存のユニットテスト・PBT・fuzz は `Capsule::WtStream` の enum 経由でラウンドトリップしており、capsule type のハードコードバイト列は存在しない。定数入れ替えだけで既存テストは変更なしで通過する
- **wire バイトの正確性を検証する新規テストを追加する**: `fin=true` で encode した capsule type が varint 表現で `0x190B4D3B` であること、`fin=false` では `0x190B4D3C` であることを直接 assert する（定数入れ替えだけでは「encode/decode が自己整合するが wire 上は draft-15 と不一致」というバグを検出できないため）
- エッジケース: 空データ + `fin=true` の capsule（終端のみでデータなし）、`fin=false` を複数送った後に `fin=true` を送るシーケンス、decode 側で `0x190B4D3B` 受信時に `fin=true` になることの直接検証
- コメント・ドキュメントの draft 参照は draft-15 Section 6.4 に更新する（機械置換全体は 0074）

## スコープ外

- Reliable Size の一致必須（0083）
- エラーコード名変更（0084）
- SETTINGS_WT_ENABLED（0081）

## 他 issue との関係

- **0083**: Reliable Size。FIN 極性修正後のストリーム送受信テストと重なるため、本 issue の後が安全
- **0074**: draft 番号テキスト同期

## 変更対象ファイル一覧

- `src/webtransport/capsule.rs` — `capsule_type::WT_STREAM` / `WT_STREAM_FIN` の定数値入れ替え、コメント更新
- `src/webtransport/mod.rs` — enum 経由のため影響なし（変更不要）
- `tests/test_webtransport/capsule.rs` — wire バイト正確性の新規テスト追加（既存テストは変更不要）
- `crates/tokio-http2/tests/test_webtransport.rs` — enum 経由のため影響なし（変更不要）
- `pbt/tests/prop_webtransport/main.rs` — enum 経由のため影響なし（変更不要）
- `fuzz/fuzz_targets/fuzz_capsule_encoder.rs` / `fuzz_capsule_decoder.rs` — enum 経由・ランダムバイトのため影響なし（変更不要）
- `CHANGES.md` develop

## 完了条件

- draft-15 どおり、非終端 WT_STREAM が `0x190B4D3C`、終端が `0x190B4D3B` で encode/decode される
- `fin=true` で encode した capsule type が varint 表現で `0x190B4D3B` であることを assert するテストが追加されている
- 既存ストリーム送受信テストが新 polarity で通る
- `cargo test --workspace` / clippy `-D warnings` が通る
- `CHANGES.md` にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 6.4（WT_STREAM、LSB=FIN、3C 連続 + 終端 3B）
- `refs/draft-ietf-webtrans-http2-14.txt` Section 6.4（旧: 3B 連続 + 終端 3C。LSB=FIN との内部矛盾あり）
- `src/webtransport/capsule.rs` — `capsule_type::WT_STREAM` / `WT_STREAM_FIN`

## 解決方法

`src/webtransport/capsule.rs` の `capsule_type::WT_STREAM` を 0x190B4D3C (FIN=0, 非終端)、`capsule_type::WT_STREAM_FIN` を 0x190B4D3B (FIN=1, 終端) に変更した。encode/decode ロジックは定数名参照のため、定数値入れ替えだけで自動的に追従する。

`tests/test_webtransport/capsule.rs` に wire バイト正確性のテスト 6 件を追加した: fin=true/false の encode 後 capsule type 直接検証、空データ + fin=true のラウンドトリップ、非終端→終端シーケンス、raw バイトからの decode 検証 (0x190B4D3B → fin=true、0x190B4D3C → fin=false)。
