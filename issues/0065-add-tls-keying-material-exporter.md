# サーバー側 TLS Keying Material Exporter API を追加する

- Priority: Medium
- Created: 2026-06-08
- Polished: 2026-06-14
- Model: deepseek-v4-pro
- Branch: feature/add-tls-keying-material-exporter

## 目的

draft-ietf-webtrans-http2-14 Section 5.3 (L683-L708) の条件付き SHALL 要件に従い、アプリケーションが要求した場合に `EXPORTER-WebTransport` ラベルとセッション固有の Exporter Context を用いて TLS exporter を導出できるサーバー側 API を追加する。本 issue ではサーバー側 API のみを対象とし、クライアント側 API はスコープ外とする。

## 優先度根拠

WebTransport over HTTP/2 が TLS exporter をサポートする仕様上の SHALL 要件は条件付きであり、アプリケーションが exporter を要求した場合に発動する。セキュアなセッション固有の鍵素材が必要なプロトコル (例: WebTransport over QUIC からの移植プロトコル) では必須となる。一方、多くの WebTransport ユースケースで毎回必要ではないため Priority は Medium とする。

## 現状

draft-ietf-webtrans-http2-14 Section 5.3 L689-L694:

> If the application requests an exporter for a given WebTransport session with a specified label and context, the resulting exporter SHALL be a TLS exporter as defined in Section 7.5 of [TLS] with the label set to "EXPORTER-WebTransport" and the context set to the serialization of the "WebTransport Exporter Context" struct as defined below.

```
WebTransport Exporter Context {
  WebTransport Session ID (64),
  WebTransport Application-Supplied Exporter Label Length (8),
  WebTransport Application-Supplied Exporter Label (8..),
  WebTransport Application-Supplied Exporter Context Length (8),
  WebTransport Application-Supplied Exporter Context (..)
}
```

L706-L708 (Context omission):

> A TLS exporter API might permit the context field to be omitted. In this case, as with TLS 1.3, the WebTransport Application-Supplied Exporter Context becomes zero-length if omitted.

サイズ表記の解釈 (RFC 9000 Section 1.3 の慣用に基づく):

- `(64)` = 64 bit fixed (8 バイト)
- `(8)` = 8 bit fixed Length フィールド (u8 で表現、値域 0-255)
- `(8..)` / `(..)` = 可変長コンテンツ。直前の 8 bit Length フィールドで長さを表現するため最大 255 バイト。Length フィールドが権威であり、値が 0 でも許容する

Session ID は CONNECT stream ID のことである。draft-ietf-webtrans-http2-14 Section 2 L204-L210:

> The stream that carries the CONNECT request is used to exchange bidirectional data for the session. This stream will be referred to as a _CONNECT stream_. The stream ID of a CONNECT stream, which will be referred to as a _Session ID_, is used to uniquely identify a given WebTransport session within the connection.

Session ID は HTTP/2 Stream ID (RFC 9113 Section 5.1.1、unsigned 31-bit integer、`refs/rfc9113.txt` L904-L907) を `u64` 拡張して 64-bit big-endian で書き込む。draft Section 5.3 自身にはバイト順を明示していないが、RFC 9113 L273-L281 に「All numeric values are in network byte order」と定義されているため、Session ID も big-endian と解釈する。なお、WebTransport over HTTP/2 の capsule 表記も RFC 9000 Section 1.3 の慣用に基づくが、Exporter Context は HTTP/2 上のバイト列であり RFC 9113 の表記規則を第一の根拠とする。

TLS exporter の定義は RFC 8446 Section 7.5 にある。該当文面:

> The exporter value is computed as:
> ```
> TLS-Exporter(label, context_value, key_length) =
> HKDF-Expand-Label(Derive-Secret(Secret, label, ""),
>                   "exporter", Hash(context_value), key_length)
> ```
> Where Secret is either the early_exporter_master_secret or the exporter_master_secret. Implementations MUST use the exporter_master_secret unless explicitly specified by the application.
>
> If no context is provided, the context_value is zero-length. Consequently, providing no context computes the same value as providing an empty context.
>
> New uses of exporters SHOULD provide a context in all exporter computations, though the value could be empty.

