# WebTransport-Init / WT-Available-Protocols の RFC 9651 準拠を強化する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-header-parsing
- Polished: 2026-11-17

## 目的

`WebTransport-Init` は RFC 9651 Dictionary、`WT-Available-Protocols` は RFC 9651 List of String である。実装は同名ヘッダーの複数行を結合せず、`WebTransport-Init` の既知キーの型検査を辞書メンバーごとに行うため重複キーの先行する型違反で last-wins 後の値が採用されず、`WT-Available-Protocols` の Date / Display String パラメータをパース失敗にし、byte sequence の base64 検証も行わない。draft-ietf-webtrans-http2-16 Section 4.3.2 / Section 3.3 と RFC 9651 に適合させる。

## 現状

- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::header` は同名ヘッダーの最初の 1 個だけを返し、`webtransport_init()` / `wt_available_protocols()` はその結果を使う。RFC 9651 Section 4.2 は全フィールド行をカンマ結合してからパースすることを MUST とする。
- `src/webtransport/init.rs` の `WtInit::parse` は既知キーの型検査を辞書メンバーごとに行い、先行する型違反で即 `Err` を返す。RFC 9651 Section 4.2.2 は同名キーの last-wins を定めるため、`u=@1, u=100` は `u=100` として受理すべきである。既存テスト `test_parse_rejects_type_violation_before_last_wins` が誤った挙動を固定している。
- `src/webtransport/protocols.rs` の `skip_bare_item` は Date (`@`) と Display String (`%"..."`) を扱わず、正当なパラメータでフィールド全体を失敗させる。HTTP/3 draft Section 3.3 はパラメータを MUST 無視とする。
- `init.rs` の `skip_byte_sequence` と `protocols.rs` の `skip_byte_sequence` は base64 をデコードせず文字種だけを見るため、RFC 9651 のパースに失敗する値を受理する。

## 設計方針

- `WtServerRequest::header` は疑似ヘッダー・Origin 用に現状の `Option<&[u8]>` を維持し、WT ヘッダー専用のカンマ結合 helper を追加する。`webtransport_init()` / `wt_available_protocols()` は同名ヘッダーをカンマ結合した `Option<Vec<u8>>` を返すように変更する (公開 API の破壊的変更)。`accept()` は結合済みの値を使う。
- RFC 9651 パーサーを `init.rs` と `protocols.rs` で共通化し、辞書全体の last-wins、Date / Display String の読み飛ばし、base64 デコード検証を実装する。
- base64 検証は RFC 9651 Section 4.2.7 の SHOULD NOT に従い、`=` padding の省略と非ゼロ pad bits を拒否しない。
- `u=@1, u=100` と `u=%"x", u=100` を受理し、`u=100, u=@1` と `u=100, u=%"x"` を拒否するテストに修正する。既存テスト `test_parse_rejects_type_violation_before_last_wins` は名前と期待値を実装に合わせる。
- `tests/test_webtransport/init.rs` / `tests/test_webtransport/protocols.rs` / `crates/tokio-http2/tests/test_webtransport.rs` に、複数行の `WebTransport-Init` / `WT-Available-Protocols`、Date / Display String パラメータ、不正 base64 のテストを追加する。`crates/tokio-http2/tests/test_webtransport.rs` の既存の `wt_available_protocols()` の比較も新しい戻り値に合わせる。
- `crates/tokio-http2/src/webtransport.rs` の `webtransport_init()` / `wt_available_protocols()` の doc、`src/webtransport/init.rs` の `WtInit::parse` の doc と型検査・base64 検証のコメント、`src/webtransport/protocols.rs` のモジュール doc と `WtAvailableProtocols::parse` / `skip_bare_item` の doc、`pbt/tests/prop_webtransport/init.rs` の base64 に関するコメント、`skills/shiguredo-http2/SKILL.md` の `WtInit::parse` / `WtAvailableProtocols` の記述を実装に合わせて更新する。

## 完了条件

- 同名ヘッダーの複数行が RFC 9651 Section 4.2 に従ってカンマ結合され、`WtServerRequest::webtransport_init()` / `wt_available_protocols()` が結合済みの値を返すこと。
- `WebTransport-Init` の重複キーは辞書全体の last-wins 後に型検査され、`u=@1, u=100` と `u=%"x", u=100` が受理され、`u=100, u=@1` と `u=100, u=%"x"` が拒否されること。既存テスト `test_parse_rejects_type_violation_before_last_wins` の期待値と名前が修正されていること。
- `WT-Available-Protocols` の Date / Display String パラメータが無視され、正当なヘッダーが受理されること。
- 不正な base64 が `WtInit::parse` と `WtAvailableProtocols::parse` でパースエラーになること。
- 上記のテストが `tests/test_webtransport/init.rs` / `tests/test_webtransport/protocols.rs` / `crates/tokio-http2/tests/test_webtransport.rs` に追加されていること。
- `crates/tokio-http2/src/webtransport.rs` の `webtransport_init()` / `wt_available_protocols()` の doc、`src/webtransport/init.rs` の `WtInit::parse` の doc と型検査・base64 検証のコメント、`src/webtransport/protocols.rs` のモジュール doc と `WtAvailableProtocols::parse` / `skip_bare_item` の doc、`pbt/tests/prop_webtransport/init.rs` の base64 に関するコメント、`skills/shiguredo-http2/SKILL.md` の `WtInit::parse` / `WtAvailableProtocols` の記述が実装と一致していること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
