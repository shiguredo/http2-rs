# TLS Keying Material Exporter を追加する

- Priority: Medium
- Created: 2026-06-08
- Polished: 2026-06-09
- Model: deepseek-v4-pro
- Branch: feature/add-tls-keying-material-exporter

## 目的

draft-ietf-webtrans-http2-14 Section 5.3 (L683-L708) に定義されている TLS Keying Material Exporter を実装し、WebTransport セッションごとに `EXPORTER-WebTransport` ラベルとセッション固有の Exporter Context で独立した鍵素材を導出できるようにする。

## 優先度根拠

仕様上の SHALL 要件。アプリケーションが TLS exporter を要求した場合に必要で、セキュアなセッション固有の鍵素材が必要なプロトコル (例: WebTransport over QUIC からの移植プロトコル) で必須となる。多くの WebTransport ユースケースで毎回必要ではないため Priority は Medium。

## 現状

draft-ietf-webtrans-http2-14 Section 5.3 (L683-L708):

> WebTransport over HTTP/2 supports the use of TLS keying material exporters Section 7.5 of [TLS]. Since the underlying HTTP/2 connection could be shared by multiple WebTransport sessions, WebTransport defines a mechanism for deriving a TLS exporter that separates keying material for different sessions. If the application requests an exporter for a given WebTransport session with a specified label and context, the resulting exporter SHALL be a TLS exporter as defined in Section 7.5 of [TLS] with the label set to "EXPORTER-WebTransport" and the context set to the serialization of the "WebTransport Exporter Context" struct as defined below.

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

サイズ表記の解釈:

- `(64)` = 64 bit fixed (8 バイト)
- `(8)` = 8 bit fixed Length フィールド (u8 で表現、値域 0-255)
- `(8..)` / `(..)` = 可変長コンテンツ。直前の 8 bit Length フィールドで長さを表現するため最大 255 バイト
- ネットワークバイト順 (big-endian) でシリアライズする

Session ID は HTTP/2 Stream ID (RFC 9113 Section 5.1.1、unsigned 31-bit integer、`refs/rfc9113.txt` L902-L905) を `u64` 拡張して 64-bit big-endian で書き込む。

現在の実装: `grep -rn "export_keying_material\|EXPORTER-WebTransport" src/ crates/` でヒットなし、完全な未実装。

## 本 issue で扱う

- Sans I/O 層: `WebTransport Exporter Context` のシリアライズ関数 `serialize_exporter_context(session_id: u64, app_label: &[u8], app_context: &[u8]) -> Result<Vec<u8>, WtError>` を `src/webtransport/exporter.rs` (新規) に追加し、`src/webtransport/mod.rs` から `pub mod exporter;` で公開する
- tokio-http2 層:
  - `DriverCmd::ExportKeyingMaterial` を追加
  - `DriverState::handle_cmd` に `ExportKeyingMaterial` 処理を追加 (Sans I/O 層でシリアライズ → `ServerConnection::with_tls` 経由で `rustls::ServerConnection::export_keying_material` を呼ぶ)
  - `WtServerSession::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` を追加
  - `WtSessionHandle::export_keying_material` を同じシグネチャで追加 (driver 側で `connect_stream_id` から session_id を導出するため、handle 側で session_id を持つ必要はない)

## 本 issue のスコープ外

- **クライアント側 export API**: 現状 `crates/tokio-http2/src/client.rs` に WebTransport クライアント API が無いためサーバー側のみ対応する
- **`Error` バリアントの新規追加**: rustls の export 失敗は既存の `Error::Tls(Box<dyn ...>)` (`error.rs` L14) を再利用する
- **`refs/rfc8446.txt` 収録**: TLS 1.3 仕様本体への参照は本 issue では `Section 7.5 (Exporters)` の慣行を参照するだけで、実装は rustls の API に委ねる。`refs/` 収録は `update-refs` 対象として別途扱う
- **TLS 1.2 + EMS での export**: 0063 で TLS 1.3 強制が確定するため、TLS 1.2 経路は accept 時に弾かれる。本 issue で扱う必要はない

