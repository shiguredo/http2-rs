# HTTP/2 ペイロードを Bytes 化する (お試し)

- Created: 2026-05-07
- Reopened: 2026-05-07 (送信側 encoder/output_buffer/stream buffer の抜け漏れ)
- Reopened: 2026-05-07 (HPACK ヘッダーブロック分割経路の抜け漏れ + varint デッドコード)
- Reopened: 2026-05-07 (README サンプルとテスト 1 箇所が新 API に追従していない)
- Reopened: 2026-05-07 (空 DATA フレーム生成で `vec![]` が残っていた)
- Reopened: 2026-05-07 (encoder 型に出力先バッファ重複と memcpy 経路あり)
- Completed: 2026-05-07
- Model: Opus 4.7

## 再 Reopen 理由 (5 回目)

`FrameEncoder` / `CapsuleEncoder` が独立した内部 `BytesMut` を持ち、`encode()` 後に `output_buffer.extend_from_slice(encoder.buffer())` (または `&encoder.take()`) で memcpy する経路が残っていた。relay 1:N のホットパスで送信ごとに必ず memcpy が発生し、設計矛盾。

検討の結果、両 encoder は本質的に state を持たない (encoder 内 `BytesMut` は単なる出力先キャッシュ) ため、型として残す合理性がない。データ型 (`Frame` / `Capsule`) に `encode(&self, buf: &mut BytesMut)` メソッドを生やし、呼び出し側の `output_buffer` に直接書く形に変更する。

- `pub struct FrameEncoder` と `pub struct CapsuleEncoder` を削除
- `Frame::encode(&self, buf: &mut BytesMut) -> Result<()>` を追加
- `Capsule::encode(&self, buf: &mut BytesMut)` を追加
- `Connection::frame_encoder` フィールド削除、`send_frame` を `frame.encode(&mut self.output_buffer)` に置換
- `WtSession::capsule_encoder` フィールド削除、9 箇所の送信パスを `capsule.encode(&mut self.output_buffer)` に置換
- `encode_frame_to_bytes(&Frame) -> Result<Bytes>` は内部で `Frame::encode` を呼ぶ helper として維持

これにより encoder 経由の memcpy が設計上排除される (中間バッファ自体が存在しない)。`FrameDecoder` / `CapsuleDecoder` がステートフルなのは入力 byte 列がフレーム境界で来る保証がなく内部バッファリングが必須だからで、encoder 側にその制約はない。型のシンメトリーより性質のシンメトリーを優先する。

## 再 Reopen 理由 (4 回目)

`Connection::send_data` の RFC 9113 Section 6.9 に従う空 DATA + END_STREAM 送出 (src/connection/mod.rs:636) で `DataFrame::new(stream_id, vec![])` のままになっていた。`DataFrame::new` は `impl Into<Bytes>` を取るため動作はするが、`Vec::new()` から `Bytes::from(Vec)` への変換を経由する。`Bytes::new()` を直接渡せば中間変換不要で意図も明確。本番コード (src/) で `vec![]` を Bytes 受け API に渡している箇所はここ 1 箇所だけ残っていた。

## 再 Reopen 理由 (3 回目)

reopen 2 回目で本番コードの送信経路はすべて Bytes 化したが、ドキュメント / テストに 2 箇所追従漏れがあった:

- `crates/tokio-http2/README.md` L72 のサンプルコードが `b"...".to_vec()` のままで、新 API の推奨スタイル (`bytes::Bytes::from_static(b"...")`) になっていない。サンプルは「お手本」として最新の API を示すべき
- `crates/tokio-http2/tests/client_server.rs` L1615-1626 が `Vec<(StreamId, Vec<u8>)>` で受け、`status.value.to_vec()` で再アロケーションしている。`HeaderField.value` は既に `Bytes` なので `Bytes::clone()` (Arc inc) で済ませるべき

他のテストは送信側 Bytes 化のときに追従させたが、この 1 箇所だけ漏れていた。整合性のため対応する。

## 再 Reopen 理由 (2 回目)

reopen 1 回目で送信側 (encoder, output_buffer, stream buffer) を Bytes 化したが、HPACK ヘッダーブロック送信経路に以下の抜け漏れが残っていた:

- `Connection::send_header_block` (L1862) が `encoded_headers: Vec<u8>` を受け取り、CONTINUATION 分割時に `first_chunk.to_vec()` (L1903) と `chunk.to_vec()` (L1915) で alloc + memcpy が発生する。`Bytes::slice(range)` (Arc inc + offset/len の付け替えのみ、O(1)) で zero-copy 化可能
- 呼び出し側 3 箇所 (L489 / L826 / L892) の `Vec::new()` + HPACK encode 結果も `Bytes::from(vec)` で move して送信経路に流せる
- L1873 のテーブルサイズ更新時の merge も最終的に `Bytes::from()` で freeze する形に整える