`key_length` は HKDF-Expand-Label 内の `uint16 length` フィールド (RFC 8446 Section 7.1) なので API 上限は 65535 バイトまでだが、HKDF-Expand の反復回数上限 (RFC 5869 Section 2.3) により実用上の最大出力長は `255 * Hash.length` である。rustls もこれを超える要求を拒否する。

## 本 issue で扱う

- Sans I/O 層: `WebTransport Exporter Context` のシリアライズ関数 `serialize_exporter_context(session_id: u64, app_label: &[u8], app_context: &[u8]) -> Result<Vec<u8>, WtError>` を `src/webtransport/exporter.rs` (新規) に追加し、`src/webtransport/mod.rs` から `pub mod exporter;` (`pub mod error;` と `pub mod flow_control;` の間) と `pub use exporter::serialize_exporter_context;` (`pub use error::{...};` と `pub use flow_control::...;` の間) で公開する
- tokio-http2 層:
  - `DriverCmd::ExportKeyingMaterial` を `crates/tokio-http2/src/webtransport.rs` の `DriverCmd` enum (L638-L673) に追加
  - `DriverState::handle_cmd` (L726-L836) に `ExportKeyingMaterial` 処理を追加 (Sans I/O 層でシリアライズ → `ServerConnection::with_tls` 経由で `rustls::ServerConnection::export_keying_material` を呼ぶ)
  - `WtServerSession::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` を `WtServerSession` impl (L283-L371) に追加
  - `WtSessionHandle::export_keying_material` を同じシグネチャで `WtSessionHandle` impl (L406-L455) に追加 (driver 側で `connect_stream_id` から session_id を導出するため、handle 側で session_id を持つ必要はない)
  - `crates/tokio-http2/src/webtransport.rs` の既存 use 文 (L19-L22 付近) に `serialize_exporter_context` を追加

## 本 issue のスコープ外

- **クライアント側 export API**: 現状 `crates/tokio-http2/src/client.rs` に WebTransport クライアント API が無いためサーバー側のみ対応する
- **TLS 1.2 + EMS での export**: draft-ietf-webtrans-http2-14 Section 7 L1425-L1439 は TLS 1.3 または TLS 1.2 + EMS を要求するが、0063 で TLS 1.3 強制が確定しているため本 issue では TLS 1.3 のみを対象とする
- **`refs/rfc8446.txt` / `refs/rfc5869.txt` 新規収録**: これらの RFC 本文は `update-refs` 対象として別途扱う。本 issue では上記の通り必要な文面を引用済み
- **PBT / fuzzing**: `serialize_exporter_context` の入力空間は `app_label` / `app_context` が 255 バイト制限、`length` が 65535 制限、 `session_id` も big-endian 書き込みの境界値をテストすれば十分なため、本 issue では対象外とする

## 設計判断

### 1. シリアライズ関数は Sans I/O 層 (`src/webtransport/exporter.rs` 新規) に配置する

I/O 非依存の純粋なバイト列構築関数なので Sans I/O 層に置く。エラー型は既存 `WtError::invalid_input(reason)` (`src/webtransport/error.rs` L104-L106) で長さ制約違反を表現する。戻り値は `Vec<u8>` (所有データ) とし、本モジュール内の他関数と同様に `Vec::new()` から構築する。`pub` とするのは、Sans I/O 層の単体テストで直接検証できるようにするためであり、tokio-http2 層は `pub use` 経由で利用する。

### 2. `app_label` / `app_context` の長さ制約

Length フィールドが 8 bit (= u8) なので、コンテンツは最大 255 バイト。`app_label.len() > 255` または `app_context.len() > 255` のいずれかが成立したら `WtError::invalid_input` でエラーを返す。`app_context` の暗黙的切り詰めは行わない (鍵素材分岐の意味が壊れて対向と不一致になるため。issues/closed の `WT_CLOSE_SESSION reason` 切り詰めエラー化 (0061) と同じ方針)。

