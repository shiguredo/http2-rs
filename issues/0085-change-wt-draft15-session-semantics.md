# draft-15 のセッション意味論（CLOSE / Origin / Max Streams）に追従する

- Priority: Medium
- Created: 2026-07-20
- Polished: {Polished}
- Model: Grok 4.5
- Branch: feature/change-wt-draft15-session-semantics

## 目的

draft-ietf-webtrans-http2-15 で変わったセッション周辺の MUST/SHOULD を実装に反映する。対象は WT_CLOSE_SESSION の reason 扱い、Origin 検証条件、Maximum Streams 上限超過時のエラー、およびリソース非対応時の 405 ガイダンス。

## 優先度根拠

いずれも相互運用・仕様準拠に関わるが、SETTINGS / FIN / Reliable Size（0081–0083）ほどの wire 破壊ではないため Medium。まとめて 1 issue にする理由は、変更箇所がセッション確立・終了・フロー制御上限の「意味論」に閉じ、単一ブランチでレビューしやすいため。

## 現状

### WT_CLOSE_SESSION reason

- `src/webtransport/mod.rs` の `close()`: 1024 バイト超は **エラー返却**（切り詰めない）
- `src/webtransport/capsule.rs` decode: 1024 超・非 UTF-8 は `capsule_decode` エラー
- draft-15: 送信側はアプリ供給メッセージを切り詰める場合 **UTF-8 文字境界で MUST**。受信側は 1024 超または非 UTF-8 を session error **`WT_ERROR`** として扱う MUST

### Origin

- `crates/tokio-http2/src/webtransport.rs` の `accept(..., allowed_origin)`:
  - `allowed_origin=Some` かつ Origin **欠落** → 403 を送らず `InvalidArgument` のみ
  - 不一致 → `:status=403`
- draft-15 Section 3.2: **Origin ヘッダーがある場合**に MUST verify。失敗は SHOULD 403。欠落時の必須検証は書かれていない

### Maximum Streams

- `WT_MAX_STREAMS` / `WT_STREAMS_BLOCKED` 受信時に `maximum > 2^62-1` を明示拒否する経路は未確認（varint 最大が 2^62-1 のため encode 側は自然に制限されるが、受信検証の MUST 明示が必要）
- draft-15: 当該上限超過は session `WT_FLOW_CONTROL_ERROR`

### リソース非対応ステータス

- `WtServerRequest::reject(status)` は任意 status。ライブラリは 405 を自動選択しない
- draft-15: ターゲットリソースが WebTransport 非対応なら SHOULD **405**（draft-14 は 406）
- アプリ責務のまま、ドキュメント / example で `reject(405)` を明示する

## 設計方針

1. **CLOSE reason（送信）**: `close()` で 1024 超のときエラーにせず、UTF-8 文字境界で 1024 以下に切り詰めて送る（draft-15 MUST）。空や短い reason は現状維持
2. **CLOSE reason（受信）**: 1024 超または非 UTF-8 は `capsule_decode` ではなく session 終了に繋がる `WtError`（最終的に `WT_ERROR` / `ErrorCode::WtError`）にする。0084 完了後の名前に合わせる
3. **Origin**: `allowed_origin=Some` でも Origin **欠落時は検証スキップ**（accept 継続可）。存在するときだけ照合し、不一致は 403
4. **Max Streams**: `WT_MAX_STREAMS` / `WT_STREAMS_BLOCKED` の Maximum Streams が `> 2^62-1` なら `flow_control_error`
5. **405**: `crates/tokio-http2` の docs / `examples/wt_server` でリソース非対応時に `reject(405)` を示す。自動 405 固定ロジックは追加しない

## スコープ外

- SETTINGS_WT_ENABLED（0081）
- FIN 極性（0082）
- Reliable Size（0083）
- エラーコード名のリネーム作業本体（0084）。本 issue は 0084 後の識別子を使う
- 0066 サブプロトコル交渉（非対応時の status 例が 406 のままなら、0066 側で 405 に直すのは 0066 着手時）

## 他 issue との関係

- **0081** / **0084** の後に実施
- **0066**: サブプロトコル不適合の status 例。本 issue の 405 ガイダンスと混同しない

## 変更対象ファイル一覧

- `src/webtransport/mod.rs` — `close()` の切り詰め
- `src/webtransport/capsule.rs` — CLOSE 受信時のエラー種別
- `src/webtransport/flow_control.rs` / `mod.rs` — Max Streams 上限
- `crates/tokio-http2/src/webtransport.rs` — Origin 欠落時スキップ
- `crates/tokio-http2` docs / `examples/wt_server`
- `tests/test_webtransport/` / tokio-http2 テスト
- `CHANGES.md` develop

## 完了条件

- CLOSE 送信が UTF-8 境界で 1024 以下に切り詰められる
- CLOSE 受信の不正 reason が session `WT_ERROR` 経路になる
- Origin 欠落 + `allowed_origin=Some` でも accept 可能（不一致のみ 403）
- Maximum Streams `> 2^62-1` が `WT_FLOW_CONTROL_ERROR` になる
- 405 が docs/example で明示されている
- `cargo test --workspace` / clippy `-D warnings` が通る
- `CHANGES.md` にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 3.2（Origin / 405）、Section 6.7 / 6.10（Max Streams）、Section 6.12（WT_CLOSE_SESSION）
- `src/webtransport/mod.rs` — `close()`
- `src/webtransport/capsule.rs` — `MAX_CLOSE_REASON_LEN` / CLOSE decode
- `crates/tokio-http2/src/webtransport.rs` — `accept` の Origin 検証
