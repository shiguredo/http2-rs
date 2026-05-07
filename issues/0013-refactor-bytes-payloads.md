# HTTP/2 ペイロードを Bytes 化する (お試し)

- Created: 2026-05-07
- Reopened: 2026-05-07
- Model: Opus 4.7

## Reopen 理由

初回クローズ時点では受信側 (`FrameDecoder`, `CapsuleDecoder`) と Frame / HeaderField / Event / Capsule のペイロードを Bytes 化したが、送信側に以下の重大な抜け漏れがあった:

- `src/frame/encoder.rs::FrameEncoder` の内部バッファが `Vec<u8>` のまま、`take()` の戻り値も `Vec<u8>` のまま (受信側 `FrameDecoder` の `BytesMut` 化と非対称)
- `src/connection/mod.rs::Connection` の `output_buffer` が `VecDeque<u8>` のまま、`poll_output()` の戻り値が `Option<Vec<u8>>` のまま (`webtransport::WtSession::poll_output()` は既に `Option<Bytes>` 化済みで非対称)
- `src/stream/buffer.rs::SendBuffer` / `RecvBuffer` の内部が `VecDeque<u8>` のまま、`pop()` / `take()` の戻り値が `Vec<u8>` のまま。relay 配信のホットパスで Bytes → Vec への展開と `drain().collect()` による再コピーが連続して発生する

relay (1:N 配信) の zero-copy という当初の動機からも、送信側 (encoder + output_buffer + stream buffer) を Vec のまま残したのは設計矛盾。reopen して送信側まで Bytes 化を貫徹する。

## 概要

`shiguredo_http2` ルートクレートに `bytes` クレート (1.x) を依存追加し、HTTP/2 protocol 層 (frame, HPACK, event) と WebTransport 層 (capsule, event, session) のバイト列ペイロードを `Vec<u8>` から `bytes::Bytes` / `bytes::BytesMut` に置換する。

ルートクレートの「依存ゼロ」方針からの脱却を伴うため、後続の `no_std` 化、`hashbrown` 導入、`Backtrace` 削除との相性を見る **お試し issue** を兼ねる。

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
- ルートクレートが「依存ゼロ」を貫く理由 (組み込み等) は no_std 化で別途担保する方針 (別 issue で対応)

## スコープ

`shiguredo_http2` ルートクレートのバイト列ペイロード全般を対象とする。型を持たない透明バイト列だけが対象で、UTF-8 文字列 (`String`) は触らない。

### 依存追加

- `Cargo.toml` の `[dependencies]` に `bytes = "1"` を追加
- `src/lib.rs` の「0 依存」記述を更新 (現時点では `bytes` のみに依存する旨)

### HTTP/2 frame 層 (`src/frame/`)

- `DataFrame.data: Vec<u8>` → `Bytes`
- `HeadersFrame.header_block_fragment: Vec<u8>` → `Bytes`
- `ContinuationFrame.header_block_fragment: Vec<u8>` → `Bytes`
- `GoawayFrame.debug_data: Vec<u8>` → `Bytes`
- `PriorityUpdateFrame.priority_field_value: Vec<u8>` → `Bytes`
- `Frame::Unknown { payload: Vec<u8> }` → `Bytes`
- `FrameDecoder` の内部 `buf: Vec<u8>` を `BytesMut` に置換、ペイロード切り出しを `split_to(payload_len).freeze()` で zero-copy 化
- `decode_data` 等で発生していた `payload.to_vec()` を排除 (Bytes をそのまま格納)
- `FrameEncoder` の内部 `buf: Vec<u8>` を `BytesMut` に置換、`take()` の戻り値を `Bytes` に変更
- `encode_frame_to_vec` は API 互換のため `Bytes` 版 (`encode_frame_to_bytes`) を追加し、`encode_frame_to_vec` は `bytes.to_vec()` 経由で残すか deprecated 化

### HPACK 層 (`src/hpack/`)

- `HeaderField.name: Vec<u8>` → `Bytes`
- `HeaderField.value: Vec<u8>` → `Bytes`
- `HeaderField::new(name: Vec<u8>, value: Vec<u8>)` → `new(name: impl Into<Bytes>, value: impl Into<Bytes>)`
- `HeaderField::from_str(name: &str, value: &str)` の内部実装を `Bytes::copy_from_slice` に変更
- `StaticEntry::to_header_field()` で `Bytes::from_static(&'static [u8])` を使い、静的テーブルエントリは zero-allocation に
- `Decoder::decode` の戻り値 `Vec<HeaderField>` 内部の name/value が Bytes になる
- `Decoder::decode_string` の戻り値 `Vec<u8>` を `Bytes` に変更 (Huffman デコード後は所有値なので `Bytes::from(vec)`、リテラルは入力 BytesMut から `split_to` で zero-copy)
- `Encoder::encode` 系の `buf: &mut Vec<u8>` 引数は据え置き (HPACK エンコーダの API は呼び出し側が出力バッファを渡すスタイル)。`buf: &mut BytesMut` 版の追加は本 issue では行わない

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

