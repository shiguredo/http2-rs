# fuzz target を網羅的に追加する

- Priority: Medium
- Created: 2026-05-24
- Model: Opus 4.7
- Branch: feature/add-comprehensive-fuzz-targets

## 目的

既存の fuzz target 10 個はデコーダー単体のパニック安全性を中心にカバーしているが、エンコーダーのパニック安全性、エンコード/デコードのラウンドトリップ整合性、クライアントロールの受信パス、接続プリフェイスの検証パス、双方向操作の複合状態遷移、WebTransport セッション層、フロー制御の連続操作など、体系的に欠落している領域がある。

CLAUDE.md のテスト役割分担「Fuzzing: 任意入力に対するクラッシュ耐性（パニック安全性）」に基づき、fuzz target を網羅的に追加する。

## 優先度根拠

Medium。既存の fuzz target でデコーダー系の基本的なパニック安全性はカバーされている。本 issue は防御の深さ (defense in depth) を強化するものであり、緊急性は高くないが、issue 0045 (wire ヘルパ移行) と同時期に整備することで fuzz 基盤全体を一括で完成させられる。

## 現状

### 既存 fuzz target (10 個)

| fuzz target | 対象 | 入力 | 検証内容 |
|---|---|---|---|
| `fuzz_frame_decoder` | `FrameDecoder` | `&[u8]` | 任意バイト列のフレームデコード。パニック安全性 |
| `fuzz_hpack_decoder` | `HpackDecoder` | `&[u8]` | 任意バイト列の HPACK デコード。パニック安全性 |
| `fuzz_hpack_integer` | `hpack::integer::decode` | `&[u8]` | HPACK 整数デコード。オーバーフロー耐性 |
| `fuzz_huffman_decoder` | `hpack::huffman::decode` | `&[u8]` | Huffman デコード。パニック安全性 |
| `fuzz_hpack_roundtrip` | `HpackEncoder` + `HpackDecoder` | `Arbitrary` | HPACK エンコード → デコードのラウンドトリップ一致 |
| `fuzz_capsule_decoder` | `CapsuleDecoder` | `&[u8]` | WebTransport Capsule デコード。パニック安全性 |
| `fuzz_varint_decoder` | `varint::decode` | `&[u8]` | WebTransport varint デコード。パニック安全性 |
| `fuzz_connection` | `Connection` (サーバー) | `&[u8]` | サーバーロールの接続処理。パニック安全性 |
| `fuzz_settings` | `Setting::from_wire` + `Settings::apply` | `Arbitrary` | SETTINGS wire 値のパースと適用。パニック安全性 |
| `fuzz_validation` | `validate_*` | `Arbitrary` | ヘッダー検証。パニック安全性 |

### カバレッジの欠落

1. **エンコーダー単体**: フレームエンコーダー (`FrameEncoder::encode`)、Capsule エンコーダー (`CapsuleEncoder::encode`) に対する任意入力の fuzz がない
2. **ラウンドトリップ**: フレーム、Capsule、varint のエンコード → デコードのラウンドトリップ検証がない (HPACK のみ存在)
3. **クライアントロール**: `fuzz_connection` はサーバーロール固定。クライアントとしての受信パスが未テスト
4. **プリフェイス検証**: `fuzz_connection` は `mark_preface_received()` でバイパスしており、プリフェイスバッファの境界条件が未テスト
5. **双方向操作**: ローカル操作 (ストリーム開始、データ送信等) とリモート入力 (feed + process) の交互実行がない
6. **HeaderField 構築**: `HeaderField::new` の検証ロジック自体への任意入力 fuzz がない
7. **フロー制御**: `FlowControl` / `WtFlowControl` の連続操作によるオーバーフロー/アンダーフロー耐性が未テスト
8. **HPACK 連続操作**: 複数ラウンドの HPACK エンコード/デコードで動的テーブルの状態遷移が一貫しているかの検証がない

## 設計方針

### 追加する fuzz target

#### 高優先度

**1. `fuzz_frame_roundtrip`** — フレームのラウンドトリップ

任意の `Frame` を `FrameEncoder::encode` でエンコードし、`FrameDecoder::decode` でデコードして、ラウンドトリップの一致を検証する。全フレーム種別 (DATA, HEADERS, RST_STREAM, SETTINGS, PING, GOAWAY, WINDOW_UPDATE, CONTINUATION, PRIORITY_UPDATE) をカバーする。

