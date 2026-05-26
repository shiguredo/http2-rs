# ストリーム ID 枯渇時のチェックを追加する

- Priority: High
- Created: 2026-05-24
- Completed: 2026-05-26
- Model: Opus 4.7
- Branch: feature/fix-stream-id-exhaustion

## 目的

`Connection::start_stream` で `self.next_stream_id += 2` としているが、
`next_stream_id` が `STREAM_ID_MAX` (2^31 - 1) を超えた場合のチェックがない。
`StreamId::from_wire` の範囲チェックは `debug_assert!` のみであり、
release ビルドでは 31-bit 範囲外の値がサイレントに通過して
RFC 違反のストリーム ID が wire に送信される。

## 優先度根拠

- RFC 9113 §5.1.1: ストリーム ID は unsigned 31-bit integer（最大 2^31 - 1）
- `StreamId::from_wire` は `debug_assert!` のみで release では通過する
- 長時間稼働する接続でストリーム ID が枯渇する可能性がある
- クライアントは奇数 ID のみ使用するため、実質的に約 10 億ストリームで枯渇する

## RFC 根拠

RFC 9113 §5.1.1:

> Stream identifiers cannot be reused. Long-lived connections can result in an endpoint
> exhausting the available range of stream identifiers. A client that is unable to establish
> a new stream identifier can establish a new connection for new streams. A server that is
> unable to establish a new stream identifier can send a GOAWAY frame so that the client is
> forced to open a new connection for new streams.

クライアント側の枯渇時の推奨動作は「新しい接続を確立する」であり、GOAWAY 送信は
サーバー側の推奨動作。本 issue の `start_stream` はクライアント専用メソッドであるため、
GOAWAY は送信せずエラーを返し、接続の再確立は呼び出し元の責務とする。

## 設計方針

### チェック位置

`start_stream` 内の `let stream_id_u32 = self.next_stream_id;` の直前
（既存のバリデーションチェック群の後、ストリーム ID の使用前）に枯渇チェックを挿入する。

既存のバリデーション順序（ヘッダー検証 → サーバー拒否 → GOAWAY 受信 → 同時ストリーム上限 →
Extended CONNECT → ヘッダーリストサイズ）の後に配置する理由:
- 他のバリデーションエラーが先に検出されるべき（枯渇は接続全体の問題であり、
  個別リクエストの問題を先に報告する方が呼び出し元にとって有用）
- 既存の全エラーパスが GOAWAY を送信しない設計と一貫性を保つ

### エラーの返し方

`start_stream` 内では GOAWAY を送信しない。既存の同時ストリーム上限超過と同じパターンで
ストリームエラーを返す:

```rust
if self.next_stream_id > STREAM_ID_MAX {
    return Err(Error::stream_error(
        ErrorCode::RefusedStream,
        "stream ID space exhausted, establish a new connection",
    ));
}
```

`RefusedStream` を選択する理由:
- 既存の `max_concurrent_streams` 超過（同ファイル L474）と同じエラーコードで一貫性がある
- RFC 9113 §8.7: REFUSED_STREAM は「アプリケーション処理前の拒否」を意味し、
  再試行可能性を示す。枯渇は「この接続では無理だが、新しい接続なら可能」であり、
  この意味合いに合致する
- `max_concurrent_streams` 超過は一時的だが ID 枯渇は不可逆である。しかしいずれも
  「このストリームは開始できないが、別の手段（待機 or 新接続）で回復可能」という点で
  RefusedStream の再試行セマンティクスが妥当

GOAWAY の送信判断は呼び出し元（`tokio-http2` のドライバー層）に委ねる。
`start_stream` がエラーを返した後、呼び出し元が graceful shutdown を行う場合は
`send_goaway(ErrorCode::NoError)` を呼べばよい。

### `STREAM_ID_MAX` の可視性

`src/stream_id.rs` の `STREAM_ID_MAX` は現在 `const`（private）。
`pub(crate)` に変更して `connection/mod.rs` からアクセス可能にする。

## 影響範囲

- `src/connection/mod.rs`: `start_stream` に枯渇チェック追加
- `src/stream_id.rs`: `STREAM_ID_MAX` を `pub(crate)` に変更
- `tests/test_connection.rs`: 枯渇時の境界値単体テスト追加
- `CHANGES.md`: `[FIX]` エントリ追記

注: `next_stream_id` は private フィールドであり、PBT（外部クレート）からは直接操作できない。
約 10 億回の `start_stream` 呼び出しは非現実的なため、本テストは PBT ではなく
単体テストで対応する（AGENTS.md: 「PBT で実現できないケースは単体テストで対応」）。
テストで `next_stream_id` を境界値付近に設定する方法として、`Connection` に
`#[cfg(test)] pub(crate) fn set_next_stream_id(&mut self, id: u32)` を追加する。

## 完了条件

- `next_stream_id` が `STREAM_ID_MAX` を超えている場合、`start_stream` が
  `Error::stream_error(ErrorCode::RefusedStream, ...)` を返す
- `next_stream_id == STREAM_ID_MAX`（2147483647、最後の有効な奇数）で最後のストリームが正常に開始される
- `next_stream_id += 2` で `STREAM_ID_MAX + 2`（2147483649）になった後の `start_stream` でエラーが返される
- GOAWAY は `start_stream` 内では送信されない
- 既存テストが通る
- 単体テストで以下の境界値を検証する（PBT では到達不可能なため単体テストで対応）:
  - `next_stream_id == STREAM_ID_MAX` → `start_stream` 成功
  - `next_stream_id == STREAM_ID_MAX + 2` → `start_stream` が RefusedStream エラーを返す
  - 境界: `STREAM_ID_MAX - 2`（2147483645）→ 成功、その後 `STREAM_ID_MAX` → 成功、
    その後 `STREAM_ID_MAX + 2` → 失敗の 3 段階
- CHANGES.md に `[FIX]` エントリ追記

## 解決方法

`start_stream` 内のストリーム ID 使用前に `next_stream_id > STREAM_ID_MAX` のチェックを追加し、枯渇時は `RefusedStream` エラーを返すようにした。`STREAM_ID_MAX` を `pub(crate)` に変更して `connection/mod.rs` からアクセス可能にした。

### 変更ファイル

- `src/stream_id.rs`: `STREAM_ID_MAX` を `pub(crate)` に変更
- `src/connection/mod.rs`: `start_stream` に枯渇チェック追加、テスト用 `set_next_stream_id` 追加、境界値単体テスト 3 件追加
- `CHANGES.md`: `[FIX]` エントリ追記
