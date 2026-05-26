# WebTransport draft 注記を充実させる

- Priority: Low
- Created: 2026-05-14
- Model: deepseek-v4-pro
- Completed: 2026-05-26
- Branch: feature/fix-draft-notes-webtransport

## 目的

AGENTS.md の「資料を由来の機能を実装する場合は、根拠資料名、節番号、将来変更される可能性があることをコードコメントで明記すること」に従い、WebTransport 関連の draft 由来コードに「暫定値であり将来変更される可能性がある」注記を追加する。

現状、`src/settings.rs` の WebTransport SETTINGS enum バリアントには draft の節番号参照はあるが、暫定性の注記がない。`src/webtransport/` モジュール全体でも同様。

## 優先度根拠

コードの正確性や動作に影響しないコメント追加。ただし draft が RFC 化される際に変更が必要になる箇所を事前に識別可能にするため、早めに対応すべき。

## 現状

### `src/settings.rs` の WebTransport SETTINGS

`Setting` enum の WebTransport バリアント (WtInitialMaxData, WtInitialMaxStreamDataUni 等) には `draft-ietf-webtrans-http2-14 Section 11.2` の参照があるが、「暫定値であり将来変更される可能性がある」の注記がない。

一方、`src/error.rs` の WebTransport エラーコード (`WebtransportError` 等) にはこの種の注記が存在しており、不整合がある。

### `src/webtransport/` モジュール

- `mod.rs`: モジュールヘッダーに draft 名はあるが「将来変更される可能性がある」の一文がない
- `capsule.rs`: capsule タイプ定数に draft 参照があるが暫定性注記なし
- `flow_control.rs`, `stream.rs`: 暫定仕様に依存する挙動に注記なし

## 設計方針

1. `src/settings.rs` の各 WebTransport SETTINGS バリアントの doc comment に以下を追加:
   ```
   /// 注: この値は draft-ietf-webtrans-http2-14 由来の暫定値であり、
   /// IANA 登録後に変更される可能性がある。
   ```

2. `src/webtransport/mod.rs` のモジュールヘッダー doc comment に以下を追加:
   ```
   //! 注: 本モジュールは draft-ietf-webtrans-http2-14 に基づく実装であり、
   //! draft の改訂や RFC 化に伴い仕様が変更される可能性がある。
   ```

3. `src/webtransport/capsule.rs` の capsule タイプ定数群に同様の暫定性注記を追加

4. `src/webtransport/flow_control.rs`, `src/webtransport/stream.rs` のメソッドのうち draft 固有の挙動に依存するものに節番号と注記を追加

## 変更対象ファイル

- `src/settings.rs`: WebTransport SETTINGS バリアントの doc comment 追加
- `src/webtransport/mod.rs`: モジュールヘッダー追加
- `src/webtransport/capsule.rs`: capsule タイプ定数に注記追加
- `src/webtransport/flow_control.rs`: 該当メソッドに注記追加
- `src/webtransport/stream.rs`: 該当メソッドに注記追加

## 完了条件

- 全 WebTransport SETTINGS 定数に暫定性注記がある
- `src/webtransport/` モジュールヘッダーに draft 由来・変更可能性の注記がある
- capsule タイプ定数に注記がある
- `cargo test --workspace` が通る
- `cargo clippy --workspace --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る

## 解決方法

以下のファイルに draft-ietf-webtrans-http2-14 由来の暫定性注記を追加した:

1. `src/settings.rs`: 6 つの WebTransport SETTINGS バリアント全てに「暫定値であり IANA 登録後に変更される可能性がある」注記を追加。Section 11.2 の参照も全バリアントに統一
2. `src/webtransport/mod.rs`: モジュールヘッダーに「draft 由来の実装であり仕様変更の可能性がある」注記を追加
3. `src/webtransport/capsule.rs`: `capsule_type` モジュールの doc comment に暫定値注記を追加
4. `src/webtransport/flow_control.rs`: モジュールヘッダーに暫定仕様注記を追加。`update_send_max`, `can_accept_stream`, `update_max_streams` メソッドに個別注記を追加
5. `src/webtransport/stream.rs`: `WtStream::update_send_max` メソッドに暫定仕様注記を追加
