# Stream 構造体の凝集度を改善する

Created: 2026-05-14
Model: deepseek-v4-pro

## 対象

- `src/stream/mod.rs` (292 行)
- `src/connection/mod.rs` (Stream フィールドアクセス箇所 25 箇所)
- `tests/test_stream.rs` (新設)

## 内容

`Stream` 構造体が 19 フィールドを持ち、独立した関心事がフラットに押し込まれている。サブ構造体に抽出して凝集度を改善する。

## 修正方針

### `ConnectContext` サブ構造体

CONNECT 関連の 4 フィールドを抽出する:

```rust
// src/stream/mod.rs
#[derive(Debug, Default)]
pub struct ConnectContext {
    /// CONNECT トンネル確立済みフラグ (RFC 9113 Section 8.5)
    connect_established: bool,
    /// リクエストメソッド
    request_method: Option<Vec<u8>>,
    /// Extended CONNECT (:protocol 付き) かどうか
    has_protocol: bool,
    /// Extended CONNECT の :protocol 擬似ヘッダー値
    protocol: Option<Vec<u8>>,
}

impl ConnectContext {
    pub fn connect_established(&self) -> bool { ... }
    pub fn set_connect_established(&mut self, established: bool) { ... }
    pub fn request_method(&self) -> Option<&[u8]> { ... }
    pub fn set_request_method(&mut self, method: Vec<u8>) { ... }
    pub fn has_protocol(&self) -> bool { ... }
    pub fn set_has_protocol(&mut self, has_protocol: bool) { ... }
    pub fn protocol(&self) -> Option<&[u8]> { ... }
    pub fn set_protocol(&mut self, protocol: Vec<u8>) { ... }
}
```

### `ContentLengthTracker` サブ構造体

Content-Length 管理の 2 フィールドを抽出する。`no_content` は Content-Length 追跡とは別関心事のため Stream に残す:

```rust
// src/stream/mod.rs
#[derive(Debug, Default)]
pub struct ContentLengthTracker {
    /// 期待される Content-Length (RFC 9113 Section 8.1.1)
    expected: Option<u64>,
    /// 受信した Content-Length (累積)
    received: u64,
}

impl ContentLengthTracker {
    pub fn expected(&self) -> Option<u64> { ... }
    pub fn set_expected(&mut self, length: Option<u64>) { ... }
    pub fn received(&self) -> u64 { ... }
    pub fn add_received(&mut self, length: u64) { ... }
}
```

### Stream 構造体の変更

分割後の `Stream` は 12 フィールドに削減される (10 core + 2 sub-struct):

```rust
pub struct Stream {
    id: StreamId,
    state: StateMachine,
    flow_control: FlowControl,
    headers: Vec<HeaderField>,
    recv_buffer: RecvBuffer,
    send_buffer: SendBuffer,
    pending_end_stream: bool,
    initial_headers_received: bool,
    no_content: bool,
    final_response_sent: bool,
    connect_ctx: ConnectContext,
    content_length: ContentLengthTracker,
}
```

### 公開 API の互換性

`Stream` の既存の公開メソッドは委譲メソッドとして残し、内部でサブ構造体に転送する。呼び出し側 (`connection/mod.rs`) のコードは変更不要:

```rust
impl Stream {
    pub fn request_method(&self) -> Option<&[u8]> {
        self.connect_ctx.request_method()
    }
    pub fn set_request_method(&mut self, method: Vec<u8>) {
        self.connect_ctx.set_request_method(method)
    }
    pub fn expected_content_length(&self) -> Option<u64> {
        self.content_length.expected()
    }
    // ... 他の委譲メソッドも同様
}
```

### `connection/mod.rs` の追従

呼び出し側の変更は不要（委譲メソッドが同じシグネチャを維持するため）。ただし、メソッドが同じであれば呼び出し側のコードは変更する必要はない。

## テスト

- `tests/test_stream.rs` を新設し、`ConnectContext` と `ContentLengthTracker` の単体テストを追加する
- `pbt/tests/prop_stream_state.rs` は `StateMachine` のみをテストしているため、本変更による影響はない
- `cargo test --workspace` で全テストが通過することを確認する

## CHANGES.md (実装時に追記)

- `## develop` の `### misc` に以下を追加する:
  - `[UPDATE]` `Stream` の CONNECT 関連フィールドを `ConnectContext` に抽出する
    - @voluntas
  - `[UPDATE]` `Stream` の Content-Length 管理フィールドを `ContentLengthTracker` に抽出する
    - @voluntas

## 受け入れ基準

- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
- `Stream` 構造体の直接フィールド数が 19 → 12 に削減されている
- 既存の `Stream` 公開メソッドのシグネチャに変更がない
