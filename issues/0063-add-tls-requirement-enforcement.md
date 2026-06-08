# TLS バージョン要件チェックの追加

- Priority: High
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/add-tls-requirement-enforcement

## 目的

draft-ietf-webtrans-http2-14 Section 7 の MUST 要件に従い、TLS 1.3 未満で extended master secret (RFC 7627) なしの接続での WebTransport 利用を拒否する。

## 優先度根拠

仕様上の MUST 要件違反。TLS 1.2 で EMS なしの接続で WebTransport を使うと、TLS Keying Material Exporter の認証が安全でなくなる。セキュリティ上の問題。

## 現状

draft-ietf-webtrans-http2-14 Section 7 (L1425-L1439):

> Because TLS keying material exporters are only secure for authentication when they are uniquely bound to the TLS session [RFC7627], WebTransport requires either one of the following conditions:
> * The TLS version in use is greater than or equal to 1.3 [TLS].
> * The TLS version in use is 1.2, and the extended master secret extension [RFC7627] has been negotiated.
>
> Clients MUST NOT send WebTransport over HTTP/2 requests on connections that do not meet one of the two conditions above. If a server receives a WebTransport over HTTP/2 request on a connection that meets neither, the server MUST treat the request as malformed, as specified in Section 8.1.1 of [HTTP2].

現在の実装: tokio-http2 層の `tls.rs`, `server.rs`, `webtransport.rs` のいずれにも TLS バージョンまたは EMS のチェックコードは存在しない。

### 技術的制約: rustls 0.23 の EMS API

rustls 0.23 では、`ServerConnection` に対して EMS がネゴシエートされたかどうかを問い合わせる**公開 API が存在しない**。`extended_master_secret_ack` フィールドは `pub(crate)` であり、外部クレートからはアクセスできない。

また `ServerConfig.require_ems` のデフォルト値は `cfg!(feature = "fips")` であり、fips feature 非有効時は **デフォルトで `false`** である。

TLS バージョンの取得は `CommonState::protocol_version()` (`Option<ProtocolVersion>`) で可能。

## 設計判断

rustls 0.23 の制約を踏まえ、以下の方針を採る:

1. **TLS 1.2 + EMS チェックは `ServerConfig.require_ems = true` で代用する**: EMS 非対応クライアントは TLS ハンドシェイク自体が失敗するため、そもそも WebTransport リクエストが到達しない。これは仕様の「malformed 扱い」とは動作が異なるが、セキュリティ上の要件は満たす。
2. **TLS >= 1.3 のチェックは `protocol_version()` で行う**: これは公開 API で可能。
3. **チェックは tokio-http2 層の `WtServerRequest::accept()` 内で行う**: トランスポート層の関心事であり、Sans I/O 層の責務ではない。

## 完了条件

- TLS >= 1.3 の接続では WebTransport が許可されること
- TLS 1.2 + `require_ems = true` の設定で、EMS 対応クライアントは接続可能なこと
- TLS 1.2 + `require_ems = true` の設定で、EMS 非対応クライアントは TLS ハンドシェイクで拒否されること
- `protocol_version()` が `None`（ハンドシェイク未完了）の場合はエラー扱い
- TLS を使用していない平文接続では WebTransport が拒否されること
- 単体テストで検証されていること

### スコープ外

- TLS 1.2 の接続後に EMS ネゴシエート有無を動的に判定する仕組み（rustls の API 制約により実現不可能）
- `ServerConfig` の `require_ems` 設定の自動化（呼び出し側が `tls.rs` で明示的に設定する）

## 解決方法

### 1. TLS 設定 (`crates/tokio-http2/src/tls.rs`)

`ServerConfig` 構築時に `require_ems` を明示的に設定する。これは既存の TLS 設定の延長であり、WebTransport 専用ではないが、ドキュメントで推奨する。

```rust
config.require_ems = true;
```

### 2. ServerConnection に TLS バージョン問い合わせ API を追加

`ServerConnection` の内部に保持している `tokio_rustls::server::TlsStream<TcpStream>` から `get_ref().0.get_ref()` で `rustls::ServerConnection` に到達し、`protocol_version()` を呼び出すラッパーメソッドを追加する。

### 3. WtServerRequest::accept() 内でのチェック

```rust
pub async fn accept(
    self,
    config: WtConfig,
    allowed_origin: Option<&[u8]>,
) -> Result<WtServerSession> {
    // draft-ietf-webtrans-http2-14 Section 7 (L1425-L1439):
    // TLS >= 1.3 または TLS 1.2 + EMS が必須。
    let tls_version = self.conn.tls_protocol_version().ok_or_else(|| {
        Error::InvalidArgument(
            "WebTransport requires a completed TLS handshake".into(),
        )
    })?;
    if tls_version < rustls::ProtocolVersion::TLSv1_3 {
        return Err(Error::InvalidArgument(format!(
            "WebTransport requires TLS 1.3 or higher, got {tls_version:?}"
        )));
    }
    // TLS 1.2 + EMS は ServerConfig.require_ems = true で TLS レベルで保証済み

    // 既存の Origin チェック、200 レスポンス、WtSession 生成
    // ...
}
```

上記コードでは `tls_version < TLSv1_3` を一律拒否する。TLS 1.2 + EMS は `require_ems = true` により TLS ハンドシェイクレベルで保証されているため、`accept()` 到達時点で条件を満たしている。

### テスト戦略

#### 単体テスト (`crates/tokio-http2/tests/test_webtransport.rs`)

- TLS 1.3 接続 → `accept()` 成功
- `protocol_version()` が `None` → エラー
- テスト用に `rcgen` で自己署名証明書を生成し、`rustls::ServerConfig` で TLS 1.3 のみの設定と TLS 1.2 + EMS の設定を構築する

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 7 (Requirements on TLS Usage), L1423-L1439
- RFC 7627 (TLS Session Hash and Extended Master Secret Extension)
- RFC 8446 (TLS 1.3)
- RFC 9113 Section 8.1.1 (Malformed Requests and Responses)
