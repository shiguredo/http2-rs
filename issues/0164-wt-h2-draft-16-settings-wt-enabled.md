# SETTINGS_WT_ENABLED の識別子を draft 16 の 0x2b67 に変更する

- Created: 2026-11-03
- Completed: {YYYY-MM-DD}
- Branch: feature/change-wt-enabled-draft16-codepoint
- Polished: 2026-11-03

## 目的

WebTransport over HTTP/2 draft 16 のバージョン識別に追従し、draft 16 のピアと WebTransport 対応の有無を正しく判定できるようにする。分類は後方互換のない仕様変更とする。

## 仕様差分

- `refs/draft-ietf-webtrans-http2-16.txt` Section 11.2 は、`SETTINGS_WT_ENABLED` の識別子を draft 15 の `0x2b60` から `0x2b67` に変更している。
- 新設された Section 9.1 は、wire format またはプロトコル動作に互換性のない変更を加える draft で、この設定の識別子を変更する方針を定めている。
- Section 3.1 の値域、初期値 0、サーバーによるサポート通知という意味は変わっていない。`SETTINGS_WT_INITIAL_MAX_*` の識別子 `0x2b61`〜`0x2b66` も変更されていない。

一次資料: [draft-ietf-webtrans-http2-16](https://www.ietf.org/archive/id/draft-ietf-webtrans-http2-16.txt)

## 現状

`src/settings.rs` の `Setting::from_wire` と `Setting::as_wire` は `0x2b60` を使用する。公開 API を実行し、以下を確認した。

- `Setting::from_wire(0x2b67, 1)` は `Setting::Unknown` になる。
- `Setting::from_wire(0x2b60, 1)` は `Setting::WtEnabled(true)` になる。

このままでは draft 16 のサーバーによる通知を認識できず、draft 15 の識別子を draft 16 のサポートと誤認する。

## 設計方針

- `Setting::WtEnabled` のエンコード・デコードを `0x2b67` に変更する。
- draft 15 との複数バージョン対応は追加しない。`0x2b60` は未知の SETTINGS として扱い、`wt_enabled` を変更しない。旧識別子の値 2 も boolean の値域違反として拒否しない。
- `tests/test_settings.rs` と `pbt/tests/prop_connection/settings.rs` の raw wire 入力を更新する。
- `pbt/tests/prop_settings.rs` の `sample_invalid_wt_enabled_wire` も `0x2b67` を生成するように変更し、`prop_invalid_wt_enabled_rejected` が新識別子の値域違反を検証するようにする。
- `pbt/tests/prop_settings.rs` の未知 ID サンプラーを修正する。既知 ID の連続範囲が `0x2b60`〜`0x2b66` から `0x2b61`〜`0x2b67` に移るため、`0x2b67` を未知 ID として生成せず、`0x2b60` を未知 ID として検証する。
- `src/limits.rs`、`README.md`、`skills/shiguredo-http2/SKILL.md` の設定識別子の説明もこの変更と同期する。
- HTTP/3 の設定識別子や、その draft バージョン交渉の処理は持ち込まない。

## 完了条件

- `Setting::WtEnabled(false/true)` の wire 識別子が `0x2b67` であり、新識別子では値 0・1 のみを受理すること。
- 旧識別子を含む SETTINGS を受信しても WebTransport サポートを有効化しないこと。
- 実際の HTTP/2 SETTINGS の送受信経路と PBT で新旧識別子の扱いを検証すること。自分自身のエンコーダーとデコーダーの往復だけで済ませないこと。
- `cargo test -p shiguredo_http2` と `cargo test -p pbt` が通ること。
