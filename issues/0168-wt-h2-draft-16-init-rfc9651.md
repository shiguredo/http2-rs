# WebTransport-Init の Dictionary パーサーを RFC 9651 に対応させる

- Created: 2026-11-03
- Completed: 2026-11-03
- Branch: feature/change-wt-init-rfc9651
- Polished: 2026-11-03

## 目的

WebTransport over HTTP/2 draft 16 が参照する Structured Fields に追従し、正しい未知キー・未知パラメータを含む `WebTransport-Init` を誤って拒否しないようにする。分類は仕様変更への対応とする。

## 仕様差分

- `refs/draft-ietf-webtrans-http2-16.txt` Section 4.3.2 は Dictionary の規範参照を RFC 8941 から RFC 9651 に変更した。
- 未知キーとパラメータを無視する MUST、および既知キー `u`・`bl`・`br` が Integer であるという要件は維持されている。
- `refs/rfc9651.txt` Appendix D は Date・Display String の追加とパース失敗の扱いの明確化を列挙する。Section 4.2.3.1、4.2.9、4.2.10 が新しい型のパースを定義する。

一次資料: [draft-ietf-webtrans-http2-16 Section 4.3.2](https://datatracker.ietf.org/doc/html/draft-ietf-webtrans-http2-16#section-4.3.2)、[RFC 9651](https://www.rfc-editor.org/rfc/rfc9651.html)

## 現状

`src/webtransport/init.rs` の `DictionaryParser::parse_bare_item` は RFC 8941 の型のみを処理し、`@` と `%` を拒否する。公開 API を実行し、以下がすべて `InvalidInput` になることを確認した。

```text
WtInit::parse(b"u=100, x=@0")
WtInit::parse(b"u=100, x=%\"hello\"")
WtInit::parse(b"u=100;x=@0")
```

いずれも RFC 9651 では正しい Dictionary である。新しい型が未知キーやパラメータに使われるだけで、既知キーの値を取得できなくなる。

## 設計方針

- Date と Display String を正しくパース・検証し、未知キー・パラメータ・未知キーの Inner List 内で読み飛ばせるようにする。未知値を利用者向け API に保存する必要はない。
- 既知キーに Date や Display String を指定した場合は Integer 型ではないため拒否する。Date を内部的に整数で読む場合も型を区別する。
- Date の小数・桁数違反、Display String の不正な percent encoding・終端欠落・復号後の不正 UTF-8 を拒否する。percent encoding の 16 進英字が小文字に限られる点も Section 4.2.10 に従う。
- RFC 9651 Section 4.2 の失敗処理を確認し、未知値だからという理由で壊れた Dictionary を受理しない。
- `WtConfig::apply_init_as_peer` の `u`・`bl`・`br` の対応付けやヘッダーのキー集合は変更しない。
- `src/webtransport/protocols.rs` の別ヘッダー用パーサーの汎用化・統合は対象外とする。HTTP/2 draft 16 の当該変更は `WebTransport-Init` の Dictionary 参照の変更である。

## 完了条件

- 上記 3 入力が成功し、`u == Some(100)` になること。
- 新しい型を含む未知キー、既知・未知キーのパラメータ、Inner List を公開 API 経由で検証すること。
- 既知キーの型違反と、新しい型の構文違反を区別して拒否すること。
- `tests/test_webtransport/init.rs` と関連 PBT で正常な構造と境界値を検証し、既存の整数範囲・既知キー・マージの検証も通ること。
- `cargo test -p shiguredo_http2` と `cargo test -p pbt` が通ること。

## 解決方法

- `DictionaryParser::parse_bare_item` に RFC 9651 の Date (`@` + Integer) と Display String (`%"..."`) を追加し、未知キー・未知パラメータ・Inner List 内で構文検証して読み飛ばせるようにした
- Date は `@` + Integer のみを受理し、Decimal・空・非数値を拒否する (Section 4.2.9)
- Display String は `%"` で始まり DQUOTE で終わること、未エンコード文字が SP / VCHAR であること、percent encoding が 0-9 / 小文字 a-f の 2 文字であること、復号後のバイト列が UTF-8 として妥当であることを検証する (Section 4.2.10)
- 既知キー `u` / `bl` / `br` に Date / Display String を指定した場合は Integer 型違反として拒否する。重複キーでも各メンバーの型検査を先に行う
- `SfValue` に `Date` / `DisplayString` を追加して型を区別し、`parse_number` の戻り値を `SfNumber` に絞って Integer / Decimal の取り違えを防ぐ
- モジュール doc・rustdoc・SKILL.md を RFC 9651 / draft 16 に更新した
- `tests/test_webtransport/init.rs` に Date / Display String の正常系 (未知キー・パラメータ・Inner List、未知→既知キーの順序)、境界値、構文違反、既知キーの型違反、重複キーと型違反の組み合わせを追加した
- `pbt/tests/prop_webtransport/init.rs` を新設し、RFC 9651 の全 bare item 型を含む妥当な Dictionary の受理と既知キー抽出、変異型の負例、既知キーの型違反を検証する PBT を追加した。Date / Display String の生成はフラグで追跡して到達を保証する
- `fuzz/fuzz_targets/fuzz_wt_init.rs` を追加し、任意のヘッダー値バイト列で `WtInit::parse` がパニックしないことを検証する
- `CODEBASE.md` の指示に従い `CHANGES.md` は変更していない。RFC 8941 を参照する既存コメントの残りは別途行う仕様参照の同期で更新する
