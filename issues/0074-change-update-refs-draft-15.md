# refs/ の WebTransport draft を -14 から -15 に更新する

- Priority: Medium
- Created: 2026-06-11
- Polished: {Polished}
- Model: deepseek-v4-pro
- Branch: feature/change-update-refs-draft-15

## 目的

`skills/shiguredo-http2/SKILL.md` が draft-15 を参照している一方、`refs/` には draft-14 が存在し、全ソースコードコメントも draft-14 を参照しているバージョン不一致を解消する。

## 現状の問題

- `skills/shiguredo-http2/SKILL.md:15`: "WebTransport over HTTP/2: draft-ietf-webtrans-http2-15 に対応"
- `refs/draft-ietf-webtrans-http2-14.txt`: -14 の本文
- 全ソースコードコメント: `draft-ietf-webtrans-http2-14`

バージョン不一致により:
- -15 で仕様が変更された場合（セクション番号、capsule type 値、エラーコード、MUST/SHOULD/MAY 要件）、コードが古い仕様に準拠したままになるリスクがある
- 特に draft-14 の暫定 capsule type 値 (`0x190B4D3D` 等) が IANA 登録で変更されている可能性がある

## 完了条件

- `refs/` に最新の draft-ietf-webtrans-http2（-15 以降）が収録されていること
- -15 で追加・変更された項目を全件確認し、必要に応じてコードを修正すること:
  - Section 番号の変動
  - Capsule type 値の変動
  - 新しい MUST/SHOULD/MAY 要件の追加
  - WT_CLOSE_SESSION reason の上限 (1024) の変更有無
  - SETTINGS_WT_MAX_SESSIONS の追加有無
  - WebTransport-Init ヘッダーのフォーマット変更
- 全ソースコードコメントの `draft-ietf-webtrans-http2-14` を `-15`（または最新版）に更新すること
- スキルファイルの draft バージョンと refs/ とコードコメントが一致していること
- CHANGES.md `## develop` に `[UPDATE]` エントリを追加すること

## 解決方法

1. IETF Datatracker から最新の `draft-ietf-webtrans-http2` を取得し `refs/` に配置
2. 旧 draft-14 ファイルを削除
3. draft-15 と draft-14 の diff を取得し、変更点を列挙
4. 変更点ごとにコード修正の要否を判断
5. 全コメントの draft バージョン番号を更新

注: draft の取得には `update-refs` スキルが利用可能。

## 参照

- `refs/draft-ietf-webtrans-http2-14.txt` — 更新対象
- `skills/shiguredo-http2/SKILL.md:15` — draft-15 を参照
- `src/webtransport/mod.rs:1` — draft-14 を参照（例）
- `src/webtransport/capsule.rs:1` — draft-14 を参照（例）