加えて `webtransport::varint::encode_to_vec` (L120) が pub だがテスト内でしか呼ばれていない真のデッドコード。Capsule encoder/decoder API で完結しており外部利用想定もないため削除する。

reopen 1 回目では sans-io 入力 API (`feed(&[u8])`) は据え置きとしたが、送信経路は中間表現の zero-copy を貫徹する方針に従って今回も対応する。

## Reopen 理由 (1 回目)

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

## 解決方法 (reopen 後の追加分)

レビューで指摘された送信側の抜け漏れを以下の通り対応した:

### `shiguredo_http2` (送信側 Bytes 化)

- `frame::FrameEncoder::buf` を `Vec<u8>` から `BytesMut` に変更し、`take()` の戻り値を `bytes::Bytes` に変更 (`split().freeze()`)。`bytes::BufMut` の `put_u8` / `put_bytes` を使ったフレームヘッダー / パディング書き込みに統一
- `frame::encode_frame_to_vec` を `encode_frame_to_bytes` にリネームし、戻り値を `bytes::Bytes` に変更
- `Connection::output_buffer` を `VecDeque<u8>` から `BytesMut` に変更し、`poll_output() -> Option<Bytes>` に変更 (`split().freeze()` で zero-copy)。`webtransport::WtSession::poll_output()` と整合
- `stream::SendBuffer` / `RecvBuffer` の内部を `VecDeque<u8>` から `BytesMut` に変更し、`pop()` / `take()` の戻り値を `bytes::Bytes` に変更 (`split_to(n).freeze()` で zero-copy)。relay (1:N 配信) のホットパスで Bytes → Vec への展開と `drain().collect()` による再コピーを排除

### PBT 統一

- `prop_dynamic_table.rs` の `TableOp::Insert { name: Bytes, value: Bytes }` に変更、`byte_string()` Strategy が `Bytes` を生成
- `prop_webtransport.rs` の `SessionOp::SendDatagram(bytes::Bytes)` に変更
- `prop_connection.rs` の `encode_frame()` / `encode_valid_request_headers()` の戻り値を `Bytes` 化、`create_headers_without_end_headers` / `create_continuation` の `fragment` 引数を `impl Into<Bytes>` に変更
- `prop_frame.rs` の `Vec<Vec<u8>>` を `Vec<bytes::Bytes>` に変更

### 追加確認

- 送信側 Bytes 化により `cargo test --workspace` (全クレート) 全 pass
- `cargo clippy --workspace` pass
- `cargo fmt --check` pass

## 解決方法 (reopen 2 回目の追加分)

レビューで指摘された HPACK ヘッダーブロック送信経路の抜け漏れと、varint のデッドコードを以下の通り対応した:

### `Connection::send_header_block` の Bytes 化

- `send_header_block` の引数を `encoded_headers: Vec<u8>` から `Bytes` に変更
- 呼び出し側 3 箇所 (`request_with_options` / `respond_with_options` / `send_trailers`) で HPACK エンコーダ出力 `Vec<u8>` を `Bytes::from(...)` で move して送信経路に流す (Vec → Bytes は内部的に zero-copy)
- CONTINUATION 分割時のチャンク切り出しを `slice(..)` / `slice(start..end)` に変更し、`first_chunk.to_vec()` / `chunk.to_vec()` の alloc + memcpy を排除 (`Bytes::slice` は Arc inc + offset/len の付け替えのみで O(1))
- テーブルサイズ更新時の merge は HPACK エンコーダ API が `&mut Vec<u8>` を取るため、一旦 `Vec` に書き出してから `Bytes::from()` で freeze する形に整える

### `webtransport::varint::encode_to_vec` の削除

- pub だがテスト内 (`test_encode_to_vec`) でしか呼ばれていない真のデッドコード
- Capsule encoder/decoder API で完結しており外部利用想定もないため、関数本体とテストの両方を削除
- `encode(value, &mut buf)` (バッファを呼び出し側が用意するスタイル) は維持

### 補足: 変更しなかった箇所

- `hpack::huffman::encode_to_vec` / `decode`: ユーザー指摘通りエンコーダ側の API は据え置き、デコーダ側は新規生成データのため `alloc` 不可避。`Bytes::from(huffman::decode(...))` の Vec → Bytes 変換は move で zero-copy なので現状で最適

## 解決方法 (reopen 3 回目の追加分)

レビューで指摘された README とテストの追従漏れを以下の通り対応した:

### `crates/tokio-http2/README.md`

- L72 のサンプルコード `b"Hello, HTTP/2!".to_vec()` を `bytes::Bytes::from_static(b"Hello, HTTP/2!")` に変更。サンプルは「お手本」として新 API の推奨スタイル (`Bytes::from_static`) を示す

### `crates/tokio-http2/tests/client_server.rs`