```rust
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    frame_type: u8,
    stream_id: u32,
    flags: u8,
    payload: Vec<u8>,
    // フレーム種別に応じた追加フィールド
}
```

エンコード成功時のみデコードを実行し、デコード結果との一致を検証する。エンコード/デコード双方のパニック安全性も検証する。

**2. `fuzz_connection_client`** — クライアントロールの受信処理

クライアントとして `Connection::client` を生成し、サーバーからの任意バイト列を feed + process する。サーバーからの不正フレーム列 (不正な SETTINGS ACK、不正な PUSH_PROMISE、巨大なヘッダーブロック等) に対するパニック安全性を検証する。

```rust
fuzz_target!(|data: &[u8]| {
    let limits = Limits::default();
    let mut conn = Connection::client(limits);
    conn.mark_preface_sent();
    let _ = conn.initiate();
    while conn.poll_output().is_some() {}
    let _ = conn.feed(data);
    let _ = conn.process();
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}
});
```

**3. `fuzz_connection_preface`** — プリフェイス検証パス

サーバーロールで `mark_preface_received()` を呼ばずに、任意バイト列を直接 feed する。HTTP/2 接続プリフェイス (RFC 9113 §3.4) のバッファリングと検証の境界条件を検証する。断片的な入力 (1 バイトずつ等) も含む。

```rust
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    chunks: Vec<Vec<u8>>,
}

fuzz_target!(|input: FuzzInput| {
    let limits = Limits::default();
    let mut conn = Connection::server(limits);
    let _ = conn.initiate();
    while conn.poll_output().is_some() {}
    for chunk in &input.chunks {
        let _ = conn.feed(chunk);
        let _ = conn.process();
        while conn.poll_event().is_some() {}
        while conn.poll_output().is_some() {}
    }
});
```

**4. `fuzz_connection_interactive`** — 双方向操作の複合状態遷移

ローカル操作 (`start_stream`, `send_data`, `send_response`, `reset_stream`, `send_ping`, `send_goaway`, `send_window_update`) とリモート入力 (`feed` + `process`) を任意に交互実行する。複合状態遷移でのパニック安全性を検証する。

```rust
#[derive(Debug, Arbitrary)]
enum FuzzAction {
    Feed(Vec<u8>),
    StartStream { headers: Vec<FuzzHeader> },
    SendData { stream_id: u32, data: Vec<u8>, end_stream: bool },
    ResetStream { stream_id: u32, error_code: u32 },
    SendPing { opaque_data: [u8; 8] },
    SendGoaway { error_code: u32 },
    SendWindowUpdate { stream_id: u32, increment: u32 },
    Process,
    PollEvent,
    PollOutput,
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    role_is_server: bool,
    actions: Vec<FuzzAction>,
}
```

全ての操作は `Result` を無視し、パニックしないことのみを検証する。

#### 中優先度

**5. `fuzz_capsule_roundtrip`** — Capsule のラウンドトリップ

任意の `Capsule` を `CapsuleEncoder::encode` でエンコードし、`CapsuleDecoder` でデコードして、ラウンドトリップの一致を検証する。

**6. `fuzz_varint_roundtrip`** — varint のラウンドトリップ

任意の `u64` (ただし varint の表現可能範囲 0..2^62-1) を `varint::encode` でエンコードし、`varint::decode` でデコードして一致を検証する。

**7. `fuzz_header_field`** — HeaderField 構築のパニック安全性

任意の name/value バイト列を `HeaderField::new` / `HeaderField::new_with_sensitive` に渡し、パニック安全性を検証する。成功した場合は不変条件 (name が小文字、value に NUL/CR/LF なし、先頭/末尾に SP/HTAB なし) を assert で検証する。

```rust
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    name: Vec<u8>,
    value: Vec<u8>,
    sensitive: bool,
}

fuzz_target!(|input: FuzzInput| {
    match HeaderField::new_with_sensitive(&input.name, &input.value, input.sensitive) {
        Ok(hf) => {
            // 構築成功時の不変条件を検証
            assert!(hf.name().iter().all(|&b| !b.is_ascii_uppercase() || !hf.name().starts_with(b":")));
            assert!(!hf.value().iter().any(|&b| b == 0x00 || b == 0x0d || b == 0x0a));
        }
        Err(_) => {}
    }
});
```