## 設計判断

### 1. シリアライズ関数は Sans I/O 層 (`src/webtransport/exporter.rs` 新規) に配置する

I/O 非依存の純粋なバイト列構築関数なので Sans I/O 層に置く。エラー型は既存 `WtError::invalid_input(reason)` (`error.rs` L102-L106) で長さ制約違反を表現する。戻り値は `Vec<u8>` (所有データ) で、`Bytes` 化はプロジェクト全体の no_std 化 (検討中) と合わせて将来再検討する。本 issue では `WtConfig` 等の既存 Sans I/O API が `Vec<u8>` を扱う流儀に合わせる。

### 2. `app_label` / `app_context` の長さ制約

Length フィールドが 8 bit (= u8) なので、コンテンツは最大 255 バイト。`app_label.len() > 255` または `app_context.len() > 255` のいずれかが成立したら `WtError::invalid_input` でエラーを返す。`app_context` の暗黙的切り詰めは行わない (鍵素材分岐の意味が壊れて対向と不一致になるため。issues/closed の `WT_CLOSE_SESSION reason` 切り詰めエラー化 (0061) と同じ方針)。

### 3. `length == 0` の扱い

`rustls::ServerConnection::export_keying_material` は出力バッファが空の場合エラーを返す (`rustls-0.23` `conn.rs` 該当行で "fails if `output.len()` is zero" と規定)。本 API では `length == 0` を Sans I/O 層に到達する前に `WtError::invalid_input` で弾く。これにより rustls エラーへの依存を最小化する。

### 4. 巨大 `length` の扱い

`rustls::ConnectionCommon::export_keying_material` は HKDF-Expand-Label で任意長 (実用上は数 KiB が上限) を生成できる。本 issue では DoS 対策の上限を設けず、rustls API の挙動に委ねる (アプリケーション層が `length` を選ぶ責任を持つ)。

### 5. Session ID は driver タスク内の `DriverState.connect_stream_id` から導出する

Sans I/O 層 `shiguredo_http2::webtransport::WtSession` には CONNECT ストリーム ID を保持するフィールドが無い。tokio-http2 層の `DriverState` (`webtransport.rs` L630-L645) が `connect_stream_id: StreamId` を保持しているため、`u64::from(self.connect_stream_id.as_u32())` で 64-bit 化する (`WtServerSession::session_id()` (`webtransport.rs` L180) と同じ変換)。Sans I/O 層に HTTP/2 stream ID を持ち込まないことでアーキテクチャ責務を保つ。

### 6. `WtSessionHandle` 側の重複実装

`WtServerSession` (L223-L230) と `WtSessionHandle` (L351-L353) は両者とも `cmd_tx: mpsc::UnboundedSender<DriverCmd>` のみ参照するため、`export_keying_material` の実装は両側で同一になる。既存の `open_bidi` / `open_uni` / `send_datagram` も同じ重複パターンを踏襲しており、本 issue もこの流儀に従う。

### 7. `accept()` フローへの追加なし

`export_keying_material` は実行時 API なので、`accept()` 内処理 (0063 設計判断 5 で確定した「TLS → Origin → 0064 → :status=200 → 0066」) には何も追加しない。本 issue は driver タスクと公開 API のみを変更する。

### 8. `ServerConnection::with_tls` は 0063 で導入済みのものを再利用する

`with_tls<F, R>(&self, f: F) -> R where F: FnOnce(&rustls::ServerConnection) -> R` は 0063 解決方法 1 (`server.rs`) で `pub(crate)` として追加される。本 issue で新規追加はしない。

## 完了条件

