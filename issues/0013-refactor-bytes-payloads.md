# HTTP/2 ペイロードを Bytes 化する

- Created: 2026-05-07
- Model: Opus 4.7

## 概要

`shiguredo_http2` ルートクレートに `bytes` クレート (1.x) を依存追加し、HTTP/2 protocol 層 (frame, HPACK, event) と WebTransport 層 (capsule, event, session) のバイト列ペイロードを `Vec<u8>` から `bytes::Bytes` / `bytes::BytesMut` に置換する。

ルートクレートの「依存ゼロ」方針からの脱却を伴う。依存ゼロ方針は no_std 化で別途担保する (別 issue)。

## 背景

ルートクレートは sans-io ライブラリとして「依存ゼロ」を売りにしてきたが、以下の動機から `bytes` 導入を検討する:

- WebTransport over HTTP/2 のメインユースケースとして relay (1:N 配信) を想定。サーバーが受信したストリームデータを N 個の宛先に転送する際、現状は `Vec::clone()` (alloc + memcpy) が N 回発生する。`bytes::Bytes` に置換すれば clone は Arc inc になり、N がそのまま削減倍率になる。
- 受信経路全体で `Vec<u8>` のコピーが連続している:
  - `FrameDecoder::decode` (`src/frame/decoder.rs:92`): `self.buf.drain(..payload_len).collect()`
  - `decode_data` (`src/frame/decoder.rs:205`): `payload.to_vec()` (2 回目のコピー)
  - `Event::DataReceived { data: Vec<u8> }` で move
  - `CapsuleDecoder::decode_payload` (`src/webtransport/capsule.rs:425`): `payload[len..].to_vec()`
  - `WtEvent::StreamData { data: Vec<u8> }` で move
  - `WtSession::send_stream_data` (`src/webtransport/mod.rs:342`): `Capsule::WtStream { data: data.to_vec() }` (再コピー)
  - `CapsuleEncoder` / `FrameEncoder` で `extend_from_slice(data)` (バッファへコピー)
- HPACK の `HeaderField { name: Vec<u8>, value: Vec<u8> }` も decode/encode で頻繁に複製される。

## 根拠

- `bytes` は Rust の事実上のデファクトで、tokio/rustls/hyper など主要クレートと組み合わせやすい
- `Bytes` の Arc inc clone は relay の N に対してそのまま効くため、性能改善の効果が読みやすい
- `BytesMut::split_to(n).freeze()` で受信デコード経路の zero-copy 化が可能
- 後続の no_std 化でも `bytes = { version = "1", default-features = false }` で alloc 構成にできるため、将来の no_std 化と矛盾しない
- ルートクレートが「依存ゼロ」を貫く方針は no_std 化で別途担保する (別 issue)

## スコープ

`shiguredo_http2` ルートクレートのバイト列ペイロード全般を対象とする。型を持たない透明バイト列だけが対象で、UTF-8 文字列 (`String`) は触らない。

### 依存追加

- `Cargo.toml` の `[dependencies]` に `bytes = "1"` を追加
- `src/lib.rs` の「0 依存」記述を更新する:
  - `//! - 0 依存: 標準ライブラリのみを使用` → `//! - 最小依存: bytes クレートのみに依存 (no_std 化時に alloc 構成に変更予定)`

### HTTP/2 frame 層 (`src/frame/`)

