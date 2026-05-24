# fuzz target を網羅的に追加する

- Priority: Medium
- Created: 2026-05-24
- Completed: 2026-05-24
- Model: Opus 4.7
- Branch: feature/add-comprehensive-fuzz-targets

## 目的

既存の fuzz target 10 個はデコーダー単体のパニック安全性を中心にカバーしているが、エンコーダーのパニック安全性、クライアントロールの受信パス、接続プリフェイスの検証パス、双方向操作の複合状態遷移、HeaderField 構築、フロー制御の連続操作、HPACK 連続操作など、体系的に欠落している領域がある。

CLAUDE.md のテスト役割分担に基づき、fuzz target はパニック安全性の検証のみに限定する。ラウンドトリップの一致検証は PBT の役割であり (`pbt/tests/prop_frame.rs`、`pbt/tests/prop_webtransport.rs`、`pbt/tests/prop_hpack.rs` で網羅済み)、fuzz target には含めない。

## 優先度根拠

Medium。既存の fuzz target でデコーダー系の基本的なパニック安全性はカバーされている。本 issue は防御の深さ (defense in depth) を強化するものであり、緊急性は高くない。

## 現状

### 既存 fuzz target (10 個)

| fuzz target | 対象 | 入力 | 検証内容 |
|---|---|---|---|
| `fuzz_frame_decoder` | `FrameDecoder` | `&[u8]` | 任意バイト列のフレームデコード。パニック安全性 |
| `fuzz_hpack_decoder` | `HpackDecoder` | `&[u8]` | 任意バイト列の HPACK デコード。パニック安全性 |
| `fuzz_hpack_integer` | `hpack::integer::decode` | `&[u8]` | HPACK 整数デコード。オーバーフロー耐性 |
| `fuzz_huffman_decoder` | `hpack::huffman::decode` | `&[u8]` | Huffman デコード。パニック安全性 |
| `fuzz_hpack_roundtrip` | `HpackEncoder` + `HpackDecoder` | `Arbitrary` | HPACK エンコード → デコードのラウンドトリップ一致 (既存のため本 issue では変更しない。ラウンドトリップ検証の分離は必要に応じて別 issue で検討) |
| `fuzz_capsule_decoder` | `CapsuleDecoder` | `&[u8]` | WebTransport Capsule デコード。パニック安全性 |
| `fuzz_varint_decoder` | `varint::decode` | `&[u8]` | WebTransport varint デコード。パニック安全性 |
| `fuzz_connection` | `Connection` (サーバー) | `&[u8]` | サーバーロールの接続処理。パニック安全性 |
| `fuzz_settings` | `Setting::from_wire` + `Settings::apply` | `Arbitrary` | SETTINGS wire 値のパースと適用。パニック安全性 |
| `fuzz_validation` | `validate_*` | `Arbitrary` | ヘッダー検証。パニック安全性 |

### カバレッジの欠落

1. **エンコーダー単体**: `FrameEncoder::encode` と `CapsuleEncoder::encode` に対する任意入力 fuzz がない
2. **クライアントロール**: `fuzz_connection` はサーバーロール固定。クライアントとしての受信パスが未テスト
3. **プリフェイス検証**: `fuzz_connection` は `mark_preface_received()` でバイパスしており、プリフェイスバッファの境界条件が未テスト
4. **双方向操作**: ローカル操作 (ストリーム開始、データ送信等) とリモート入力 (feed + process) の交互実行がない
5. **HeaderField 構築**: `HeaderField::new` の検証ロジック自体への任意入力 fuzz がない
6. **フロー制御**: `FlowControl` / `WtFlowControl` の連続操作によるオーバーフロー/アンダーフロー耐性が未テスト
7. **HPACK 連続操作**: 複数ラウンドの HPACK エンコード/デコードで動的テーブルの状態遷移に対するパニック安全性の検証がない

## 設計方針

### テスト役割分担の遵守