固定ラベル `"EXPORTER-WebTransport"` は draft-ietf-webtrans-http2-14 Section 5.3 L692-L693 で直接定められている。RFC 8446 Section 7.5 は exporter label の形式要件を RFC 5705 に委ねているが、本実装では draft がラベルを固定しているため RFC 5705 の詳細は間接参照に留まる。`app_label` と `app_context` は TLS exporter の `context_value` に入る WebTransport Exporter Context の一部であり、draft Section 5.3 が文字セット制約を定めていないため任意のバイト列を許容する。

### 3. `length == 0` の扱い

`rustls::ServerConnection::export_keying_material` は出力バッファが空の場合エラーを返す。本 API では `length == 0` を tokio-http2 層の `DriverState::handle_cmd` 内で `Error::InvalidArgument` で弾く。これにより rustls エラーへの依存を最小化する。

### 4. 巨大 `length` の扱い

HKDF-Expand-Label の `key_length` は RFC 8446 Section 7.1 で `uint16` なので、本 API では `length > 65535` を `Error::InvalidArgument` で弾く。これにより `vec![0u8; length]` における異常なメモリ確保を防ぎ、DoS 経路を排除する。ただし HKDF-Expand の実用上の上限は `255 * Hash.length` (RFC 5869 Section 2.3) なので、それを超える `length` に対しては rustls から `Error::Tls` が返ることを想定する。呼び出し元は `length` を適切に選ぶ責任を持つ。

### 5. Session ID は driver タスク内の `DriverState.connect_stream_id` から導出する

Sans I/O 層 `shiguredo_http2::webtransport::WtSession` には CONNECT ストリーム ID を保持するフィールドが無い。tokio-http2 層の `DriverState` (`webtransport.rs` L681-L697) が `connect_stream_id: StreamId` を保持しているため、`u64::from(self.connect_stream_id.as_u32())` で 64-bit 化する (`WtServerRequest::accept()` 内 L231 と同じ変換)。Sans I/O 層に HTTP/2 stream ID を持ち込まないことでアーキテクチャ責務を保つ。

### 6. `WtSessionHandle` 側の重複実装

`WtServerSession` と `WtSessionHandle` は `export_keying_material` メソッドの実装に `cmd_tx: mpsc::UnboundedSender<DriverCmd>` だけが必要であるため、両側で同一の実装になる。既存の `open_bidi` / `open_uni` / `send_datagram` / `drain` も同じ重複パターンを踏襲しており、本 issue もこの流儀に従う。

### 7. `accept()` フローへの追加なし

`export_keying_material` は実行時 API なので、`accept()` 内処理 (0063 設計判断 5 で確定した「TLS → Origin → 0064 → 0066 → :status=200」) には何も追加しない。本 issue は driver タスクと公開 API のみを変更する。

### 8. `ServerConnection::with_tls` は 0063 で導入済みのものを再利用する

`with_tls<F, R>(&self, f: F) -> R where F: FnOnce(&rustls::ServerConnection) -> R` は 0063 解決方法 1 (`server.rs` L178-L189) で `pub(crate)` として追加済み。本 issue で新規追加はしない。

### 9. `&self` / `&mut self` の方針

`export_keying_material` は `cmd_tx.send` だけを行うため `&self` で十分である。`WtServerSession` の既存メソッド (`open_bidi` 等) は `&mut self` だが、これらを `&self` に変更すると後方互換を破るため本 issue では既存メソッドを変更しない。今後新規メソッドを追加する際は `&self` で十分なら `&self` とし、段階的に API スタイルを統一する。`WtSessionHandle` は既存メソッドがすべて `&self` であるため、新メソッドも `&self` とする。

### 10. エラー変換は既存の `wt_err` パターンを踏襲し、0077 で統合する

`serialize_exporter_context` が返す `WtError::invalid_input` は、現行の `wt_err` 関数 (`webtransport.rs` L1053-L1055) により `Error::InvalidArgument` に変換する。open issue 0077 で `Error::WebTransport(WtError)` バリアントが導入され `wt_err` が除去される予定だが、0065 では既存パターンを踏襲し、0077 の対応時に一括で移行する。本 issue 実装後、`DriverState::handle_cmd` 内の `.map_err(wt_err)` は 1 箇所増えるため、0077 実装時の置換対象数に注意する。

