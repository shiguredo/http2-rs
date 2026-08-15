# テストコメント内の issue 番号参照を削除する

- Created: 2026-08-10
- Completed: 2026-08-15
- Branch: feature/refactor-remove-issue-number-from-test
- Polished: {YYYY-MM-DD}

## 目的

`tests/test_connection.rs` のテストコメントに issue 番号への言及が残っており、shiguredo-issues 規約 (ソースコード・docstring・コメント・テストコメントに issue 番号を書かない。書ける場所は `issues/` 配下・git コミットメッセージ・GitHub の PR / Issue 本文のみ) に違反する。issue 番号参照を理由そのものに置き換える。

## 現状

`tests/test_connection.rs` の `test_no_content_padding_only_data_accepted` のコメントに「(0102 の残課題であり、本テストではウィンドウ消費量の検証は対象外)」と issue 番号への言及がある。コードに残したい「なぜ」は issue 番号ではなく理由そのもの (パディング分の接続ウィンドウ消費量がアプリに通知されない) を書くべきであり、この言及は規約違反にあたる。

## 設計方針

- 該当コメントの issue 番号参照を、理由そのものの説明に置き換える (例: 「パディングのみ DATA の接続ウィンドウ消費量はアプリに通知されず、本テストではウィンドウ消費量の検証は対象外」)
- コード変更・テスト内容の変更は行わない

## 完了条件

- `tests/test_connection.rs` の `test_no_content_padding_only_data_accepted` のコメントから issue 番号参照が除去され、理由そのものに置き換わる
- テスト内容は変更されず、`cargo test --workspace` が通る

## 参照

- `tests/test_connection.rs` — `test_no_content_padding_only_data_accepted`
- `issues/closed/0096-fmt-remove-issue-number-from-source.md` — 同種の issue 番号参照削除の先行事例

## 解決方法

PR #88 で対応した。

- `tests/test_connection.rs` の `test_no_content_padding_only_data_accepted` のコメントから「(0102 の残課題であり、本テストではウィンドウ消費量の検証は対象外)」を除去した
- 置き換え後は「パディング分の接続ウィンドウ消費量はアプリに通知されず、本テストではウィンドウ消費量の検証は対象外」という理由そのものの説明になっている
- テスト内容は変更せず、`cargo test --workspace` が通ることを確認した