CLAUDE.md の規約に従い、全ての fuzz target はパニック安全性の検証のみを行う:
- 全ての操作の `Result` は無視し、`panic` しないことのみを検証する
- ラウンドトリップの一致検証 (`assert_eq!` による比較) は含めない (PBT の役割)
- 不変条件の検証 (`assert!` による構築成功時のプロパティ確認) も含めない (PBT の役割)

### 追加する fuzz target

#### 高優先度

**1. `fuzz_frame_encoder`** — フレームエンコーダーのパニック安全性

各フレーム種別ごとに構造化された `Arbitrary` 入力から `Frame` を構築し、`FrameEncoder::encode` に渡してパニック安全性を検証する。

`Frame` 型は `Arbitrary` を derive していないため、fuzz target 内でフレーム種別を表す enum を定義し、各種別に必要なフィールドを `Arbitrary` で生成して `Frame` を構築する。

`FrameEncoder::encode` は `Frame::Priority` (RFC 9113 で非推奨のため送信拒否) と `Frame::PushPromise` (サーバープッシュ非サポート) で `Err` を返す。これらのバリアントも生成対象に含め、`Err` が返ることを許容する (パニックしないことが検証対象)。

```rust
#[derive(Debug, Arbitrary)]
enum FuzzFrame {
    Data {
        stream_id: u32,
        data: Vec<u8>,
        end_stream: bool,
        pad_length: Option<u8>,
    },
    Headers {
        stream_id: u32,
        header_block_fragment: Vec<u8>,
        end_stream: bool,
        end_headers: bool,
        pad_length: Option<u8>,
    },
    RstStream {
        stream_id: u32,
        error_code: u32,
    },
    Settings {
        ack: bool,
        settings: Vec<(u16, u32)>,
    },
    Ping {
        opaque_data: [u8; 8],
        ack: bool,
    },
    Goaway {
        last_stream_id: u32,
        error_code: u32,
        debug_data: Vec<u8>,
    },
    WindowUpdate {
        stream_id: u32,
        increment: u32,
    },
    Continuation {
        stream_id: u32,
        header_block_fragment: Vec<u8>,
        end_headers: bool,
    },
    PriorityUpdate {
        element_id: u32,
        priority_field_value: Vec<u8>,
    },
    Priority {
        stream_id: u32,
        dependency: u32,
        weight: u16,
        exclusive: bool,
    },
    // Frame::PushPromise は stream_id のみ保持 (サーバープッシュ非サポート)
    PushPromise {
        stream_id: u32,
    },
    Unknown {
        frame_type: u8,
        flags: u8,
        stream_id: u32,
        payload: Vec<u8>,
    },
}

fuzz_target!(|input: FuzzFrame| {
    // FuzzFrame から Frame への変換を試みる。
    // NonZeroStreamId / WindowIncrement / LastStreamId 等の
    // 型制約で構築に失敗する場合は Err を無視する。
    // Frame 構築成功時は FrameEncoder::encode に渡す。
    // 全ての結果を無視し、パニックしないことのみを検証する。
    let mut encoder = FrameEncoder::new();
    if let Some(frame) = try_build_frame(&input) {
        let _ = encoder.encode(&frame);
    }
});
```

`try_build_frame` は `NonZeroStreamId::new`、`WindowIncrement::new`、`LastStreamId::new` 等の構築時検査を呼び出し、失敗した場合は `None` を返す。`FuzzFrame::Settings` の `Vec<(u16, u32)>` は各タプルに対して `Setting::from_wire(id, value)` で変換し、失敗した項目はスキップする (既存の `fuzz_settings.rs` と同じ方式)。これにより、型安全な Frame を構築できた場合のみエンコードを試みる。

**2. `fuzz_connection_client`** — クライアントロールの受信処理

クライアントとして `Connection::client` を生成し、サーバーからの任意バイト列を feed + process する。サーバーからの不正フレーム列に対するパニック安全性を検証する。