- `DataFrame.new(stream_id: StreamId, data: Vec<u8>) → new(stream_id: StreamId, data: impl Into<Bytes>)`
- `HeadersFrame.new(stream_id: StreamId, header_block_fragment: Vec<u8>) → new(stream_id: StreamId, header_block_fragment: impl Into<Bytes>)`
- `ContinuationFrame.new(stream_id: StreamId, header_block_fragment: Vec<u8>) → new(stream_id: StreamId, header_block_fragment: impl Into<Bytes>)`
- `GoawayFrame::with_debug_data(self, debug_data: Vec<u8>) → with_debug_data(self, debug_data: impl Into<Bytes>)`
- `PriorityUpdateFrame::new(prioritized_element_id: StreamId, priority_field_value: Vec<u8>) → new(prioritized_element_id: StreamId, priority_field_value: impl Into<Bytes>)`
- `FrameDecoder` の内部 `buf: Vec<u8>` を `BytesMut` に置換、ペイロード切り出しを `split_to(payload_len).freeze()` で zero-copy 化
- `Frame::Unknown { payload: Vec<u8> }` → `Bytes`
- `decode_data` 等で発生していた `payload.to_vec()` を排除 (Bytes をそのまま格納)
- `FrameEncoder` の内部 `buf: Vec<u8>` を `BytesMut` に置換、`take()` の戻り値を `Bytes` に変更
- `encode_frame_to_vec` は **0019 (remove-dead-code) を本 issue の前に先行実施**し、削除済みであることを前提とする。0013 では `encode_frame` や `encode_frame_to_vec` の公開関数シグネチャを変更しない (`FrameEncoder` 内部で `BytesMut` の `Deref<Target=[u8]>` を経由して既存 API が動作するため)

### HPACK 層 (`src/hpack/`)

- `HeaderField.name: Vec<u8>` → `Bytes`
- `HeaderField.value: Vec<u8>` → `Bytes`
- `HeaderField::new(name: Vec<u8>, value: Vec<u8>)` → `new(name: impl Into<Bytes>, value: impl Into<Bytes>)`
- `HeaderField::with_sensitive(self, sensitive: bool) -> Self` を追加し、`new_sensitive` は削除する。`decoder.rs:168` の `new_sensitive` 呼び出しを `new` + `with_sensitive` に変更する
- `HeaderField::from_str(name: &str, value: &str)` の内部実装を `Bytes::copy_from_slice(name.as_bytes())` に変更する
- `HeaderField::sensitive(name: &str, value: &str)` も同様に Bytes 化に追従する
- `DynamicTable::insert(name: Vec<u8>, value: Vec<u8>)` → `insert(name: impl Into<Bytes>, value: impl Into<Bytes>)`
  - エンコーダー側の呼び出し (`name.to_vec()`, `value.to_vec()`) は `name.clone()`, `value.clone()` に変更し、`Bytes::clone()` (Arc inc) を利用する
- `StaticEntry::to_header_field()` で `Bytes::from_static(&'static [u8])` を使い、静的テーブルエントリは zero-allocation に
- `Decoder::decode` の戻り値 `Vec<HeaderField>` 内部の name/value が Bytes になる
- `Decoder::decode_string` の戻り値 `Vec<u8>` を `Bytes` に変更する。
  - Huffman デコード後は所有値の `Vec<u8>` なので `Bytes::from(vec)` で移行 (追加 alloc なし)
  - リテラル (非 Huffman) は `&[u8]` からの変換のため `Bytes::copy_from_slice(data)` でコピーが発生する。`Decoder` は内部バッファを持たないため `split_to` による zero-copy は不可 (HPACK decode の入力 `&[u8]` シグネチャは据え置きのため)
- `Encoder::encode` 系の `buf: &mut Vec<u8>` 引数は据え置き (HPACK エンコーダの API は呼び出し側が出力バッファを渡すスタイル)。`buf: &mut BytesMut` 版の追加は本 issue では行わない。`Bytes::from(encoded_headers)` は `Vec<u8>` のアロケーションを再利用するため追加の alloc/copy は発生しない

### 接続層 (`src/connection/`)