### 11. rustls の export 失敗は `Error::Tls` にラップする

`rustls::Error` (`HandshakeNotComplete` 等) は I/O ではないため、既存の `Error::Tls(Box<dyn std::error::Error + Send + Sync>)` (`error.rs` L14) が意味論的に正確。TLS ハンドシェイク未完了等で `rustls::Error` が返った場合も `Error::Tls` にラップして返す。

### 12. 後方互換

本変更は `WtServerSession` / `WtSessionHandle` / Sans I/O 層への新規メソッド追加、非公開 `DriverCmd` への新規バリアント追加、新規モジュール公開のみであり、既存公開 API の後方互換を壊さない。

## 完了条件

- `src/webtransport/exporter.rs` (新規) に `pub fn serialize_exporter_context(session_id: u64, app_label: &[u8], app_context: &[u8]) -> Result<Vec<u8>, WtError>` が追加されていること
- `src/webtransport/mod.rs` に `pub mod exporter;` と `pub use exporter::serialize_exporter_context;` が追加され、`shiguredo_http2::webtransport::serialize_exporter_context` で参照可能なこと
- `serialize_exporter_context` が以下を満たすこと:
  - `session_id` を 8 バイト big-endian で書き込む
  - `app_label.len() > 255` または `app_context.len() > 255` で `WtError::invalid_input` を返す
  - 長さフィールド (label / context 各 1 バイト) を正しい値で書き込む
- `DriverCmd::ExportKeyingMaterial { app_label: Vec<u8>, app_context: Vec<u8>, length: usize, ack: oneshot::Sender<Result<Vec<u8>>> }` が追加されていること (`Result` は `crate::error::Result` = tokio-http2 層の `Error`、`WtResult` ではない)
- `DriverState::handle_cmd` が `ExportKeyingMaterial` を処理し、以下を満たすこと:
  - `length == 0` または `length > 65535` を `Error::InvalidArgument` で弾く
  - `serialize_exporter_context` を呼び、`ServerConnection::with_tls(|tls| tls.export_keying_material(...))` で TLS exporter を実行する
- `WtServerSession::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` が追加されていること
- `WtSessionHandle::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` が追加されていること
- 単体テストで以下が検証されていること:
  - `serialize_exporter_context` の正常系 (空 label / 空 context / 255 バイト境界 / 異なる `session_id` で異なるバイト列)
  - `serialize_exporter_context` の `app_label.len() == 256` で `WtError::invalid_input`
  - `serialize_exporter_context` の `app_context.len() == 256` で `WtError::invalid_input`
  - `session_id` の境界値 `0`、`1`、`0x7FFF_FFFF`、`0x8000_0000`、`0xFFFF_FFFF`、`u64::MAX` で正しく big-endian 8 バイトになること (これらは u64 型のビットパターンとしての境界値であり、`0x8000_0000` 以上は HTTP/2 Stream ID として無効だが、シリアライズ関数の挙動確認として検証する)
  - `tests/test_webtransport/main.rs` に `mod exporter;` が追加されていること。現状 `mod error;` は存在しないため、本 issue では `mod exporter;` のみ追加する。将来 0068 等で `mod error;` が追加される場合は `mod capsule;` → `mod error;` → `mod exporter;` → `mod flow_control;` の順序を維持する
