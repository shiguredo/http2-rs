# draft-15 のセッション意味論（CLOSE / Origin / Max Streams / 405）に追従する

- Priority: Medium
- Created: 2026-07-20
- Polished: 2026-07-20
- Model: Grok 4.5
- Branch: feature/change-wt-draft15-session-semantics

## 目的

draft-ietf-webtrans-http2-15 で変わったセッション周辺の MUST/SHOULD を実装に反映する。対象は WT_CLOSE_SESSION の reason 扱い、Origin 検証条件、Maximum Streams 上限超過時のエラー、およびリソース非対応時の 405 ガイダンス。

## 優先度根拠

いずれも相互運用・仕様準拠に関わるが、SETTINGS / FIN / Reliable Size（0081–0083）ほどの wire 破壊ではないため Medium。まとめて 1 issue にする理由は、変更箇所がセッション確立・終了・フロー制御上限の「意味論」に閉じ、単一ブランチでレビューしやすいため。

## 現状

### WT_CLOSE_SESSION reason（送信）

- `src/webtransport/mod.rs` の `close()` (L484-L510): 1024 バイト超は **エラー返却**（切り詰めない）
- これは issue 0061 で draft-14 の MUST NOT に基づき意図的に決定した挙動。draft-15 でも「length MUST NOT exceed 1024 bytes」は維持されているが、切り詰め自体は義務ではなく、エラーを返すのも切り詰めるのも準拠。本 issue では呼び出し側の利便性（アプリ供給メッセージを暗黙に安全化できる）を優先し、切り詰め方式に変更する
- `src/webtransport/capsule.rs` の `CapsuleEncoder::encode` (L244-L255): `debug_assert!` で 1024 以下を前提

### WT_CLOSE_SESSION reason（受信）

- `src/webtransport/capsule.rs` decode (L548-L571): 1024 超・非 UTF-8 は `capsule_decode` エラー
- draft-15 Section 6.12 (L1394-1396): 1024 超または非 UTF-8 は session error **`WT_ERROR`** として扱う MUST
- 現状の `capsule_decode` エラーは `WtErrorKind::CapsuleDecode` であり、session error の wire シグナル（RST_STREAM + `WT_ERROR`）に繋がらない。draft-15 Section 3.4 (L402-429) では session error は WT_CLOSE_SESSION capsule または HTTP/2 エラーコード付きのストリームリセットで報告されると定義される

### Origin

- `crates/tokio-http2/src/webtransport.rs` の `accept(..., allowed_origin)` (L160-L175):
  - `allowed_origin=Some` かつ Origin **欠落** → 403 を送らず `InvalidArgument` のみ
  - 不一致 → `:status=403`
- これは issue 0062 で「Origin ヘッダーが存在しない場合も 403 で拒否されること (Web context の MUST 要件)」として意図的に決定した挙動
- draft-15 Section 3.2 (L341-345): **Origin ヘッダーがある場合**に MUST verify。失敗は SHOULD 403。欠落時の必須検証は書かれていない（draft-14 の無条件 "MUST verify" から条件付きに変更）

### Maximum Streams

- `WtFlowControl::update_max_streams` (`src/webtransport/flow_control.rs` L207-L224): 減少チェックのみ、上限チェックは未実装
- `WtStreamsBlocked` アーム (`src/webtransport/mod.rs` L744-L749): `maximum` を完全に無視
- draft-15 Section 6.7 (L1131-1134) / Section 6.10 (L1303-1308): Maximum Streams は **2^60** を超えてはならない（"This value cannot exceed 2^60, as it is not possible to encode stream IDs larger than 2^62-1"）。超過は session `WT_FLOW_CONTROL_ERROR` で MUST close
- 送信側 `send_max_streams` (`mod.rs` L536) / `grow_max_streams` (`mod.rs` L586、`saturating_add` で 2^60 を超え得る) も 2^60 超過値を送信し得る。draft-15 は "This value cannot exceed 2^60" と送信値そのものを拘束する