- `src/webtransport/exporter.rs` (新規) に `pub fn serialize_exporter_context(session_id: u64, app_label: &[u8], app_context: &[u8]) -> Result<Vec<u8>, WtError>` が追加されていること
- `src/webtransport/mod.rs` に `pub mod exporter;` と `pub use exporter::serialize_exporter_context;` が追加され、`shiguredo_http2::webtransport::serialize_exporter_context` で参照可能なこと
- `serialize_exporter_context` が以下を満たすこと:
  - `session_id` を 8 バイト big-endian で書き込む
  - `app_label.len() > 255` または `app_context.len() > 255` で `WtError::invalid_input` を返す
  - 長さフィールド (label / context 各 1 バイト) を正しい値で書き込む
  - 出力サイズが `8 + 1 + app_label.len() + 1 + app_context.len()` と一致する
- `DriverCmd::ExportKeyingMaterial { app_label: Vec<u8>, app_context: Vec<u8>, length: usize, ack: oneshot::Sender<Result<Vec<u8>>> }` が追加されていること (`Result` は `crate::error::Result` = tokio-http2 層の `Error`、`WtResult` ではない)
- `DriverState::handle_cmd` が `ExportKeyingMaterial` を処理し、`length == 0` を `Error::InvalidArgument` で弾いてから `serialize_exporter_context` を呼び、`ServerConnection::with_tls(|tls| tls.export_keying_material(...))` で TLS exporter を実行すること
- `WtServerSession::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` が追加されていること
- `WtSessionHandle::export_keying_material(&self, app_label: &[u8], app_context: &[u8], length: usize) -> Result<Vec<u8>>` が追加されていること
- 単体テストで以下が検証されていること:
  - `serialize_exporter_context` の正常系 (空 label / 空 context / 255 バイト境界)
  - `serialize_exporter_context` の `app_label.len() == 256` で `WtError::invalid_input`
  - `serialize_exporter_context` の `app_context.len() == 256` で `WtError::invalid_input`
  - 統合テストで `length == 0` を `Error::InvalidArgument` で弾くこと
  - 統合テストで TLS 1.3 接続上の `export_keying_material(b"label", b"ctx", 32)` が 32 バイト返すこと
  - 同一セッション・同一引数で **冪等** に同じ鍵素材が返ること
  - 同一セッションで `app_label` を変えると異なる鍵素材が返ること (例: `b"a"` と `b"b"`)
  - 同一セッションで `app_context` を変えると異なる鍵素材が返ること (例: `b"x"` と `b"y"`)
  - Sans I/O 単体テストで `serialize_exporter_context` に異なる `session_id` を渡すと異なるバイト列になること (`session_id` が context に組み込まれていることの検証)
- CHANGES.md `## develop` に `[ADD]` エントリを追加し、`WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material` / Sans I/O 層 `serialize_exporter_context` の追加を記載すること

## 解決方法

### 1. Sans I/O 層: `serialize_exporter_context`

`src/webtransport/exporter.rs` (新規):

