# WT_STREAM Capsule の FIN 極性を draft-15 に合わせる

- Priority: High
- Created: 2026-07-20
- Polished: {Polished}
- Model: Grok 4.5
- Branch: feature/change-wt-stream-fin-polarity

## 目的

draft-ietf-webtrans-http2-15 Section 6.4 で記述が逆転した WT_STREAM Capsule の FIN 極性（LSB）に実装を合わせ、draft-15 ピアとのストリームデータ送受信を相互運用可能にする。

## 優先度根拠

- draft-14: 「任意個の `0x190B4D3B` の後に終端 `0x190B4D3C`」
- draft-15: 「任意個の `0x190B4D3C` の後に終端 `0x190B4D3B`」
- Capsule Type の数値範囲 `0x190B4D3B..0x190B4D3C` と「LSB = FIN bit」は共通だが、**どちらが FIN=1 か**の叙述が逆になった。現実装は draft-14 叙述どおり `WT_STREAM=0x190B4D3B` (FIN=0) / `WT_STREAM_FIN=0x190B4D3C` (FIN=1) であり、draft-15 ピアとは wire 非互換になる

## 現状

`src/webtransport/capsule.rs`:

```rust
pub const WT_STREAM: u64 = 0x190B4D3B;     // コメント: FIN=0
pub const WT_STREAM_FIN: u64 = 0x190B4D3C; // コメント: FIN=1
```

- encode: `fin == true` のとき `WT_STREAM_FIN` (0x190B4D3C) を送る
- decode: `capsule_type == WT_STREAM_FIN` のとき `fin = true`
- テスト (`tests/test_webtransport/capsule.rs` 等)・fuzz・pbt も同じ前提

draft-15 Section 6.4 の文面:

> Stream data consists of any number of 0x190B4D3C capsules followed by a terminal 0x190B4D3B capsule.

LSB=FIN と整合させると、終端 `0x190B4D3B` の LSB が 1 であるため **FIN=1 は 0x190B4D3B**、連続用 `0x190B4D3C` は FIN=0。現定数の意味付けが逆。

## 設計方針

- `capsule_type::WT_STREAM` / `WT_STREAM_FIN` の定数値を入れ替えるか、定数名の意味を draft-15 に合わせて再定義する
  - 推奨: `WT_STREAM` = 非終端 (FIN=0) = `0x190B4D3C`、`WT_STREAM_FIN` = 終端 (FIN=1) = `0x190B4D3B` とし、encode/decode の `fin` 判定をそれに合わせる
- `Capsule::WtStream { fin, ... }` の公開フィールド意味（`fin: true` = 終端）は維持し、wire マッピングだけ直す
- ユニットテスト・統合テスト・pbt・fuzz の期待バイト列を一斉更新する
- コメント・ドキュメントの draft 参照は draft-15 Section 6.4 に更新する（機械置換全体は 0074）

## スコープ外

- Reliable Size の一致必須（0083）
- エラーコード名変更（0084）
- SETTINGS_WT_ENABLED（0081）

## 他 issue との関係

- **0083**: Reliable Size。FIN 極性修正後のストリーム送受信テストと重なるため、本 issue の後が安全
- **0074**: draft 番号テキスト同期

## 変更対象ファイル一覧

- `src/webtransport/capsule.rs` — 定数・encode/decode
- `tests/test_webtransport/capsule.rs` / `integration.rs` 等
- `pbt/tests/prop_webtransport/` / `fuzz/fuzz_targets/fuzz_capsule_*`
- `CHANGES.md` develop

## 完了条件

- draft-15 どおり、非終端 WT_STREAM が `0x190B4D3C`、終端が `0x190B4D3B` で encode/decode される
- 既存ストリーム送受信テストが新 polarity で通る
- `cargo test --workspace` / clippy `-D warnings` が通る
- `CHANGES.md` にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 6.4（WT_STREAM、LSB=FIN、3C 連続 + 終端 3B）
- `refs/draft-ietf-webtrans-http2-14.txt` Section 6.4（旧: 3B 連続 + 終端 3C）
- `src/webtransport/capsule.rs` — `capsule_type::WT_STREAM` / `WT_STREAM_FIN`