**8. `fuzz_hpack_sequential`** — HPACK 連続操作の状態一貫性

複数ラウンドのヘッダーリストを連続でエンコード/デコードし、動的テーブルの状態遷移が一貫していることを検証する。テーブルサイズ変更も含む。

```rust
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_table_size: u16,
    rounds: Vec<FuzzRound>,
}

#[derive(Debug, Arbitrary)]
struct FuzzRound {
    headers: Vec<FuzzHeader>,
    new_table_size: Option<u16>,
}
```

各ラウンドでエンコード → デコード → ラウンドトリップ一致を検証する。テーブルサイズ変更がある場合は両者に適用する。

#### 低優先度

**9. `fuzz_flow_control`** — HTTP/2 フロー制御の連続操作

`FlowControl` に対して `consume_send` / `consume_recv` / `recv_window_update` / `add_recv_window` / `update_initial_window_size` を任意順序で呼び出し、パニック安全性とオーバーフロー/アンダーフロー耐性を検証する。

```rust
#[derive(Debug, Arbitrary)]
enum FlowAction {
    ConsumeSend(u16),
    ConsumeRecv(u16),
    RecvWindowUpdate(u32),
    AddRecvWindow(u32),
    UpdateInitialWindowSize(u32),
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_window_size: u32,
    actions: Vec<FlowAction>,
}
```

**10. `fuzz_wt_flow_control`** — WebTransport フロー制御の連続操作

`WtFlowControl` に対して `consume_send` / `consume_recv` / `update_send_max` / `add_recv_max` / `update_max_streams` / `opened_stream` を任意順序で呼び出し、パニック安全性を検証する。

### Cargo.toml への追記

`fuzz/Cargo.toml` に各 `[[bin]]` エントリを追加する。既存の形式に従う。

### CHANGES.md

`## develop` の `### misc` に追記:

```
- [ADD] fuzz target を 10 個追加し、フレームラウンドトリップ・クライアントロール・プリフェイス検証・双方向操作・Capsule/varint ラウンドトリップ・HeaderField 構築・HPACK 連続操作・フロー制御の fuzzing を網羅する
  - @voluntas
```

## 完了条件

- [ ] 以下の 10 個の fuzz target が `fuzz/fuzz_targets/` に追加されている:
  - `fuzz_frame_roundtrip.rs`
  - `fuzz_connection_client.rs`
  - `fuzz_connection_preface.rs`
  - `fuzz_connection_interactive.rs`
  - `fuzz_capsule_roundtrip.rs`
  - `fuzz_varint_roundtrip.rs`
  - `fuzz_header_field.rs`
  - `fuzz_hpack_sequential.rs`
  - `fuzz_flow_control.rs`
  - `fuzz_wt_flow_control.rs`
- [ ] `fuzz/Cargo.toml` に全 `[[bin]]` エントリが追加されている
- [ ] `cargo check --manifest-path fuzz/Cargo.toml` が通る
- [ ] `cargo clippy --manifest-path fuzz/Cargo.toml -- -D warnings` が通る
- [ ] 各 fuzz target が `cargo fuzz run <target> -- -runs=0` (コンパイル確認) で成功する
- [ ] `CHANGES.md` の `### misc` に `[ADD]` エントリが追加されている

## 解決方法

高優先度 (1-4) → 中優先度 (5-8) → 低優先度 (9-10) の順に実装する。各 fuzz target は独立しているため、実装順序に厳密な依存関係はない。

issue 0045 (`__test_helpers` 廃止) が先に完了している場合、`fuzz_validation.rs` / `fuzz_hpack_roundtrip.rs` は既に wire ヘルパ方式に移行済みのため、新規 fuzz target はそれに準じた設計にする。0045 が未完了の場合でも本 issue の新規 target は `__test_helpers` feature を使用しないため、並行して実装可能。

## 関連

- [[0045-refactor-remove-test-helpers-feature]] (fuzz 基盤の wire ヘルパ移行)
- [[0037-add-fuzz-build-check-to-ci]] (CI での fuzz コンパイル確認)