- `Connection` の公開 API (`send_data`, `send_goaway`, `poll_output`) のシグネチャ変更を含むため、本 issue で明示的に変更する:
  - `send_data(stream_id, data: Vec<u8>, end_stream)` → `data: impl Into<Bytes>` を受け付ける
  - `send_goaway(error_code, debug_data: Vec<u8>)` → `debug_data: impl Into<Bytes>`
  - `poll_output() -> Option<Vec<u8>>` → `Option<Bytes>` (sans-io 境界の出力側も Bytes 化し、接続層まで zero-copy にする)
  - `output_buffer: VecDeque<u8>` → `BytesMut` に変更する。`send_frame` では `frame_encoder.encode(frame)` 後に `take()` で `Bytes` を取り出し、`output_buffer.extend_from_slice(&encoded)` で蓄積する。`take()` によりエンコーダー内部バッファはクリアされるため、ループで `send_frame` を繰り返し呼ぶ既存のパターンとも整合する
  - `header_block_fragment: Vec<u8>` (HEADERS + CONTINUATION の蓄積用) → `BytesMut` に変更する。`extend_from_slice` で断片を蓄積し、最後に `split_to(all).freeze()` で HPACK decoder に渡す
  - `concat_cookies` の実装変更:
    - `cookie_values: Vec<Vec<u8>>` → `Vec<Bytes>` に変更
    - `join(&b"; "[..])` を `BytesMut` による逐次追記に置換:
      ```rust
      let mut concatenated = BytesMut::new();
      for (i, value) in cookie_values.iter().enumerate() {
          if i > 0 {
              concatenated.extend_from_slice(b"; ");
          }
          concatenated.extend_from_slice(value);
      }
      let concatenated = concatenated.freeze();
      ```
    - name が `Bytes` になったため、`HeaderField` 構築は `HeaderField::new(Bytes::from_static(b"cookie"), concatenated).with_sensitive(cookie_sensitive)` に変更
  - `initiate()` の `self.output_buffer.extend(CONNECTION_PREFACE)` → `extend_from_slice(CONNECTION_PREFACE)` に変更 (output_buffer の BytesMut 化に追従)
  - `preface_buffer: Vec<u8>` は据え置き (プリフェイス検証用の一時バッファのため型変更不要)
- `Connection` の内部メソッドへの影響:
  - `send_header_block`: `encoded_headers: Vec<u8>` → `Bytes::from(encoded_headers)` で変換する。`Bytes::from(Vec<u8>)` は Vec のアロケーションを再利用するため追加の alloc/copy は発生しない。分割時 (`first_chunk.to_vec()` 相当) は `Bytes::slice()` で zero-copy に切り出す
  - `handle_headers`: `end_headers == false` の場合のみ `self.header_block_fragment.extend_from_slice(&frame.header_block_fragment)` に変更。`end_headers == true` では `frame.header_block_fragment` を直接 `hpack_decoder.decode()` に渡すため代入不要。CONTINUATION 受信時も `extend_from_slice` で蓄積。HPACK decoder に渡す際は `split_to(all).freeze()` で `Bytes` として取り出す

### Stream バッファ層 (`src/stream/`)

- `SendBuffer` と `RecvBuffer` の内部バッファを `VecDeque<u8>` → `BytesMut` に変更する。
  - 現在の実装は `push_back` (`extend_from_slice` 相当) と `pop_front` (`split_to` + `freeze` 相当) のみを使用しており、`push_front` 等の逆方向操作は存在しないことを確認済み。`BytesMut` で問題なく置換可能。
  - `SendBuffer::push(&[u8])` → `push(data: Bytes)` に変更する。上限 (`max_size`) は `BytesMut::len()` で現在長を取得し、`max_size.saturating_sub(current_len)` で上限を計算し、`data.slice(0..limit)` 相当で制限付き追加を行う
  - `SendBuffer::pop(usize) -> Vec<u8>` → `pop(usize) -> Bytes` に変更する (`BytesMut::split_to(limit).freeze()` を使用)
  - `RecvBuffer::pop(usize) -> Vec<u8>` → `pop(usize) -> Bytes` に同様に変更する
- `Stream` 構造体の `Vec<u8>` フィールド:
  - `request_method: Option<Vec<u8>>` → `Option<Bytes>`
  - `protocol: Option<Vec<u8>>` → `Option<Bytes>`
  - `start_stream` / `send_response` / `send_trailers` により設定されるヘッダーリストも `Vec<HeaderField>` (内部 `Bytes`) に自然に追従

### Event 層 (`src/event.rs`)

- `Event::HeadersReceived.protocol: Option<Vec<u8>>` → `Option<Bytes>`
- `Event::DataReceived.data: Vec<u8>` → `Bytes`
- `Event::GoawayReceived.debug_data: Vec<u8>` → `Bytes`
- `Event::PriorityUpdateReceived.priority_field_value: Vec<u8>` → `Bytes`
- `Event::TrailersReceived` は `Vec<HeaderField>` で、HeaderField 内部が Bytes になることで自然に追従