- L1615 の `Vec<(StreamId, Vec<u8>)>` を `Vec<(StreamId, bytes::Bytes)>` に変更
- L1626 の `status.value.to_vec()` を `status.value.clone()` (Arc inc、zero-copy) に変更
- L1638 の型注釈を `&(StreamId, bytes::Bytes)` に追従、`s.as_slice()` を `&s[..]` に変更

`HeaderField.value` は既に `Bytes` なので、テスト内でも `Bytes::clone()` で済ませることで再アロケーションを排除した

## 解決方法 (reopen 4 回目の追加分)

`Connection::send_data` ループ末尾の RFC 9113 Section 6.9 に従う空 DATA + END_STREAM 送出 (src/connection/mod.rs:636) で残っていた `DataFrame::new(stream_id, vec![])` を `DataFrame::new(stream_id, Bytes::new())` に置き換えた。`Vec::new()` から `Bytes::from(Vec)` への中間変換を排除し、空 `Bytes` を直接生成する。本番コード (src/) で `Bytes` 受け API に `vec![]` を渡している箇所はこの 1 箇所のみで、grep で再確認済み。

## 解決方法 (reopen 5 回目の追加分)

`FrameEncoder` / `CapsuleEncoder` 型を削除し、`Frame::encode(&self, buf: &mut BytesMut) -> Result<()>` / `Capsule::encode(&self, buf: &mut BytesMut)` メソッドに置き換えた。encoder の中間バッファを介さず、呼び出し側 (`Connection::output_buffer` / `WtSession::output_buffer`) に直接書き込む。

### `shiguredo_http2` (encoder 型の削除と data 型メソッド化)

- `frame::encoder` モジュールを書き換え
  - `pub struct FrameEncoder` と `impl FrameEncoder` 一式を削除
  - `impl Frame { pub fn encode(&self, buf: &mut BytesMut) -> Result<()> }` を追加し、各 variant の encode 実装を private 自由関数 (`encode_data`, `encode_headers` 等) に分離
  - `put_header(buf: &mut BytesMut, header: &FrameHeader)` を private helper として再構成
  - 既存の slice 版 `encode_header(&mut [u8], ...)` / `encode_frame(&mut [u8], ...)` は据え置き (低レベル sans-io 用途)
  - `encode_frame_to_bytes(&Frame) -> Result<Bytes>` は内部で `Frame::encode(&mut BytesMut)` を呼ぶ thin helper に変更
- `webtransport::capsule` モジュールを書き換え
  - `pub struct CapsuleEncoder` と `impl CapsuleEncoder` 一式を削除
  - `impl Capsule { pub fn encode(&self, buf: &mut BytesMut) }` を追加
  - `encode_header` / `encode_varint` を private 自由関数として再構成
- `lib.rs` / `frame/mod.rs` から `pub use FrameEncoder` を削除
- `webtransport/mod.rs` から `pub use capsule::CapsuleEncoder` を削除
- `Connection::frame_encoder: FrameEncoder` フィールドを削除し、`send_frame` を `frame.encode(&mut self.output_buffer)` の 1 行に置換
- `WtSession::capsule_encoder: CapsuleEncoder` フィールドを削除し、9 箇所の送信パス (`send_stream_data` / `reset_stream` / `stop_sending` / `send_datagram` / `close` / `send_max_data` / `send_max_stream_data` / `send_max_streams` / `drain`) を `capsule.encode(&mut self.output_buffer)` に置換

### テスト

- `src/webtransport/capsule.rs` 内の `#[cfg(test)] mod tests` を新 API に書き換え (test helper `encode_to_bytes` を導入し、`Capsule::encode` 経由でエンコード)
- `tests/test_webtransport.rs` を新 API に書き換え (test helper `encode_capsule` を追加)
- `pbt/tests/prop_webtransport.rs` を全面書き換え (encoder インスタンスを使う 16 箇所すべて新 API ベースに)
- `pbt/tests/prop_connection.rs` の `encode_frame` helper を `Frame::encode(&mut BytesMut)` ベースに変更
- `pbt/tests/prop_frame.rs` の 30 箇所超の `FrameEncoder::new()` を `bytes::BytesMut::new()` に置換し、`encoder.encode(&frame)` を `frame.encode(&mut encoder)` に、`encoder.take()` を `encoder.split().freeze()` に機械的変換

### 設計上の効果

- encoder → output_buffer 間の memcpy 経路が設計上消滅 (中間バッファ自体が存在しない)
- 公開 API surface が `FrameEncoder` / `CapsuleEncoder` の 2 型分減る
- encoder を stateless 化することで Rust の標準シリアライズ慣用句 (`Display::fmt(&mut Formatter)`, `serde::Serialize::serialize(S)`) と整合
- `FrameDecoder` / `CapsuleDecoder` がステートフルなのは入力境界に対する内部バッファリングが必須だからで、encoder 側にその制約はない。型のシンメトリーより性質のシンメトリーを優先

### 確認

- `cargo test --workspace` 全 pass
- `cargo clippy --workspace --all-targets -- -D warnings` pass
- `cargo fmt --check` pass
