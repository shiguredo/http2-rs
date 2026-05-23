# `concatenate_cookies` と `EmptyPath` scheme 依存判定の PBT を追加する

Created: 2026-05-23
Model: Opus 4.7

## 内容

issue 0024 の /review-diff-code で指摘された以下 2 点のテスト不足を解消する。

1. `src/connection/mod.rs::concatenate_cookies` の空 cookie 除外ロジックを検証する PBT / 単体テストが存在しない (MAJ-3)
2. `src/validation.rs::EmptyPath` の scheme 依存判定 (http/https のみ拒否、その他は許容) を検証する PBT が存在しない (MAJ-2)

## 設計方針

### concatenate_cookies

- `pbt/tests/prop_connection.rs` または新規 `tests/test_concatenate_cookies.rs` に以下の property を追加:
  - 任意数 (0..=8) の cookie + 任意数の非 cookie ヘッダー mix で連結結果を検証
  - 全 cookie が空のとき連結後 cookie ヘッダーが含まれない
  - 1 件以上の空 cookie 混在で `";  ;"` 等の二重区切りが発生しないこと
  - sensitive フラグの伝播 (いずれか sensitive なら結合結果も sensitive)

### EmptyPath scheme 依存

- `pbt/tests/prop_validation.rs` に以下の property を追加:
  - `:scheme = http`, `:path = ""` → `Err(EmptyPath)`
  - `:scheme = https`, `:path = ""` → `Err(EmptyPath)`
  - `:scheme = HTTP` (大文字), `:path = ""` → `Err(EmptyPath)` (eq_ignore_ascii_case 動作確認)
  - `:scheme = ftp` 等カスタム, `:path = ""` → `Ok`

## 完了条件

- [ ] `concatenate_cookies` の空 cookie 除外を検証する PBT / 単体テストが存在する
- [ ] `EmptyPath` の scheme 依存判定を検証する PBT が 4 分岐 (http/https/HTTP/ftp) で存在する
- [ ] 既存の全テスト・PBT・fuzz が通る
- [ ] CHANGES.md `### misc` に変更を追記

## 依存

なし