### WebTransport 層 (`src/webtransport/`)

- `Capsule::Datagram.data: Vec<u8>` → `Bytes`
- `Capsule::WtStream.data: Vec<u8>` → `Bytes`
- `Capsule::Unknown.data: Vec<u8>` → `Bytes`
- `Capsule::WtCloseSession.reason: String` は据え置き (UTF-8 検証絡みで別 issue)
- `WtEvent::StreamData.data: Vec<u8>` → `Bytes`
- `WtEvent::DatagramReceived.data: Vec<u8>` → `Bytes`
- `CapsuleEncoder` の内部 `buffer: Vec<u8>` を `BytesMut` 化、`take()` の戻り値を `Bytes` に変更
- `CapsuleDecoder` の内部 `buffer: Vec<u8>` を `BytesMut` 化、ペイロード切り出しで `split_to(payload_len).freeze()` を使い zero-copy
- `WtSession.output_buffer: VecDeque<u8>` → `BytesMut`
  - 現在の `self.output_buffer.extend(self.capsule_encoder.take())` パターンは、`CapsuleEncoder::take()` が `Bytes` を返すように変更した後、`BytesMut::extend_from_slice(&bytes)` に書き換える
  - ただし `VecDeque` の `drain(..).collect()` による全取出しに相当する操作は `BytesMut` にはないため、`poll_output` では `self.output_buffer.len()` を取得後 `split_to(len).freeze()` で `Bytes` に変換する
- `WtSession::poll_output() -> Option<Vec<u8>>` → `Option<Bytes>`
- `WtSession::send_stream_data(stream_id, data: &[u8], fin)` → `data: Bytes` を直接受け取る
- `WtSession::send_datagram(data: &[u8])` → `data: Bytes`

### 入力 API (sans-io 境界) は据え置き

- `FrameDecoder::feed(&mut self, data: &[u8])` シグネチャ維持 (内部で `BytesMut::extend_from_slice` するが、sans-io API としての柔軟性を優先)
- `CapsuleDecoder::feed(&mut self, data: &[u8])` 同様
- `WtSession::feed(&mut self, data: &[u8])` 同様

入力時の 1 回のコピーは許容する。出力側 (デコード結果の Bytes) は zero-copy で複数宛先に配布可能。

### `tokio-http2` クレートの追従修正 (`crates/tokio-http2/`)

- 公開 API (`WtBidiStream::send/recv`、`WtUniSendStream::send`、`WtUniRecvStream::recv`、`WtServerSession::send_datagram/recv_datagram` 等) を `Bytes` ベースに変更
- driver 内部の `mpsc::UnboundedSender<Vec<u8>>` を `Bytes` 化
- `StreamPacket::Data { data: Vec<u8> }` を `Bytes` 化
- `ServerConnection::send_data` 等の DATA フレーム送出 API も `Bytes` 受け取りに変更
- 公開 API の breaking change として CHANGES.md に明記

### `examples/` の追従修正

- `examples/wt_server/src/main.rs`: bidi/uni の `bidi.send(data, false)` の `data` が `Bytes` になるので、エコーロジックは `data` をそのまま渡せばよい (現状すでに move しているだけなので、Bytes 化で型注釈の調整のみ)
- `examples/http2_server/`, `examples/http2_client/`: 同様に追従修正

### CHANGES.md (実装時に追記)

- `## develop` に以下を追加する。また、既存の `[ADD] Event::HeadersReceived に protocol: Option<Vec<u8>>` エントリの型表記を `Option<Bytes>` に更新する:
  - `[ADD]` `shiguredo_http2` が `bytes` クレートに依存するように変更する
    - @voluntas
  - `[CHANGE]` `Frame / HeaderField / Event / Capsule / WtEvent / WtSession / Connection / Stream のバイト列ペイロードを Vec<u8> から bytes::Bytes に変更する`
    - @voluntas
  - `[CHANGE]` `tokio-http2 の WebTransport ストリーム / DATAGRAM API を bytes::Bytes ベースに変更する`
    - @voluntas

## 非スコープ (本 issue では行わない)

別 issue として後続で対応する:

