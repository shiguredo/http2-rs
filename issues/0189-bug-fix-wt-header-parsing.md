# WebTransport-Init / WT-Available-Protocols の RFC 9651 準拠を強化する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-header-parsing
- Polished: {YYYY-MM-DD}

## 目的

`WebTransport-Init` は RFC 9651 Dictionary、`WT-Available-Protocols` は RFC 9651 List of String である。実装は同名ヘッダーの複数行を結合せず、`WebTransport-Init` の重複キーで last-wins を適用せず、`WT-Available-Protocols` の Date / Display String パラメータをパース失敗にし、byte sequence の base64 検証も行わない。draft-ietf-webtrans-http2-16 Section 4.3.2 / Section 3.3 と RFC 9651 に適合させる。

## 現状

- `crates/tokio-http2/src/webtransport.rs` の `WtServerRequest::header` は同名ヘッダーの最初の 1 個だけを返す。RFC 9651 Section 4.2 は全フィールド行をカンマ結合してからパースすることを MUST とする。
- `src/webtransport/init.rs` の `WtInit::parse` は既知キーの型検査を辞書メンバーごとに行い、先行する型違反で即 `Err` を返す。RFC 9651 Section 4.2.2 は同名キーの last-wins を定めるため、`u=@1, u=100` は `u=100` として受理すべきである。既存テスト `test_parse_rejects_type_violation_before_last_wins` が誤った挙動を固定している。
- `src/webtransport/protocols.rs` の `skip_bare_item` は Date (`@`) と Display String (`%"..."`) を扱わず、正当なパラメータでフィールド全体を失敗させる。HTTP/3 draft Section 3.3 はパラメータを MUST 無視とする。
- `init.rs` の `skip_byte_sequence` は base64 をデコードせず文字種だけを見るため、RFC 9651 のパースに失敗する値を受理する。

## 設計方針

- 同名ヘッダーをカンマ結合する共通処理を追加する。
- RFC 9651 パーサーを `init.rs` と `protocols.rs` で共通化し、辞書全体の last-wins、Date / Display String の読み飛ばし、base64 デコード検証を実装する。
- `u=@1, u=100` を受理し、`u=100, u=@1` を拒否するテストに修正する。
- 複数行の `WebTransport-Init` / `WT-Available-Protocols`、Date / Display String パラメータ、不正 base64 のテストを追加する。

## 完了条件

- 同名ヘッダーの複数行が RFC 9651 に従って結合されること。
- `WebTransport-Init` の last-wins が実装され、既存テストの期待値が修正されていること。
- Date / Display String パラメータが無視され、正当なヘッダーが受理されること。
- 不正な base64 がパースエラーになること。
- 上記のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