```rust
use crate::webtransport::error::WtError;

/// WebTransport Exporter Context (draft-ietf-webtrans-http2-14 §5.3 L696-L702) をシリアライズする
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
    let mut out = Vec::with_capacity(8 + 1 + app_label.len() + 1 + app_context.len());
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

`crates/tokio-http2/src/webtransport.rs` の `DriverCmd` enum (L587-L622) に追加:

```rust
ExportKeyingMaterial {
    app_label: Vec<u8>,
    app_context: Vec<u8>,
    length: usize,
    ack: oneshot::Sender<Result<Vec<u8>>>,
},
```

`DriverState::handle_cmd` (L675-L785) に追加 (既存の `DriverCmd::Drain` の隣):

```rust
DriverCmd::ExportKeyingMaterial { app_label, app_context, length, ack } => {
    let res = (|| -> Result<Vec<u8>> {
        if length == 0 {
            return Err(Error::InvalidArgument(
                "export length must be greater than zero".into(),
            ));
        }
        let session_id = u64::from(self.connect_stream_id.as_u32());
        let ctx = serialize_exporter_context(session_id, &app_label, &app_context)
            .map_err(wt_err)?;
        let output = vec![0u8; length];
        let output = self
            .conn
            .with_tls(move |tls| {
                tls.export_keying_material(output, b"EXPORTER-WebTransport", Some(&ctx))
            })
            .map_err(|e| Error::Tls(Box::new(e)))?;
        Ok(output)
    })();
    let _ = ack.send(res);
}
```

`Error::Tls` を選ぶ理由: `rustls::Error` (`HandshakeNotComplete` 等) は I/O ではないため、既存の `Error::Tls(Box<dyn std::error::Error + Send + Sync>)` (`error.rs` L14) が意味論的に正確。

`rustls::ConnectionCommon::export_keying_material<T: AsMut<[u8]>>(&self, output: T, label: &[u8], context: Option<&[u8]>) -> Result<T, rustls::Error>` は `output` をムーブで受け取り、成功時に `output` を返す API。上記コードは `output: Vec<u8>` を `with_tls` のクロージャ (`move` キャプチャ) に渡し、`tls.export_keying_material(output, ...)` で `T = Vec<u8>` として呼び出す。`Result<Vec<u8>, rustls::Error>` が `with_tls` の戻り値として閉包外に返り、`?` で `Vec<u8>` を取り出して `Ok(output)` で返す。借用ではなく所有権移動で書くことで、`&mut Vec<u8>` のライフタイムを跨ぐ複雑さを避ける。

export 自体は HKDF-Expand-Label を 1 回回すだけでマイクロ秒オーダーのため、driver タスク内で同期実行しても tokio の他タスクをブロックしない (`spawn_blocking` 不要)。

### 3. 公開 API: WtServerSession / WtSessionHandle

`WtServerSession` (L223-L320) に追加:

```rust
impl WtServerSession {
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

`WtSessionHandle` (L351-L404) も同じシグネチャを `self.cmd_tx` 経由で実装する (内容は上記と完全に同じ。`&self` / `&mut self` の違いも無し)。

### 4. テスト戦略

- **Sans I/O 単体テスト** (`tests/test_webtransport/exporter.rs` 等、AGENTS.md 規約に従いテストログは日本語):
  - 出力サイズ・先頭 8 バイト big-endian・長さフィールド一致
  - 空 label と空 context の境界 (どちらも長さ 0 で書き込めるか)
  - 255 バイトちょうどの label / context が成功
  - 256 バイトの label / context で `WtError::invalid_input`
- **tokio-http2 統合テスト** (`crates/tokio-http2/tests/test_webtransport.rs`、既存 `test_tls()` / `server_limits()` helper を再利用):
  - TLS 1.3 接続上で `export_keying_material(b"label", b"ctx", 32)` が 32 バイト返すこと
  - 同じセッション・同じ引数で 2 回呼んで同一バイト列が返ること (冪等性)
  - 同じセッションで `app_label` を `b"a"` / `b"b"` と変えると異なる鍵素材が返ること
  - 同じセッションで `app_context` を `b"x"` / `b"y"` と変えると異なる鍵素材が返ること
  - `length == 0` で `Error::InvalidArgument` が返ること

  注: `WtServerRequest::accept()` (`webtransport.rs` L108) は `ServerConnection` をムーブして driver タスクに渡す設計のため、1 接続 = 1 WebTransport セッションとなる。同一 TCP/TLS 接続上で複数 session_id を立てるテストは現アーキテクチャでは作れないため、`session_id` の context 組み込み検証は Sans I/O 単体テスト (異なる session_id でバイト列が異なる) で行う。

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 5.3 (Use of Keying Material Exporters), L683-L708
- RFC 9113 Section 5.1.1 (Stream Identifiers), L902-L905 — 31-bit Stream ID の根拠
- RFC 8446 Section 7.5 (Exporters) — TLS 1.3 の TLS-Exporter 関数定義 (`refs/rfc8446.txt` 未収録、本 issue では rustls の API に委ねる)

## 依存関係

- 前提: 0063 (TLS バージョン要件チェック) — `ServerConnection::with_tls` は 0063 で導入される
- 実装順序: 0063 → 0065