```rust
fuzz_target!(|data: &[u8]| {
    let limits = Limits::default();
    let mut conn = Connection::client(limits);
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

RFC 9113 §3.4 の検証要件:
- プリフェイス 24 バイトの後に SETTINGS フレームが必須 (MUST)
- 不正プリフェイスは PROTOCOL_ERROR (MUST)
- feed 経路でのプリフェイスバッファリングと、process 経路での最初のフレーム (SETTINGS) 検証の二段階

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
    // chunks は先頭 64 件に切り詰める (fuzzer のスループット確保のため)
    for chunk in input.chunks.iter().take(64) {
        let _ = conn.feed(chunk);
        let _ = conn.process();
        while conn.poll_event().is_some() {}
        while conn.poll_output().is_some() {}
    }
});
```

**4. `fuzz_connection_interactive`** — 双方向操作の複合状態遷移

ローカル操作とリモート入力を任意に交互実行する。複合状態遷移でのパニック安全性を検証する。

`Connection` の公開 API シグネチャに合わせた `FuzzAction` 定義:
- `start_stream`: `headers: Vec<HeaderField>, end_stream: bool` — `HeaderField` は `Arbitrary` を derive していないため、`FuzzHeader { name: Vec<u8>, value: Vec<u8> }` から wire ヘルパ (`fuzz_hpack_roundtrip.rs` と同じ方式) で変換する
- `send_data`: `stream_id: StreamId, data: &[u8], end_stream: bool`
- `send_response`: `stream_id: StreamId, headers: Vec<HeaderField>, end_stream: bool` — サーバーロールでの応答送信。`start_stream` と同じ wire ヘルパ方式
- `reset_stream`: `stream_id: StreamId, error_code: ErrorCode`
- `send_ping`: `opaque_data: [u8; 8]`
- `send_goaway`: `error_code: ErrorCode, debug_data: Vec<u8>`
- `send_window_update`: `stream_id: StreamId, increment: u32`
- `send_trailers`: `stream_id: StreamId, headers: Vec<HeaderField>` — END_STREAM 付きトレーラー

```rust
#[derive(Debug, Arbitrary)]
enum FuzzAction {
    Feed(Vec<u8>),
    StartStream {
        headers: Vec<FuzzHeader>,
        end_stream: bool,
    },
    SendResponse {
        stream_id: u32,
        headers: Vec<FuzzHeader>,
        end_stream: bool,
    },
    SendData {
        stream_id: u32,
        data: Vec<u8>,
        end_stream: bool,
    },
    SendTrailers {
        stream_id: u32,
        headers: Vec<FuzzHeader>,
    },
    ResetStream {
        stream_id: u32,
        error_code: u32,
    },
    SendPing {
        opaque_data: [u8; 8],
    },
    SendGoaway {
        error_code: u32,
        debug_data: Vec<u8>,
    },
    SendWindowUpdate {
        stream_id: u32,
        increment: u32,
    },
    Process,
    PollEvent,
    PollOutput,
}

#[derive(Debug, Arbitrary)]
struct FuzzHeader {
    name: Vec<u8>,
    value: Vec<u8>,
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    role_is_server: bool,
    actions: Vec<FuzzAction>,
}

// actions は先頭 256 件に切り詰める (fuzzer のスループット確保のため)
```

`FuzzAction` 内の `stream_id: u32` は `StreamId::from_wire(u32)` で変換する。`error_code: u32` は `ErrorCode::from_u32(u32)` で変換する。`FuzzHeader` は既存の `fuzz_hpack_roundtrip.rs` / `fuzz_validation.rs` と同じ wire ヘルパ (`wire_header_field`) で `HeaderField` に変換する。

全ての操作は `Result` を無視し、パニックしないことのみを検証する。

#### 中優先度

**5. `fuzz_capsule_encoder`** — Capsule エンコーダーのパニック安全性

`Capsule` 型は `Arbitrary` を derive していないため、各 variant ごとの構造化入力を fuzz target 内で定義する。

`CapsuleEncoder::encode` に任意の `Capsule` を渡してパニック安全性を検証する。`CapsuleEncoder::encode` の戻り値は `()` であり `Result` を返さない。