- `## develop` に以下を追加:
  - `[CHANGE]` `Frame / HeaderField / Event / Capsule / WtEvent / WtSession のバイト列ペイロードを Vec<u8> から bytes::Bytes に変更する`
  - `[CHANGE]` `tokio-http2 の WebTransport ストリーム / DATAGRAM API を bytes::Bytes ベースに変更する`
  - `[ADD]` `shiguredo_http2 が bytes クレートに依存するように変更する`

## 非スコープ (本 issue では行わない)

別 issue として後続で対応する:

- `String` → `Bytes` 置換 (Capsule::WtCloseSession.reason、Error::reason 等は UTF-8 検証絡みで別 issue)
- `no_std` 化 (`#![no_std]` + `extern crate alloc;`)
- `hashbrown` 導入による `std::collections::HashMap` 置換
- `Backtrace` 削除
- `Encoder::encode(buf: &mut Vec<u8>, ...)` の `&mut BytesMut` 版追加
- relay 性能ベンチマーク (本 issue 完了後に別途。Vec<u8> 版 baseline と比較)
- `FrameDecoder` 内部バッファのメモリ使用量プロファイリング

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
- `src/connection/mod.rs` (Frame / Event / HeaderField の利用箇所多数)
- `src/webtransport/capsule.rs`, `src/webtransport/mod.rs`
- `tests/rfc7541.rs`, `tests/test_webtransport.rs`
- `pbt/tests/prop_frame.rs`, `prop_hpack.rs`, `prop_event.rs`, `prop_dynamic_table.rs`, `prop_connection.rs`, `prop_webtransport.rs`
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

## 解決方法

ルートクレート `shiguredo_http2` に `bytes = { version = "1.11", default-features = false }` を追加し、`shiguredo_http2` / `tokio-http2` / `examples/*` にまたがって以下の置換を実施した。

### `shiguredo_http2` (ルートクレート)

- `frame::DataFrame` / `HeadersFrame` / `ContinuationFrame` / `GoawayFrame` / `PriorityUpdateFrame` / `Frame::Unknown` のペイロードを `Vec<u8>` から `bytes::Bytes` に変更
- `FrameDecoder` の内部バッファを `BytesMut` 化し、`split_to(payload_len).freeze()` でペイロードを zero-copy に切り出す
- `FrameEncoder` の内部バッファを `BytesMut` 化し、`take()` の戻り値を `Bytes` に変更
- `hpack::HeaderField.name` / `value` を `Bytes` に変更し、`new(impl Into<Bytes>, impl Into<Bytes>)` を提供
- `hpack::StaticEntry::to_header_field` で `Bytes::from_static` を使い、HPACK 静的テーブル (61 エントリ) を zero-allocation 化
- `hpack::Decoder::decode_string` の戻り値を `Bytes` に変更
- `Event::HeadersReceived.protocol` / `DataReceived.data` / `GoawayReceived.debug_data` / `PriorityUpdateReceived.priority_field_value` を `Bytes` 化
- `webtransport::Capsule` の `Datagram` / `WtStream` / `Unknown` のペイロードを `Bytes` 化
- `webtransport::CapsuleEncoder` / `CapsuleDecoder` の内部バッファを `BytesMut` 化し、デコード時はペイロードを `split_to + freeze` で zero-copy に切り出す
- `webtransport::WtSession::output_buffer` を `BytesMut` 化、`poll_output() -> Option<Bytes>` に変更
- `webtransport::WtSession::send_stream_data` / `send_datagram` を `Bytes` 受け取りに変更
- `Connection::send_data` / `send_goaway` を `Bytes` 受け取りに変更
- `validation::ValidationError` の variant が保持するヘッダー名 / 値も `Bytes` 化
- `Stream::request_method` / `protocol` を `Option<Bytes>` 化
- `Connection` の cookie 連結ロジックを `BytesMut::with_capacity` で事前確保して 1 回のアロケーションに収めた
- 入力 API (`feed(&mut self, data: &[u8])`) は sans-io の柔軟性のため据え置き

### `tokio-http2`

- `Connection::send_data` / `send_goaway`、`ServerConnection::send_data`、`ClientConnection::send_data` を `Bytes` 受け取りに変更
- `WtBidiStream::send` / `recv`、`WtUniSendStream::send`、`WtUniRecvStream::recv` を `Bytes` ベースに変更
- `WtServerSession::send_datagram` / `WtSessionHandle::send_datagram` を `Bytes` 受け取りに変更
- driver 内部の `mpsc::UnboundedSender<Vec<u8>>` を `mpsc::UnboundedSender<Bytes>` に変更
- `StreamPacket::Data { data: Vec<u8> }` を `Bytes` 化
- `DriverCmd::SendStreamData` / `SendDatagram` の payload を `Bytes` 化

### `examples/`

- `examples/wt_server` / `examples/http2_server` の `Cargo.toml` に `bytes` 依存を追加し、`send_*` 呼び出しを `Bytes::from_static` / `Bytes::copy_from_slice` 経由に変更

### テスト

- 単体テスト / PBT / interop テストを `Bytes` 化に追従
- HPACK 比較テストは `Bytes == &[u8; N]` の impl がないため `&header.name[..] == b"..."` 形式に変更
- tokio_nghttp2 (`&[u8]` 受け) と tokio_http2 (`Bytes` 受け) を区別して扱う

### 確認

- `cargo test --workspace` 全 pass
- `cargo clippy --workspace` pass
- `cargo fmt --check` pass