- `String` → `Bytes` 置換 (Capsule::WtCloseSession.reason、Error::reason 等は UTF-8 検証絡みで別 issue)
- `no_std` 化 (`#![no_std]` + `extern crate alloc;`)
- `hashbrown` 導入による `std::collections::HashMap` 置換
- `Backtrace` 削除
- `Encoder::encode(buf: &mut Vec<u8>, ...)` の `&mut BytesMut` 版追加
- relay 性能ベンチマーク (本 issue 完了後に別途。Vec<u8> 版 baseline と比較)
- `huffman::encode_to_vec` / `huffman::decode` の戻り値型変更 (huffman モジュール自体の最適化は別途)

## テスト戦略

### PBT

`Vec<u8>` を生成している proptest strategy を `Bytes` 用に変更する。`bytes` crate の `Bytes` は proptest の `Arbitrary` を実装していないため、以下のラッパー strategy を使用する:

```rust
prop::collection::vec(any::<u8>(), ..).prop_map(Bytes::from)
```

frame / event / capsule の roundtrip PBT は `PartialEq` が `Bytes` でも動作するため引き続き機能する。

### 単体テスト

- `HeaderField` の `from_str` / `new` / `sensitive` コンストラクタの型変更に追従
- `Bytes::from_static` による静的テーブルエントリの zero-allocation 検証
- `Bytes::clone()` (Arc inc) の振る舞い検証 (共有参照が互いに影響しないこと)
- `Bytes::slice()` の zero-copy 参照検証 (スライス元とスライス先の内容が同一であること、一方の drop が他方に影響しないこと)

### Fuzzing

- `fuzz_frame_decoder` / `fuzz_hpack_decoder` / `fuzz_hpack_roundtrip` / `fuzz_capsule_decoder` / `fuzz_connection` のターゲットを `BytesMut` 使用に追従
- クラッシュ耐性検証 (パニック安全性) は既存 fuzz ターゲットで継続

## 依存・先行 issue

以下の issue 番号の順序を考慮する:

- **0019 (remove-dead-code)**: **先行必須 issue**。`encode_frame_to_vec` を 0019 で事前に削除する。0013 開始時にこの関数が存在しないことを前提とする
- **0015 (split-connection-module)**: Connection モジュールの分割と本 issue の型変更は競合する可能性がある。できれば 0015 の後に 0013 を実施するか、0015 実施時に Bytes 型を考慮する
- **0016 (improve-stream-cohesion)**: Stream 構造体の再編と本 issue の `request_method` / `protocol` フィールド型変更は競合する
- **0020 (event-non-exhaustive)**: Event に `#[non_exhaustive]` を付与すると本 issue での Event 型変更が breaking change としての衝撃を和らげられる

## RFC 関連

本 issue は内部表現の型置換のみであり、wire format やプロトコル要件に変更はない。ただし、以下の RFC 節が各フィールドの「不透明バイト列」としての位置づけの根拠となる:

| 対象フィールド | 該当 RFC 節 |
|---|---|
| `Frame.payload` / フレームペイロード全般 | RFC 9113, Section 4.1 (Frame Format) |
| `DataFrame.data` | RFC 9113, Section 6.1 |
| `HeadersFrame.header_block_fragment` | RFC 9113, Section 6.2 |
| `GoawayFrame.debug_data` | RFC 9113, Section 6.8 |
| `PriorityUpdateFrame.priority_field_value` | RFC 9218, Section 7.1 (ASCII text だが opaque octets として扱う) |
| `HeaderField.name` / `HeaderField.value` | RFC 7541, Section 1.3 |
| 静的テーブル | RFC 7541, Appendix A |
| 動的テーブル | RFC 7541, Section 2.3.2 |
| `Capsule Value` ペイロード | RFC 9297, Section 3.2 |
| `WT_STREAM.Stream Data` | draft-ietf-webtrans-http2-14, Section 6.4 |
| `DATAGRAM.HTTP Datagram Payload` | RFC 9297, Section 3.5 |

## 設計上の注意

### `BytesMut::split_to` のセマンティクス