考慮すべき制約:
- `CapsuleEncoder` 内部で `varint::encode(...).expect(...)` を呼ぶため、`stream_id` / `error_code` / `maximum` 等の `u64` フィールドが varint の最大値 (`MAX_VALUE = 4_611_686_018_427_387_903`, 2^62-1) を超えるとパニックする。`FuzzCapsule` の `u64` フィールドは `% (MAX_VALUE + 1)` で制限するか、varint 範囲内の値を生成する `Arbitrary` を手動実装する
- `Padding`: `CapsuleEncoder::encode` は `length` バイト分のゼロ埋めをメモリ確保する。`length` を `u16` (最大 65535) に制限して OOM を回避する (実型は `usize` だが fuzzer の安全性のため意図的に制限)
- `WtCloseSession`: `reason` は UTF-8 文字列。エンコーダーは 1024 バイトで切り詰める

```rust
#[derive(Debug, Arbitrary)]
enum FuzzCapsule {
    Datagram { data: Vec<u8> },
    // length は u16 に制限 (エンコーダーが length バイト分のゼロ埋めを
    // メモリ確保するため、usize のまま生成すると OOM になる)
    Padding { length: u16 },
    WtResetStream { stream_id: u64, error_code: u64, reliable_size: u64 },
    WtStopSending { stream_id: u64, error_code: u64 },
    WtStream { stream_id: u64, data: Vec<u8>, fin: bool },
    WtMaxData { maximum: u64 },
    WtMaxStreamData { stream_id: u64, maximum: u64 },
    WtMaxStreams { maximum: u64, bidirectional: bool },
    WtDataBlocked { maximum: u64 },
    WtStreamDataBlocked { stream_id: u64, maximum: u64 },
    WtStreamsBlocked { maximum: u64, bidirectional: bool },
    WtCloseSession { error_code: u32, reason: Vec<u8> },
    WtDrainSession,
    Unknown { capsule_type: u64, data: Vec<u8> },
}
```

`FuzzCapsule` から `Capsule` への変換時:
- 全ての `u64` フィールドは `% (varint::MAX_VALUE + 1)` で varint 範囲内に制限する (エンコーダー内部の `expect` パニック回避)
- `WtCloseSession` の `reason` は `String::from_utf8_lossy` で変換する (不正 UTF-8 は置換文字 U+FFFD に変換されるため、エンコーダーには常に valid UTF-8 が渡される)

**6. `fuzz_header_field`** — HeaderField 構築のパニック安全性

任意の name/value バイト列を `HeaderField::new` / `HeaderField::new_with_sensitive` に渡し、パニック安全性のみを検証する。

```rust
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    name: Vec<u8>,
    value: Vec<u8>,
    sensitive: bool,
}

fuzz_target!(|input: FuzzInput| {
    let _ = HeaderField::new_with_sensitive(&input.name, &input.value, input.sensitive);
});
```

**7. `fuzz_hpack_sequential`** — HPACK 連続操作のパニック安全性

複数ラウンドのヘッダーリストを連続でエンコード/デコードし、動的テーブルの状態遷移に対するパニック安全性を検証する。テーブルサイズ変更も含む。

`FuzzHeader` から `HeaderField` への変換は既存の wire ヘルパ方式を使用する。

テーブルサイズ変更時の手順 (RFC 7541 §4.2):
1. `HpackEncoder::set_max_table_size` で新サイズを設定する
2. `HpackEncoder::encode_size_update` で Dynamic Table Size Update をヘッダーブロックの先頭にエンコードする
3. `HpackDecoder::set_max_table_size` で許容上限を更新する (実際のテーブルサイズ変更は Dynamic Table Size Update のデコード時に行われる)。デコーダー側は `encode_size_update` のデコードより前に呼ぶ

