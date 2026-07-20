# WebTransport HTTP/2 エラーコード名を draft-15 の WT_* に合わせる

- Priority: Medium
- Created: 2026-07-20
- Polished: 2026-07-20
- Model: Grok 4.5
- Branch: feature/change-wt-error-code-names

## 目的

draft-ietf-webtrans-http2-15 Section 3.4 / Section 11.3 で改名された HTTP/2 エラーコード名（`WT_ERROR` / `WT_STREAM_STATE_ERROR` / `WT_FLOW_CONTROL_ERROR`）に、ドキュメント・`Display`・公開識別子・コメントを揃える。

draft-14 の登録名は `WEBTRANSPORT_ERROR` / `WEBTRANSPORT_STREAM_STATE_ERROR` / `WEBTRANSPORT_FLOW_CONTROL_ERROR`（`refs/draft-ietf-webtrans-http2-14.txt` Section 3.4 / Section 11.3）。

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
   - **同名衝突の注意**: `src/webtransport/error.rs` に `pub struct WtError` が既に存在し、`src/webtransport/mod.rs` で `pub use` されている。`ErrorCode::WtError`（enum variant）と `WtError`（struct）は Rust の名前空間上はコンパイルが通るが、ドキュメント・コードリーディングでの混同リスクがある。リネーム実施時にこの点を認識し、必要に応じて `ErrorCode::WtError` の doc コメントで「`webtransport::WtError` 構造体とは別物」と注記する
3. **wire 値 `0x100`–`0x102`**: 変更しない（IANA 割当まで暫定）
4. `from_u32` / `as_u32` とテストの期待 Display 文字列を更新する
5. **コメント内の旧名参照**: `src/webtransport/mod.rs`、`src/webtransport/flow_control.rs`、`src/webtransport/stream.rs`、`tests/test_webtransport/integration.rs` に `WEBTRANSPORT_STREAM_STATE_ERROR` / `WEBTRANSPORT_FLOW_CONTROL_ERROR` がコメントとして出現する。これらも新名に更新する

0077（tokio-http2 の `Error::WebTransport(WtError)`）とは別レイヤ。本 issue は `shiguredo_http2::ErrorCode` の HTTP/2 エラーコード側。

## スコープ外

- IANA 正式値への更新（未割当）
- Reliable Size / FIN / SETTINGS（0081–0083）
- CLOSE reason を `WT_ERROR` セッションエラーにする意味変更（0085）。本 issue は名前揃えのみ

## 他 issue との関係

- **0074** の後に実施（0074 が `src/error.rs` の doc コメントにある `draft-ietf-webtrans-http2-14` を `draft-ietf-webtrans-http2-15` に機械置換する。0084 が先に実施されると 0074 の置換対象テキストが変わる）
- **0085** より前に実施（0085 のテスト・文言が新名を参照できるようにする）
- **0077**: tokio-http2 の `Error` バリアント追加。本 issue の `ErrorCode` 改名とは独立だが、同時期に触る場合はコンフリクトに注意

## 変更対象ファイル一覧

- `src/error.rs` — バリアント名・Display・doc コメント
- `src/webtransport/mod.rs` — コメント内の旧名参照（L649, L680, L708, L734 付近）
- `src/webtransport/flow_control.rs` — コメント内の旧名参照（L131, L142, L204 付近）
- `src/webtransport/stream.rs` — コメント内の旧名参照（L392 付近）
- `tests/test_error.rs` — `KNOWN_ERROR_CODES` 定数（L22-24）、Display 文字列の直接 assert テスト追加
- `tests/test_webtransport/integration.rs` — コメント内の旧名参照（L77, L362, L398 付近）
- `pbt/tests/prop_error.rs` — `error_code_strategy`（L34-36）
- `CHANGES.md` develop（破壊的変更として `[CHANGE]`）

## 完了条件

- `ErrorCode` の Display が `WT_ERROR` / `WT_STREAM_STATE_ERROR` / `WT_FLOW_CONTROL_ERROR` になる
- Rust バリアントが `WtError` / `WtStreamStateError` / `WtFlowControlError` にリネームされ、ワークスペースの参照が更新されている
- コメント内の旧名（`WEBTRANSPORT_*`）が新名（`WT_*`）に更新されている
- wire 値は `0x100`–`0x102` のまま
- Display 文字列を直接 assert するテスト（例: `assert_eq!(ErrorCode::WtError.to_string(), "WT_ERROR")`）が追加されている
- `cargo test --workspace` / clippy `-D warnings` が通る
- `CHANGES.md` にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 3.4（WT_ERROR / WT_STREAM_STATE_ERROR / WT_FLOW_CONTROL_ERROR）/ Section 11.3（IANA 登録名）
- `refs/draft-ietf-webtrans-http2-14.txt` Section 3.4 / Section 11.3（旧名: WEBTRANSPORT_ERROR 等）
- `src/error.rs` — `ErrorCode::Webtransport*` / `Display`
- `src/webtransport/error.rs` — `WtError` 構造体（同名衝突の注意）