- 統合テストで以下が検証されていること:
  - 既存 `test_tls()` helper をそのまま再利用すること (0063 の `accept()` 内で TLS 1.3 未満は拒否されるため、接続確立後に追加の `protocol_version()` 確認は不要)
  - TLS 1.3 接続上で `export_keying_material(b"label", b"ctx", 32)` が 32 バイト返すこと
  - 同一セッション・同一引数で 2 回呼んだとき同じ鍵素材が返ること (冪等性)
  - 同一セッションで `app_label` を変えると異なる鍵素材が返ること (例: `b"a"` と `b"b"`)
  - 同一セッションで `app_context` を変えると異なる鍵素材が返ること (例: `b"x"` と `b"y"`)
  - `length == 1` で 1 バイト返ること
  - `length` が HKDF-Expand 上限 (`255 * Hash.length`) を超える場合、`Error::Tls` が返ること (テストでは `length = 20000` 等、SHA-256 / SHA-384 いずれの場合も確実に超える値を使用する)
  - `length == 0` を `Error::InvalidArgument` で弾くこと
  - `length > 65535` を `Error::InvalidArgument` で弾くこと
  - `app_label` が 256 バイトで `Error::InvalidArgument` が返ること
  - `app_context` が 256 バイトで `Error::InvalidArgument` が返ること
  - 2 つの独立した接続で同じ `app_label` / `app_context` / `length` を与えても異なる鍵素材が返ること
  - `session.into_parts()` 後に `parts.handle.export_keying_material(...)` が成功すること
- CHANGES.md `## develop` に以下のような `[ADD]` エントリを追加すること:
  ```markdown
  - [ADD] `tokio-http2` に `WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material`、Sans I/O 層に `serialize_exporter_context` を追加する
    - @voluntas
  ```
- `examples/wt_server` に `export_keying_material` の使用例を追加すること。追加位置は `handle_connection` 内で `session.accept(...)` 成功後、`session.into_parts()` 呼び出しの直前とする。得た鍵素材はその長さのみをログ出力し、鍵素材そのものはネットワーク応答やログに出力しない:
  ```rust
  let key_material = session
      .export_keying_material(b"wt-server-example", b"", 32)
      .await?;
  log::debug!(
      "[{remote}] exported keying material: {} bytes",
      key_material.len()
  );
  // key_material は必要に応じて zeroize 等で安全に破棄する
  ```

## 解決方法

### 1. Sans I/O 層: `serialize_exporter_context`

`src/webtransport/exporter.rs` (新規):

```rust
//! WebTransport over HTTP/2 の TLS Keying Material Exporter 用 Exporter Context シリアライズ。
//!
//! draft-ietf-webtrans-http2-14 Section 5.3 で定義される `WebTransport Exporter Context`
//! をバイト列に変換する。

use crate::webtransport::error::WtError;

/// WebTransport Exporter Context (draft-ietf-webtrans-http2-14 §5.3 L696-L702) をシリアライズする。
///
/// `app_label` と `app_context` はいずれも空スライスを許容する。
///
/// # Errors
///
/// `app_label` または `app_context` の長さが 255 バイトを超える場合、`WtError::invalid_input` を返す。
pub fn serialize_exporter_context(
    session_id: u64,
    app_label: &[u8],
    app_context: &[u8],
) -> Result<Vec<u8>, WtError> {
    if app_label.len() > 255 {
        return Err(WtError::invalid_input("exporter label exceeds 255 bytes"));
    }
    if app_context.len() > 255 {
        return Err(WtError::invalid_input(
            "exporter context exceeds 255 bytes",
        ));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&session_id.to_be_bytes());
    out.push(app_label.len() as u8); // チェック済みのため as u8 は安全
    out.extend_from_slice(app_label);
    out.push(app_context.len() as u8);
    out.extend_from_slice(app_context);
    Ok(out)
}
```

`src/webtransport/mod.rs` に以下を追加:

```rust
pub mod exporter;
pub use exporter::serialize_exporter_context;
```

### 2. tokio-http2 層: DriverCmd と handle_cmd

`crates/tokio-http2/src/webtransport.rs` の `WEBTRANSPORT_PROTOCOL` 定数 (L29 付近) の近くに TLS exporter ラベル定数を追加:

```rust
const EXPORTER_LABEL: &[u8] = b"EXPORTER-WebTransport";
```

`DriverCmd` enum (L638-L673) の末尾、`Drain` variant の直後に追加:

```rust
ExportKeyingMaterial {
    app_label: Vec<u8>,
    app_context: Vec<u8>,
    length: usize,
    ack: oneshot::Sender<Result<Vec<u8>>>,
},
```

`crates/tokio-http2/src/webtransport.rs` の既存 use 文に `serialize_exporter_context` を追加:

