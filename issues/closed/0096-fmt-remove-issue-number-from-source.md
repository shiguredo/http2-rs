# ソースコード内の issue 番号参照を削除する

- Created: 2026-07-30
- Completed: 2026-07-31
- Branch: feature/refactor-remove-issue-number-from-source
- Polished: 2026-07-30

## 目的

`src/webtransport/capsule.rs` のコメントに記載されている issue 番号参照を削除し、shiguredo-issues 規約に準拠させる。

## 現状

`src/webtransport/capsule.rs` の `WT_CLOSE_SESSION` デコード処理内のコメントに「0077 完了後は ErrorCode::WtError に対応付ける。」と issue 番号への言及がある。

shiguredo-issues 規約では、ソースコード本体に issue 番号や issue への言及を書いてはいけないと定めている。コードに残したい「なぜ」は issue 番号への参照ではなく理由そのもの（仕様の節番号・再現条件・設計意図）を書くこと。

## 完了条件

- ソースコード内から issue 番号参照が削除されていること
- 必要な場合は理由そのもの（仕様の節番号等）に置き換わっていること

## 解決方法

該当コメントを削除するか、issue 番号を含まない形（例: 「draft-ietf-webtrans-http2-15 Section 6.12: 1024 超または非 UTF-8 は session error WT_ERROR として扱う」）に書き換える。