### リソース非対応ステータス

- `WtServerRequest::reject(status)` は任意 status。ライブラリは 405 を自動選択しない
- `examples/wt_server/src/main.rs` (L141-143) は現在 `reject(404)` をデモしている
- draft-15 Section 3.2 (L329-330): ターゲットリソースが WebTransport 非対応なら SHOULD **405**（draft-14 は 406）
- アプリ責務のまま、ドキュメント / example で `reject(405)` を明示する

## 設計方針

### 1. CLOSE reason（送信）

- `close()` で 1024 超のときエラーにせず、UTF-8 文字境界で 1024 以下に切り詰めて送る
- 切り詰めアルゴリズム: `reason.as_bytes()` の 1024 バイト位置から後方へ char 境界を探索する（`str::floor_char_boundary` は unstable のため手動実装。UTF-8 continuation byte `0b10xxxxxx` でない位置まで後退）
- 空や 1024 以下の reason は現状維持
- 0061 の判断を反転する（draft-15 でも切り詰めは義務ではないが、呼び出し側の利便性を優先）

### 2. CLOSE reason（受信）

- 1024 超または非 UTF-8 は `capsule_decode` ではなく session 終了に繋がる `WtError` にする
- `WtErrorKind` に session error 用の kind（例: `SessionError`）を追加するか、既存の `SessionStateError` を流用するかは実装時に判断する。0084 完了後の `ErrorCode::WtError` に対応付ける
- **wire シグナルの経路**: Sans I/O 層 (`WtSession`) では `feed()` / `process()` が `Err(WtError)` を返すことで session 終了を通知する。tokio-http2 driver 層で `WtError` を受信した際に RST_STREAM + `WT_ERROR` を送信する経路が必要。0077（`Error::WebTransport(WtError)` 追加）が未完了の場合、暫定的に `Error::InvalidArgument` 経由で RST_STREAM を送る
- 0077 との関係: 0077 が完了していれば `Error::WebTransport(WtError)` 経由でエラーコードを伝播できる。0077 未完了の場合は暫定経路で対応し、0077 完了後に差し替える

### 3. Origin

- `allowed_origin=Some` でも Origin **欠落時は検証スキップ**（accept 継続可）。存在するときだけ照合し、不一致は 403
- 0062 の判断を反転する（draft-15 で Origin 検証が「ヘッダーがある場合」の条件付きに変更されたため）

### 4. Max Streams

- `WT_MAX_STREAMS` / `WT_STREAMS_BLOCKED` の Maximum Streams が `> 2^60` なら `flow_control_error`
- 受信側: `update_max_streams` と `WtStreamsBlocked` アームに上限チェックを追加
- 送信側: `send_max_streams` / `grow_max_streams` に 2^60 上限の検証を追加（`saturating_add` で 2^60 を超えた場合にエラーを返す）

### 5. 405

- `crates/tokio-http2` の docs / `examples/wt_server` でリソース非対応時に `reject(405)` を示す。既存の `reject(404)` デモを 405 に更新する。自動 405 固定ロジックは追加しない

## スコープ外

- SETTINGS_WT_ENABLED（0081）
- FIN 極性（0082）
- Reliable Size（0083）
- エラーコード名のリネーム作業本体（0084）。本 issue は 0084 後の識別子を使う
- 0066 サブプロトコル交渉（非対応時の status 例が 406 のままなら、0066 側で 405 に直すのは 0066 着手時）

## 他 issue との関係

