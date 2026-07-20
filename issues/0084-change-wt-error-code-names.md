# WebTransport HTTP/2 エラーコード名を draft-15 の WT_* に合わせる

- Priority: Medium
- Created: 2026-07-20
- Polished: {Polished}
- Model: Grok 4.5
- Branch: feature/change-wt-error-code-names

## 目的

draft-ietf-webtrans-http2-15 Section 3.4 / Section 11.3 で改名された HTTP/2 エラーコード名（`WT_ERROR` / `WT_STREAM_STATE_ERROR` / `WT_FLOW_CONTROL_ERROR`）に、ドキュメント・`Display`・公開識別子を揃える。

## 優先度根拠

- wire 値は両 draft とも `0xTBD`（実装の暫定値 `0x100`–`0x102` は維持）
- 名前変更は相互運用のバイト列には影響しないが、ログ・デバッグ・仕様照合で旧名のまま残ると draft-15 文書と食い違う
- IANA 登録前の暫定値である点はコメントで明示し続ける

## 現状

`src/error.rs` の `ErrorCode`:

| バリアント | Display | wire |
|-----------|---------|------|
| `WebtransportError` | `WEBTRANSPORT_ERROR` | `0x100` |
| `WebtransportStreamStateError` | `WEBTRANSPORT_STREAM_STATE_ERROR` | `0x101` |
| `WebtransportFlowControlError` | `WEBTRANSPORT_FLOW_CONTROL_ERROR` | `0x102` |

draft-15 の登録名は `WT_ERROR` / `WT_STREAM_STATE_ERROR` / `WT_FLOW_CONTROL_ERROR`。

内部の `WtErrorKind::{StreamStateError, FlowControlError}` は HTTP/2 Error Code の IANA 名とは別層であり、本 issue の主対象外（必要ならコメントのみ）。

## 設計方針

公開 API の破壊を伴う改名範囲を次で固定する:

1. **`Display` / ドキュメントコメント**: 必ず `WT_*` に変更する
2. **Rust バリアント名**: `WebtransportError` → `WtError`、`WebtransportStreamStateError` → `WtStreamStateError`、`WebtransportFlowControlError` → `WtFlowControlError` にリネームする（クレートは未安定想定で破壊的変更を許容。呼び出し箇所をワークスペース全体で置換）
3. **wire 値 `0x100`–`0x102`**: 変更しない（IANA 割当まで暫定）
4. `from_u32` / `as_u32` とテストの期待 Display 文字列を更新する

0077（tokio-http2 の `Error::WebTransport(WtError)`）とは別レイヤ。本 issue は `shiguredo_http2::ErrorCode` の HTTP/2 エラーコード側。

## スコープ外

- IANA 正式値への更新（未割当）
- Reliable Size / FIN / SETTINGS（0081–0083）
- CLOSE reason を `WT_ERROR` セッションエラーにする意味変更（0085）。本 issue は名前揃えのみ

## 他 issue との関係

- **0085** より前に実施（0085 のテスト・文言が新名を参照できるようにする）
- **0077**: tokio-http2 の `Error` バリアント追加。本 issue の `ErrorCode` 改名とは独立だが、同時期に触る場合はコンフリクトに注意

## 変更対象ファイル一覧

- `src/error.rs` — バリアント・Display・doc
- `src/` / `crates/` / `tests/` / `pbt/` の `ErrorCode::Webtransport*` 参照
- `CHANGES.md` develop（破壊的変更として `[CHANGE]`）

## 完了条件

- `ErrorCode` の Display が `WT_ERROR` / `WT_STREAM_STATE_ERROR` / `WT_FLOW_CONTROL_ERROR` になる
- Rust バリアントが `WtError` / `WtStreamStateError` / `WtFlowControlError` にリネームされ、ワークスペースの参照が更新されている
- wire 値は `0x100`–`0x102` のまま
- `cargo test --workspace` / clippy `-D warnings` が通る
- `CHANGES.md` にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 3.4 / Section 11.3
- `src/error.rs` — `ErrorCode::Webtransport*` / `Display`
