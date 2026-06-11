# 未使用コードを一括削除する

- Priority: Medium
- Created: 2026-06-11
- Polished: {Polished}
- Model: deepseek-v4-pro
- Branch: feature/refactor-remove-unused-code

## 目的

コードベース内の未使用コード（死にコード・呼び出しのない関数・未使用の Default 実装等）を一括削除する。

## 削除対象一覧

### 1. `WtError::incomplete()` — `src/webtransport/error.rs:92-94`

全コードベースで一度も呼ばれない。`varint::decode` 内では `WtError::new(WtErrorKind::Incomplete)` が直接使われている。

```rust
pub fn incomplete() -> Self {
    Self::new(WtErrorKind::Incomplete)
}
```

### 2. `WtError::buffer_too_short()` — `src/webtransport/error.rs:97-99`

同上。テストでのみ参照。

```rust
pub fn buffer_too_short() -> Self {
    Self::new(WtErrorKind::BufferTooShort)
}
```

### 3. `WtErrorKind::SessionClosed` + `WtError::session_closed()` — `src/webtransport/error.rs:36, 140-142`

セッションクローズは状態遷移 (`WtSessionState::Closed`) で表現されており、エラー経路では未使用。

### 4. `stream_id::stream_type()` — `src/webtransport/stream.rs:64-66`

全コードベースで呼び出しなし。

```rust
pub const fn stream_type(id: WtStreamId) -> u8 {
    (id & 0x03) as u8
}
```

### 5. `SendBuffer::clear()` — `src/stream/buffer.rs:63-65`

製品コードからの呼び出しなし。

### 6. `RecvBuffer::clear()` — `src/stream/buffer.rs:128-130`

製品コードからの呼び出しなし。

### 7. `RecvBuffer::take()` — `src/stream/buffer.rs:105-107`

テストでのみ使用。テストが参照するなら保持するが、テスト自体が不要なら削除。

### 8. `WtFlowControl::Default` — `src/webtransport/flow_control.rs:274-278`

全コードベースで `WtFlowControl::default()` が一度も呼ばれていない。常に `WtFlowControl::new(...)` が明示的な引数付きで呼ばれている。`WtConfig::default()` が同じデフォルト値を二重管理している。

## 完了条件

- 上記全項目が削除されていること
- 削除後のコードが `cargo build --workspace` で成功すること
- `cargo test --workspace` が成功すること（テストコードから参照がある項目はテスト側も修正または削除）
- `cargo clippy --all-targets --all-features -- -D warnings` が成功すること
- CHANGES.md `## develop` に `[CHANGE]` エントリを追加すること

## 参照

- `issues/closed/0019-chore-remove-dead-code.md` — 過去の死にコード削除事例
- `src/webtransport/error.rs:92-142` — WtError の未使用コンストラクタ群
- `src/webtransport/stream.rs:64-66` — `stream_type()`
- `src/stream/buffer.rs:63-65, 105-107, 128-130` — SendBuffer/RecvBuffer の未使用メソッド
- `src/webtransport/flow_control.rs:274-278` — WtFlowControl::Default