```rust
use shiguredo_http2::webtransport::{
    serialize_exporter_context, WtConfig, WtEvent, WtInit, WtSession, WtSessionState, WtStreamId,
    stream::stream_id as wt_stream_id,
};
```

`DriverState::handle_cmd` (L726-L836) の `DriverCmd::Drain` arm の直後に追加:

```rust
DriverCmd::ExportKeyingMaterial { app_label, app_context, length, ack } => {
    let res = (|| -> Result<Vec<u8>> {
        if length == 0 || length > 65535 {
            return Err(Error::InvalidArgument(
                "export length must be in 1..=65535".into(),
            ));
        }
        let session_id = u64::from(self.connect_stream_id.as_u32());
        let ctx = serialize_exporter_context(session_id, &app_label, &app_context)
            .map_err(wt_err)?;
        let mut output = vec![0u8; length];
        let output = self
            .conn
            .with_tls(move |tls| {
                tls.export_keying_material(output, EXPORTER_LABEL, Some(ctx.as_slice()))
            })
            .map_err(|e| Error::Tls(Box::new(e)))?;
        Ok(output)
    })();
    // 本 arm は TLS exporter を呼ぶだけで wt_session の状態を変更しないため、
    // 他 arm のような flush_wt_output() は不要。
    let _ = ack.send(res);
}
```

### 3. 公開 API: WtServerSession / WtSessionHandle

`WtServerSession` impl (L283-L371) と `WtSessionHandle` impl (L406-L455) に同じ doc comment と実装を追加する:

```rust
impl WtServerSession {
    /// セッション固有の TLS Keying Material Exporter を導出する。
    ///
    /// 本メソッドは `&self` で呼び出せる (内部で `cmd_tx.send` のみ行う)。
    ///
    /// 返り値は暗号鍵素材に相当する機密情報であるため、必要に応じて呼び出し側で
    /// `zeroize` 等を使用して安全に破棄すること。
    ///
    /// `app_label` と `app_context` はいずれも空スライスを許容する。
    /// 空スライスを渡すと、WebTransport Exporter Context の該当部分がゼロ長となる。
    pub async fn export_keying_material(
        &self,
        app_label: &[u8],
        app_context: &[u8],
        length: usize,
    ) -> Result<Vec<u8>> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::ExportKeyingMaterial {
                app_label: app_label.to_vec(),
                app_context: app_context.to_vec(),
                length,
                ack,
            })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }
}
```

`WtSessionHandle` 側も `WtServerSession` と同一の doc comment・実装を追加する。

### 4. テスト戦略

- **Sans I/O 単体テスト** (`tests/test_webtransport/exporter.rs` 新規、AGENTS.md 規約に従いテストログは日本語):
  - 出力サイズ・先頭 8 バイト big-endian・長さフィールド一致
  - 空 label と空 context の境界 (例: `session_id = 1`、`app_label = b""`、`app_context = b""` の場合、`[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]` になること)
  - 空でない label と context が混在する場合のバイト列順序 (例: `app_label = b"label"`、`app_context = b"context"` で Session ID → label 長 → label → context 長 → context の順序になること)
  - 255 バイトちょうどの label / context が成功
  - label と context が両方とも 255 バイトのケースが成功
  - 256 バイトの label / context で `WtError::invalid_input`
  - 異なる `session_id` で異なるバイト列になること
  - `session_id` の境界値 `0`、`1`、`0x7FFF_FFFF`、`0x8000_0000`、`0xFFFF_FFFF`、`u64::MAX` で正しく big-endian 8 バイトになること