```rust
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_table_size: u32,
    rounds: Vec<FuzzRound>,
}

#[derive(Debug, Arbitrary)]
struct FuzzRound {
    headers: Vec<FuzzHeader>,
    new_table_size: Option<u32>,
}

// rounds は先頭 64 件に切り詰める (fuzzer のスループット確保のため)

fuzz_target!(|input: FuzzInput| {
    let table_size = input.initial_table_size as usize;
    let mut encoder = HpackEncoder::new(table_size);
    let mut decoder = HpackDecoder::new(table_size);

    for round in input.rounds.iter().take(64) {
        let mut buf = Vec::new();

        if let Some(new_size) = round.new_table_size {
            let new_size = new_size as usize;
            encoder.set_max_table_size(new_size);
            encoder.encode_size_update(&mut buf, new_size);
            decoder.set_max_table_size(new_size);
        }

        let headers: Vec<HeaderField> = round.headers.iter()
            .map(|h| wire_header_field(&h.name, &h.value))
            .collect();
        encoder.encode(&mut buf, &headers);
        let _ = decoder.decode(&buf);
    }
});
```

#### 低優先度

**8. `fuzz_flow_control`** — HTTP/2 フロー制御の連続操作

`FlowControl` に対して `consume_send` / `consume_recv` / `recv_window_update` / `add_recv_window` / `update_initial_window_size` を任意順序で呼び出し、パニック安全性とオーバーフロー/アンダーフロー耐性を検証する。

`FlowControl::consume_send` / `consume_recv` は `usize` 引数を取る。`Arbitrary` で `usize` を直接生成すると巨大値になりすぎるため、`u32` で生成して `as usize` で変換する。これにより初期ウィンドウサイズ (最大 2^31-1) を超える入力も生成できる。

```rust
#[derive(Debug, Arbitrary)]
enum FlowAction {
    ConsumeSend(u32),
    ConsumeRecv(u32),
    RecvWindowUpdate(u32),
    AddRecvWindow(u32),
    UpdateInitialWindowSize(u32),
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_window_size: u32,
    actions: Vec<FlowAction>,
}

fuzz_target!(|input: FuzzInput| {
    let mut fc = FlowControl::new(input.initial_window_size);
    for action in &input.actions {
        match action {
            FlowAction::ConsumeSend(v) => { let _ = fc.consume_send(*v as usize); }
            FlowAction::ConsumeRecv(v) => { let _ = fc.consume_recv(*v as usize); }
            FlowAction::RecvWindowUpdate(v) => { let _ = fc.recv_window_update(*v); }
            FlowAction::AddRecvWindow(v) => { let _ = fc.add_recv_window(*v); }
            FlowAction::UpdateInitialWindowSize(v) => { let _ = fc.update_initial_window_size(*v); }
        }
    }
});
```

**9. `fuzz_wt_flow_control`** — WebTransport フロー制御の連続操作

`WtFlowControl` に対して全ての公開メソッドを任意順序で呼び出し、パニック安全性を検証する。

`WtFlowControl` は `u64` ベースのフロー制御で HTTP/2 の `FlowControl` (`i64`/`usize` ベース) とは型も振る舞いも異なる。

```rust
#[derive(Debug, Arbitrary)]
enum WtFlowAction {
    ConsumeSend(u64),
    ConsumeRecv(u64),
    UpdateSendMax(u64),
    AddRecvMax(u64),
    UpdateMaxStreams { maximum: u64, bidirectional: bool },
    AddMaxStreamsLocal { increment: u64, bidirectional: bool },
    OpenedStream { bidirectional: bool },
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    initial_max_data: u64,
    max_streams_bidi: u64,
    max_streams_uni: u64,
    actions: Vec<WtFlowAction>,
}

fuzz_target!(|input: FuzzInput| {
    let mut fc = WtFlowControl::new(
        input.initial_max_data,
        input.max_streams_bidi,
        input.max_streams_uni,
    );
    for action in &input.actions {
        match action {
            WtFlowAction::ConsumeSend(v) => { let _ = fc.consume_send(*v); }
            WtFlowAction::ConsumeRecv(v) => { let _ = fc.consume_recv(*v); }
            WtFlowAction::UpdateSendMax(v) => { let _ = fc.update_send_max(*v); }
            WtFlowAction::AddRecvMax(v) => { let _ = fc.add_recv_max(*v); }
            WtFlowAction::UpdateMaxStreams { maximum, bidirectional } => {
                let _ = fc.update_max_streams(*maximum, *bidirectional);
            }
            WtFlowAction::AddMaxStreamsLocal { increment, bidirectional } => {
                fc.add_max_streams_local(*increment, *bidirectional);
            }
            WtFlowAction::OpenedStream { bidirectional } => {
                fc.opened_stream(*bidirectional);
            }
        }
    }
});
```

