# WT_RESET_STREAM 未登録ストリームへのエラー未検出

- Priority: Medium
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-reset-stream-unregistered-error

## 目的

draft-ietf-webtrans-http2-14 Section 6.2 の MUST 要件に従い、存在しないストリームに対する WT_RESET_STREAM カプセル受信時に `WEBTRANSPORT_STREAM_STATE_ERROR` を返す。

## 優先度根拠

仕様上の MUST 要件違反だが、正常なピア実装では発生しないケース。攻撃的なピアや実装バグのあるピアに対してのみ顕在化する。致命的ではないが見逃すと状態不整合の原因になりうる。

## 現状

draft-ietf-webtrans-http2-14 Section 6.2 (L826-L835):

> A stream error (Section 3.4) of type WEBTRANSPORT_STREAM_STATE_ERROR MUST be sent if a WT_RESET_STREAM capsule is received for a stream that is not in a valid state.

Section 3.4 (L372-L373) の `WEBTRANSPORT_STREAM_STATE_ERROR` 定義:

> A stream-related capsule identified a stream that was in an invalid state.

現在の実装 (`src/webtransport/mod.rs:588-615`):

```rust
Capsule::WtResetStream {
    stream_id, error_code, reliable_size,
} => {
    if let Some(stream) = self.streams.get_mut(&stream_id) {
        if !stream.can_recv() { ... }
        if reliable_size < stream.recv_offset() { ... }
        stream.recv_reset();
    }
    // ストリームが None の場合 → エラーにならず WtEvent::StreamReset 発行
    self.events.push_back(WtEvent::StreamReset { stream_id, error_code });
}
```

ストリームが `streams` HashMap に存在しない場合、`if let Some` で静かにスキップされ、`WtEvent::StreamReset` が発行される。これは仕様違反。

### 関連: 同種のバグが他ハンドラにも存在

`WtStreamDataBlocked` (L670-L677) のハンドラも同様に `if let Some(stream)` パターンで未登録ストリームをスキップしている。Section 6.9 (L1219-L1221) も Section 6.2 と同じ「not in a valid state」文言を用いている。本 issue では WT_RESET_STREAM のみを修正対象とし、`WtStreamDataBlocked` は別 issue で対応する。

## 完了条件

- 存在しないストリーム ID への WT_RESET_STREAM 受信時に `WtError::stream_state_error` が返ること
- 存在するストリーム ID への WT_RESET_STREAM 受信時は既存動作が維持されること（退行なし）
- 単体テストで検証されていること

## 解決方法

### Sans I/O 層 (`src/webtransport/mod.rs:588-615`)

`if let Some(stream) = ...` を `get_mut` + `ok_or_else` に変更する。変更は 1 行。

```rust
Capsule::WtResetStream {
    stream_id,
    error_code,
    reliable_size,
} => {
    let stream = self
        .streams
        .get_mut(&stream_id)
        .ok_or_else(|| {
            WtError::stream_state_error(format!(
                "WT_RESET_STREAM received for unknown stream {stream_id}"
            ))
        })?;

    // 以下既存の状態チェック（変更なし）
    if !stream.can_recv() {
        return Err(WtError::stream_state_error(
            "WT_RESET_STREAM received for stream not in valid state",
        ));
    }
    if reliable_size < stream.recv_offset() {
        return Err(WtError::stream_state_error(format!(
            "WT_RESET_STREAM reliable_size {reliable_size} is less than recv_offset {}",
            stream.recv_offset()
        )));
    }
    stream.recv_reset();

    self.events.push_back(WtEvent::StreamReset {
        stream_id,
        error_code,
    });
}
```

### テスト戦略

#### 単体テスト (`tests/test_webtransport/stream.rs`)

これはエラーパスの検証であり、PBT の対象ではない。単体テストに以下を追加:

```rust
#[test]
fn test_wt_reset_stream_unknown_stream_id() {
    // 未登録の stream_id に対する WT_RESET_STREAM が stream_state_error を返すこと
    let mut session = WtSession::server(WtConfig::default());
    session.initiate().unwrap();

    // WT_RESET_STREAM capsule を直接投入
    let capsule = Capsule::WtResetStream {
        stream_id: 0, // 未登録のストリーム ID
        error_code: 1,
        reliable_size: 0,
    };
    // handle_capsule 経由でエラーが返ることを検証
    // または CapsuleDecoder で encode → feed → decode の経路でテスト
}
```

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 3.4 (Session Termination and Error Handling), L344-L380
- draft-ietf-webtrans-http2-14 Section 6.2 (WT_RESET_STREAM Capsule), L826-L835