`BytesMut::split_to(at)` は前半を `BytesMut` として切り出して所有権ごと渡し、`freeze()` で `Bytes` に変換できる。`Bytes` 同士の `clone()` は Arc 参照カウント増加だけで zero-copy。受信デコード経路 (FrameDecoder, CapsuleDecoder) の内部バッファを `BytesMut` にすれば、`split_to(payload_len).freeze()` でペイロードを zero-copy に切り出せる。

### `BytesMut` のキャパシティ管理

`BytesMut::extend_from_slice` は内部バッファが満杯なら再 alloc する。`split_to` 後も残りバッファは保持されるため、繰り返し feed する HTTP/2 のストリーミングデコードと相性がよい (現状の `Vec::extend_from_slice` + `Vec::drain` と意味的に等価で、性能特性も同等以上)。

### HPACK 静的テーブルの `Bytes::from_static`

HPACK 静的テーブル (61 エントリ) は `'static [u8]` の文字列リテラルから生成しているため、`Bytes::from_static(&'static [u8])` を使えばヒープ確保なしで `Bytes` を生成できる。デコード時の `get_header_by_index` 等で静的テーブルから引いた `HeaderField` を返す経路もそのまま zero-copy になる。

### HPACK 動的テーブルの所有権

動的テーブルは `Vec<HeaderField>` で entries を保持し、受信ヘッダーを `insert(name, value)` で追加する。Bytes 化後は `entries: Vec<HeaderField { name: Bytes, value: Bytes }>` となり、`get` で参照を返すスタイルは維持。decode の戻り値 `Vec<HeaderField>` は静的・動的テーブルから引く場合に `clone()` が必要だが、`Bytes::clone()` は Arc inc なので安価。

### `tokio-http2` API の breaking change

`WtBidiStream::send(data: Vec<u8>, fin: bool)` 等を `data: Bytes` に変えるのは公開 API の breaking change。`canary` 版段階なので許容するが、CHANGES.md に明示する。`examples/wt_server` の修正で実際の使い勝手も検証する。

### `Bytes` の `Eq` / `PartialEq`

`Frame`, `Event`, `Capsule` は `#[derive(Debug, Clone, PartialEq, Eq)]` を持つ。`Bytes` は `PartialEq` / `Eq` を実装しているため derive はそのまま動く (PBT の往復検証で内容比較が引き続き機能する)。

## 影響範囲

- `Cargo.toml`, `Cargo.lock`
- `src/lib.rs` (依存ゼロ記述の更新)
- `src/frame/mod.rs`, `src/frame/decoder.rs`, `src/frame/encoder.rs`
- `src/hpack/table.rs`, `src/hpack/decoder.rs`, `src/hpack/dynamic_table.rs`, `src/hpack/encoder.rs`
- `src/event.rs`
- `src/connection/mod.rs` (Frame / HeaderField / Bytes の利用箇所多数。内部メソッドの Bytes 化)
- `src/stream/mod.rs`, `src/stream/buffer.rs` (SendBuffer / RecvBuffer / Stream の Vec<u8> フィールド変更)
- `src/webtransport/capsule.rs`, `src/webtransport/mod.rs`
- `tests/` (test_webtransport.rs)
- `pbt/tests/prop_frame.rs`, `prop_hpack.rs`, `prop_event.rs`, `prop_dynamic_table.rs`, `prop_connection.rs`, `prop_webtransport.rs`, `prop_stream_state.rs`, `prop_flow_control.rs`
- `fuzz/fuzz_targets/fuzz_frame_decoder.rs`, `fuzz_hpack_decoder.rs`, `fuzz_hpack_roundtrip.rs`, `fuzz_capsule_decoder.rs`, `fuzz_connection.rs`
- `crates/tokio-http2/src/connection.rs`, `client.rs`, `server.rs`, `webtransport.rs`
- `examples/wt_server/`, `examples/http2_server/`, `examples/http2_client/`

## 受け入れ基準

- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
- `cargo +nightly fuzz` ターゲットがビルドできる (実行は別タスク)
- `examples/wt_server` が手動動作確認で bidi/uni/datagram のエコーを行える
- `CHANGES.md` の `## develop` に該当エントリ追加
- `Cargo.lock` の更新をコミット