- **0081** の後に実施（0081 側が「0085 に SETTINGS_WT_ENABLED ゲートが前提になる箇所がある」と記載。本 issue の CLOSE 受信エラー経路が WT セッション確立後の挙動であるため、ゲート実装後のほうが統合テストの整合性が取りやすい）
- **0084** の後に実施（0084 のテスト・文言が新名を参照できるようにする）
- **0077**: tokio-http2 の `Error::WebTransport(WtError)` 追加。設計方針 2 のエラー伝播経路が 0077 と重なる。0077 未完了の場合は暫定経路で対応
- **0066**: サブプロトコル不適合の status 例。本 issue の 405 ガイダンスと混同しない
- **0061**（closed）: CLOSE reason 1024 超のエラー返却を決定。本 issue で切り詰め方式に反転する（draft-15 でも切り詰めは義務ではないが、利便性優先）
- **0062**（closed）: Origin 欠落時の 403 拒否を決定。本 issue で検証スキップに反転する（draft-15 で条件付きに変更）

## 変更対象ファイル一覧

- `src/webtransport/mod.rs` — `close()` の切り詰め (L484-L510)、`WtStreamsBlocked` アームの上限チェック (L744-L749)、`send_max_streams` / `grow_max_streams` の送信側検証 (L536, L586)
- `src/webtransport/capsule.rs` — CLOSE 受信時のエラー種別変更 (L548-L571)
- `src/webtransport/flow_control.rs` — `update_max_streams` の上限チェック (L207-L224)
- `src/webtransport/error.rs` — session error 用の `WtErrorKind` 追加（要否は実装時に判断）
- `crates/tokio-http2/src/webtransport.rs` — Origin 欠落時スキップ (L160-L175)、CLOSE 受信エラー時の RST_STREAM 送出経路
- `crates/tokio-http2` docs / `examples/wt_server/src/main.rs` — 405 ガイダンス (L141-143)
- `tests/test_webtransport/` — 切り詰め境界値テスト、CLOSE 受信エラー種別テスト、Max Streams 上限テスト
- `tests/test_webtransport/root.rs` — 既存 `test_close_reason_exceeds_max_length_errors` (L137-146) の挙動反転対応
- `crates/tokio-http2/tests/test_webtransport.rs` — 既存 `test_wt_origin_missing_rejected` (L1087-1111) の挙動反転対応、Origin 欠落 + accept 成功テスト追加
- `CHANGES.md` develop（`close()` と `accept()` の挙動反転は `[CHANGE]`）

## 完了条件

- CLOSE 送信が UTF-8 境界で 1024 以下に切り詰められる（1024 バイト境界にマルチバイト文字がまたがる場合の境界値テストを含む）
- CLOSE 受信の不正 reason（1024 超 / 非 UTF-8）が session `WT_ERROR` 経路になる
- Origin 欠落 + `allowed_origin=Some` でも accept 可能（不一致のみ 403）
- Maximum Streams `> 2^60` が送受信双方で `WT_FLOW_CONTROL_ERROR` になる（`2^60` 自身は許容）
- 405 が docs/example で明示されている
- 既存テストの挙動反転対応が完了している
- `cargo test --workspace` / clippy `-D warnings` が通る
- `CHANGES.md` にエントリがある

## 参照

- `refs/draft-ietf-webtrans-http2-15.txt` Section 3.2（Origin / 405）、Section 3.4（session error の報告手段）、Section 6.7 / 6.10（Max Streams、上限 2^60）、Section 6.12（WT_CLOSE_SESSION、reason 1024 バイト、UTF-8 境界、WT_ERROR）
- `refs/draft-ietf-webtrans-http2-14.txt` Section 3.2（旧: 無条件 Origin MUST verify、SHOULD 406）
- `src/webtransport/mod.rs` — `close()`、`WtStreamsBlocked` アーム
- `src/webtransport/capsule.rs` — `MAX_CLOSE_REASON_LEN` / CLOSE decode
- `src/webtransport/flow_control.rs` — `update_max_streams`
- `crates/tokio-http2/src/webtransport.rs` — `accept` の Origin 検証
- `issues/closed/0061-bug-fix-wt-close-session-reason-truncation.md` — CLOSE reason エラー返却の過去判断
- `issues/closed/0062-add-origin-header-verification.md` — Origin 欠落時 403 の過去判断