### Cargo.toml への追記

`fuzz/Cargo.toml` に各 `[[bin]]` エントリを追加する。既存の形式に従う。

### CHANGES.md

`## develop` の `### misc` に `[ADD]` エントリを追記する。既存の `[ADD]` エントリ群の後、`[UPDATE]` エントリの前に挿入する (CLAUDE.md の種別順序 CHANGE -> ADD -> UPDATE -> FIX に従う)。

## 完了条件

- [ ] 以下の 9 個の fuzz target が `fuzz/fuzz_targets/` に追加されている:
  - `fuzz_frame_encoder.rs`
  - `fuzz_connection_client.rs`
  - `fuzz_connection_preface.rs`
  - `fuzz_connection_interactive.rs`
  - `fuzz_capsule_encoder.rs`
  - `fuzz_header_field.rs`
  - `fuzz_hpack_sequential.rs`
  - `fuzz_flow_control.rs`
  - `fuzz_wt_flow_control.rs`
- [ ] `fuzz/Cargo.toml` に全 `[[bin]]` エントリが追加されている
- [ ] `cargo check --manifest-path fuzz/Cargo.toml` が通る
- [ ] `cargo clippy --manifest-path fuzz/Cargo.toml -- -D warnings` が通る
- [ ] 各 fuzz target が `cargo fuzz run <target> -- -runs=0` (コンパイル確認) で成功する
- [ ] 各 fuzz target が `cargo fuzz run <target> -- -runs=100` (短時間実行) でパニックしないことを確認する
- [ ] `CHANGES.md` の `### misc` に `[ADD]` エントリが追加されている

## 解決方法

以下の 9 個の fuzz target を `fuzz/fuzz_targets/` に追加した:

1. `fuzz_frame_encoder.rs` — `FrameEncoder::encode` に対する構造化入力からのパニック安全性検証
2. `fuzz_connection_client.rs` — クライアントロールでの任意バイト列受信のパニック安全性検証
3. `fuzz_connection_preface.rs` — サーバーロールでプリフェイス未処理状態での断片的入力のパニック安全性検証
4. `fuzz_connection_interactive.rs` — ローカル操作とリモート入力の交互実行による複合状態遷移のパニック安全性検証
5. `fuzz_capsule_encoder.rs` — `CapsuleEncoder::encode` に対する構造化入力からのパニック安全性検証 (varint 範囲制限つき)
6. `fuzz_header_field.rs` — `HeaderField::new` / `HeaderField::new_with_sensitive` への任意バイト列入力のパニック安全性検証
7. `fuzz_hpack_sequential.rs` — 複数ラウンドの HPACK エンコード/デコードでの動的テーブル状態遷移のパニック安全性検証
8. `fuzz_flow_control.rs` — `FlowControl` の全公開メソッドの任意順序呼び出しによるパニック安全性検証
9. `fuzz_wt_flow_control.rs` — `WtFlowControl` の全公開メソッドの任意順序呼び出しによるパニック安全性検証

`fuzz/Cargo.toml` に全 `[[bin]]` エントリを追加し、`cargo check` / `cargo clippy` / `cargo fuzz run <target> -- -runs=100` が全て通ることを確認した。

## 関連

- [[0045-refactor-remove-test-helpers-feature]] (fuzz 基盤の wire ヘルパ移行、完了済み)
- [[0037-add-fuzz-build-check-to-ci]] (CI での fuzz コンパイル確認)
