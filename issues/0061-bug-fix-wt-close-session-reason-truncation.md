# WT_CLOSE_SESSION reason 超過時の暗黙的切り詰めをエラーに変更

- Priority: Medium
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-close-session-reason-truncation

## 目的

draft-ietf-webtrans-http2-14 Section 6.12 の MUST NOT 要件に従い、WT_CLOSE_SESSION の reason が 1024 バイトを超過した場合、暗黙的に切り詰めるのではなくエラーを返すようにする。

## 優先度根拠

仕様上の MUST NOT 要件違反。現在の実装では reason が 1024 バイトを超えた場合に `.min(1024)` で暗黙的に切り詰めており、呼び出し側に通知されない。過剰に長い reason が正当なものとして処理されてしまう。

## 現状

draft-ietf-webtrans-http2-14 Section 6.12 (L1355-L1358):

> A UTF-8 encoded error message string provided by the application closing the connection. The message takes up the remainder of the capsule, and its length MUST NOT exceed 1024 bytes.

現在の実装:

- **エンコーダー** (`src/webtransport/capsule.rs:244-252`): `reason_bytes.len().min(MAX_CLOSE_REASON_LEN)` でサイレント切り詰め。
- **デコーダー** (`src/webtransport/capsule.rs:553-558`): 既に 1024 超過をエラー検出済み。エンコーダー側との非対称が問題。

```rust
// エンコーダー側 (L244-252) - 問題の箇所
Capsule::WtCloseSession { error_code, reason } => {
    let reason_bytes = reason.as_bytes();
    let reason_len = reason_bytes.len().min(MAX_CLOSE_REASON_LEN); // ← 暗黙切り詰め
    let payload_len = 4 + reason_len;
    self.encode_header(capsule_type::WT_CLOSE_SESSION, payload_len);
    self.buffer.extend_from_slice(&error_code.to_be_bytes());
    self.buffer.extend_from_slice(&reason_bytes[..reason_len]);
}
```

## 完了条件

- reason が 1024 バイト超過時にエラーが返ること
- reason が 1024 バイト以内の場合は正常にエンコードされること
- デコーダー側の既存チェック（1024 超過エラー）との一貫性が保たれること
- 単体テストで検証されていること

### 0058 との競合について

0058 (WT_CLOSE_SESSION + END_STREAM 自動送信) も `WtSession::close()` 周辺を修正する。両者の修正箇所は独立しているが、実装順序は 0058 → 0061 を推奨。0061 は `close()` 内で reason 長チェックを追加するのみで、0058 の変更に依存しない。

## 解決方法

### 方針: `WtSession::close()` 内での事前チェック

最も変更範囲が小さい。`CapsuleEncoder::encode()` のシグネチャ変更が不要で、Sans I/O 層の利用者にとって自然な場所でエラーが検出される。`close()` は WT_CLOSE_SESSION の唯一の生成箇所であり、`CapsuleEncoder` が外部から `WtCloseSession` で直接呼ばれることはない。

`src/webtransport/mod.rs:446` の `WtSession::close()` に reason 長チェックを追加:

```rust
pub fn close(&mut self, error_code: u32, reason: &str) -> WtResult<()> {
    if self.state == WtSessionState::Closed {
        return Err(WtError::session_state_error("session already closed"));
    }

    // draft-ietf-webtrans-http2-14 Section 6.12 (L1355-L1358):
    // reason の長さは MUST NOT exceed 1024 bytes
    if reason.len() > MAX_CLOSE_REASON_LEN {
        return Err(WtError::capsule_decode(format!(
            "WT_CLOSE_SESSION reason exceeds {} bytes (got {})",
            MAX_CLOSE_REASON_LEN,
            reason.len(),
        )));
    }

    let capsule = Capsule::WtCloseSession {
        error_code,
        reason: reason.to_string(),
    };
    self.capsule_encoder.encode(&capsule);
    self.output_buffer.extend(self.capsule_encoder.take());

    self.state = WtSessionState::Closed;

    Ok(())
}
```

#### CapsuleEncoder 内の `.min()` の扱い

`close()` での事前チェックにより `encode()` 内の `.min(MAX_CLOSE_REASON_LEN)` は到達不能になる。コード上は防衛的に残すか、`debug_assert!` に置き換える。

**推奨**: `debug_assert!` に置き換える。到達不能コードを削除する AGENTS.md の方針に従う。

```rust
Capsule::WtCloseSession { error_code, reason } => {
    let reason_bytes = reason.as_bytes();
    debug_assert!(
        reason_bytes.len() <= MAX_CLOSE_REASON_LEN,
        "WT_CLOSE_SESSION reason length already checked in WtSession::close()"
    );
    let reason_len = reason_bytes.len(); // .min() 削除
    let payload_len = 4 + reason_len;
    self.encode_header(capsule_type::WT_CLOSE_SESSION, payload_len);
    self.buffer.extend_from_slice(&error_code.to_be_bytes());
    self.buffer.extend_from_slice(reason_bytes);
}
```

### テスト戦略

#### 単体テスト (`tests/test_webtransport/`)

- reason = 1024 バイト → 正常エンコード
- reason = 1025 バイト → `close()` がエラーを返す
- 既存の Capsule エンコード/デコードラウンドトリップテストの退行確認

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 6.12 (WT_CLOSE_SESSION Capsule), L1321-L1370
