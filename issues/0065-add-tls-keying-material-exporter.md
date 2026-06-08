# TLS Keying Material Exporter が未実装

- Priority: Medium
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/add-tls-keying-material-exporter

## 目的

draft-ietf-webtrans-http2-14 Section 5.3 に定義されている TLS Keying Material Exporter 機能を実装し、WebTransport セッションごとに独立した鍵素材を導出できるようにする。

## 優先度根拠

仕様上の SHALL 要件。アプリケーションが TLS exporter を要求した場合に必要。多くの WebTransport ユースケースでは必須ではないが、セキュアなセッション固有の鍵素材が必要なプロトコルを移植する際に必要。

## 現状

draft-ietf-webtrans-http2-14 Section 5.3 (L683-L708):

> If the application requests an exporter for a given WebTransport session with a specified label and context, the resulting exporter SHALL be a TLS exporter as defined in Section 7.5 of [TLS] with the label set to "EXPORTER-WebTransport" and the context set to the serialization of the "WebTransport Exporter Context" struct.

```
WebTransport Exporter Context {
  WebTransport Session ID (64),
  WebTransport Application-Supplied Exporter Label Length (8),
  WebTransport Application-Supplied Exporter Label (8..),
  WebTransport Application-Supplied Exporter Context Length (8),
  WebTransport Application-Supplied Exporter Context (..)
}
```

- Session ID: CONNECT ストリームの HTTP/2 Stream ID。現在 `WtServerSession::session_id()` で `u64` として取得可能。HTTP/2 Stream ID は最大 31 ビットだが、WebTransport Exporter Context では 64 ビット big-endian でシリアライズする。
- Label Length / Context Length: 8 ビット (u8)。アプリケーションが指定する label/context が 256 バイトを超える場合は、仕様上の扱いが定義されていないためエラーとする。

現在の実装: 全くの未実装。TLS exporter 呼び出しコードはコードベースに存在しない。

## アーキテクチャ上の制約

1. **`WtServerSession` は TLS 接続を持たない**: `WtServerSession` は mpsc/oneshot チャネル経由で driver タスクと通信するハンドル構造体。TLS 接続 (`rustls::ServerConnection`) は `WtServerRequest::accept()` 内で `DriverState.conn` に move される。
2. **`ServerConnection` は内部の `rustls::ServerConnection` を公開していない**: `ServerConnection.conn` は private。これを公開するか、アクセサメソッドを追加する必要がある。このインフラ変更は 0063 (TLS バージョンチェック) と共通のため、0063 との調整が必要。
3. **TLS export は同期呼び出し**: `rustls::ServerConnection::export_keying_material()` は同期。driver タスク内で `DriverCmd::ExportKeyingMaterial` を處理し、oneshot で結果を返す必要がある。

## 前提 issue

- 0063 (TLS バージョン要件チェック): `ServerConnection` → `rustls::ServerConnection` のアクセスパスを設計する。本 issue はそのパスを再利用する前提。実装順序: 0063 → 0065。

## 完了条件

- WebTransport セッションごとの鍵素材が導出できること
- `EXPORTER-WebTransport` ラベルと WebTransport Exporter Context 構造体が正しく使われること
- app_label / app_context が 256 バイト以上の場合はエラーになること
- `WtServerSession` と `WtSessionHandle` の両方から利用可能なこと
- 単体テストで検証されていること

## 解決方法

### Sans I/O 層

Exporter Context のシリアライズのみ担当。I/O 非依存の純粋なバイト列構築を行う。

```rust
// src/webtransport/mod.rs または新規モジュール
pub fn serialize_exporter_context(
    session_id: u64,
    app_label: &[u8],
    app_context: &[u8],
) -> Result<Vec<u8>, WtError> {
    if app_label.len() > 255 {
        return Err(WtError::invalid_input(
            "exporter label exceeds 255 bytes",
        ));
    }
    // app_context は可変長だが、文脈によっては制限を設けてもよい
    let mut ctx = Vec::new();
    ctx.extend_from_slice(&session_id.to_be_bytes());           // 64-bit
    ctx.push(app_label.len().try_into().expect("checked above"));
    ctx.extend_from_slice(app_label);
    // app_context が空の場合、Length = 0 で空バイト列
    // TLS API が context を省略可能でも、WebTransport 側では常に Context 全体を構築する
    ctx.push(u8::try_from(app_context.len()).unwrap_or(u8::MAX));
    ctx.extend_from_slice(app_context);
    Ok(ctx)
}
```

### tokio-http2 層

#### 1. ServerConnection に TLS アクセス用メソッド追加 (0063 と共通)

```rust
impl ServerConnection {
    pub(crate) fn with_tls<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&rustls::ServerConnection) -> R,
    {
        let tls_stream = self.conn.get_ref();
        // tokio_rustls::TlsStream::get_ref() で内部タプルにアクセス
        // (&TcpStream, &rustls::ServerConnection)
        let (_tcp, tls_conn) = tls_stream.get_ref();
        f(tls_conn)
    }
}
```

#### 2. DriverCmd 追加

```rust
enum DriverCmd {
    // ... 既存
    ExportKeyingMaterial {
        app_label: Vec<u8>,
        app_context: Vec<u8>,
        length: usize,
        ack: oneshot::Sender<Result<Vec<u8>>>,
    },
}
```

#### 3. DriverState::handle_cmd 内で処理

```rust
DriverCmd::ExportKeyingMaterial { app_label, app_context, length, ack } => {
    let exp_ctx = serialize_exporter_context(
        self.wt_session.session_id(), // 要: WtSession に session_id 追加 or DriverState に保持
        &app_label,
        &app_context,
    ).map_err(wt_err);
    let res = match exp_ctx {
        Ok(ctx) => {
            let mut output = vec![0u8; length];
            self.conn.with_tls(|tls| {
                tls.export_keying_material(&mut output, b"EXPORTER-WebTransport", Some(&ctx))
            }).map(|_| output).map_err(|e| Error::Io(
                std::io::Error::other(format!("TLS export failed: {e}"))
            ))
        }
        Err(e) => Err(wt_err(e)),
    };
    let _ = ack.send(res);
}
```

#### 4. 公開 API

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

impl WtSessionHandle {
    pub async fn export_keying_material(
        &self,
        app_label: &[u8],
        app_context: &[u8],
        length: usize,
    ) -> Result<Vec<u8>> {
        // WtServerSession と同様
    }
}
```

### テスト戦略

単体テスト (`crates/tokio-http2/tests/test_webtransport.rs`):
- `rcgen` で自己署名証明書を生成し TLS 接続を確立
- `export_keying_material(label, context, 32)` を呼び出し、32 バイトの出力が得られること
- 異なるセッション ID で異なる鍵素材が得られること
- 同一セッション・同一引数で同一の鍵素材が得られること（冪等性）
- app_label が 256 バイト以上でエラーになること

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 5.3 (Use of Keying Material Exporters), L683-L708
- RFC 8446 Section 7.5 (TLS 1.3 — Keying Material Exporters)