- **tokio-http2 統合テスト** (`crates/tokio-http2/tests/test_webtransport.rs`、既存 `test_tls()` helper をそのまま再利用):
  - TLS 1.3 接続上で `export_keying_material(b"label", b"ctx", 32)` が 32 バイト返すこと (0063 の `accept()` 内で TLS 1.3 未満は拒否されるため、接続確立後に追加のバージョン確認は不要)
  - 同じセッション・同じ引数で 2 回呼んで同一バイト列が返ること
  - 同じセッションで `app_label` を `b"a"` / `b"b"` と変えると異なる鍵素材が返ること
  - 同じセッションで `app_context` を `b"x"` / `b"y"` と変えると異なる鍵素材が返ること
  - `length == 0` で `Error::InvalidArgument` が返ること
  - `length > 65535` で `Error::InvalidArgument` が返ること
  - `length` が HKDF-Expand 上限 (`255 * Hash.length`) を超える場合、`Error::Tls` が返ること。テストでは `length = 20000` を使用する (SHA-256 の上限 `255 * 32 = 8160`、SHA-384 の上限 `255 * 48 = 12240` をいずれも超える値)
  - `app_label` / `app_context` が 256 バイトで `Error::InvalidArgument` が返ること
  - 2 つの独立した接続で同じ引数でも異なる鍵素材が返ること
  - `session.into_parts()` 後に `parts.handle.export_keying_material(...)` が成功すること。テスト終了時は `parts.driver.abort()` または `tokio::time::timeout(...).await` で driver タスクを片付ける

  注: 統合テストでは `tokio_http2::Client` が内部 TLS コネクションをカプセル化しており、クライアント側の `export_keying_material` を呼び出す公開 API がないため、サーバー側の出力長・冪等性・label / context 感度のみを検証する。`session_id` の context 組み込み検証は Sans I/O 単体テストで行う。

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 2 (WebTransport Sessions), L204-L210 — Session ID = CONNECT stream ID の定義
- draft-ietf-webtrans-http2-14 Section 5.3 (Use of Keying Material Exporters), L683-L708
- draft-ietf-webtrans-http2-14 Section 7 (Security Considerations), L1425-L1439 — TLS 1.3 または TLS 1.2 + EMS の要件
- RFC 9113 Section 5.1.1 (Stream Identifiers), L904-L907 — 31-bit Stream ID の根拠
- RFC 9113 L273-L276 — HTTP/2 フィールドのネットワークバイト順 (big-endian) 定義
- RFC 9113 L278-L281 — HTTP/2 が RFC 9000 Section 1.3 の表記規則を採用していることの定義
- RFC 8446 Section 7.5 (Exporters) — TLS 1.3 の TLS-Exporter 関数定義 (`refs/rfc8446.txt` は未収録のため実装時に原典を確認すること)
- RFC 8446 Section 7.1 — HKDF-Expand-Label の `uint16 length` フィールド (`refs/rfc8446.txt` は未収録のため実装時に原典を確認すること)
- RFC 5869 — HKDF-Expand の出力上限 (`255 * Hash.length`) (`refs/rfc5869.txt` は未収録のため実装時に原典を確認すること)
- RFC 9000 Section 1.3 — 構文表記の慣用

## 依存関係

- 実装済み前提: 0063 (TLS バージョン要件チェック) — `ServerConnection::with_tls` は 0063 で導入済み
- 実装済み前提: 0064 (WebTransport-Init ヘッダー) — `WtServerRequest::accept()` 内の処理順序は 0064 で確定済み
- 実装順序: 0063 → 0064 → 0065
- 関連: 0066 (`WtServerRequest::accept()` のシグネ変更と `test_webtransport.rs` の修正) — 現状の `accept()` は 3 引数 (`self`, `config`, `allowed_origin`) になっている。統合テストおよび `examples/wt_server` は現状のシグネチャを使用すること
- 関連: 0074 (draft-14 から最新版への参照更新) — 0074 マージ後、issue 本文およびソースコメント内の `draft-ietf-webtrans-http2-14 Section X.Y L###-L###` という行番号付き引用は古くなる。実装着手時には最新版該当節の行番号を確認し、ソースコメントも合わせて更新すること。可能であれば 0074 を先にマージしてから本 issue を実装する
- 関連: 0077 (`Error::WebTransport(WtError)` 導入) — 0077 マージ後は `wt_err` が削除され `Error::WebTransport(WtError)` に統合される。本 issue の `DriverState::handle_cmd` 内で追加する `.map_err(wt_err)` は 0077 実装時の置換対象となり、0065 マージ後は `wt_err` 呼び出しが 1 箇所増える。0077 が 0065 より先にマージされた場合、実装時点で既存パターンに合わせてエラー変換を行うこと
