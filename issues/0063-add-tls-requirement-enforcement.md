# TLS バージョン要件チェックの追加

- Priority: High
- Created: 2026-06-08
- Polished: 2026-06-09
- Model: deepseek-v4-pro
- Branch: feature/add-tls-requirement-enforcement

## 目的

draft-ietf-webtrans-http2-14 Section 7 の MUST 要件に従い、WebTransport セッションを受け入れる前に TLS バージョンを検査し、要件を満たさない接続を `RST_STREAM(PROTOCOL_ERROR)` で拒否するサーバー側ロジックを追加する。

## 優先度根拠

仕様上の MUST 要件違反。TLS Keying Material Exporter (0065) の認証安全性の前提条件であり、未実装のまま 0065 を提供すると鍵素材の一意性が保証されない。セキュリティ上の問題。

## 現状

draft-ietf-webtrans-http2-14 Section 7 (L1425-L1438):

> Because TLS keying material exporters are only secure for authentication when they are uniquely bound to the TLS session [RFC7627], WebTransport requires either one of the following conditions:
>
> *  The TLS version in use is greater than or equal to 1.3 [TLS].
>
> *  The TLS version in use is 1.2, and the extended master secret extension [RFC7627] has been negotiated.
>
> Clients MUST NOT send WebTransport over HTTP/2 requests on connections that do not meet one of the two conditions above. If a server receives a WebTransport over HTTP/2 request on a connection that meets neither, the server MUST treat the request as malformed, as specified in Section 8.1.1 of [HTTP2].

RFC 9113 Section 8.1.1 (`refs/rfc9113.txt` L2463-L2466) が malformed の具体要件を定める:

> Malformed requests or responses that are detected MUST be treated as a stream error (Section 5.4.2) of type PROTOCOL_ERROR.

現在の実装: `crates/tokio-http2/src/` の `tls.rs` / `server.rs` / `webtransport.rs` のいずれにも TLS バージョンチェック・EMS チェック・PROTOCOL_ERROR 送信のロジックは存在しない (`grep -n "protocol_version\|require_ems"` でヒットなし)。

## 本 issue で扱う

- `WtServerRequest::accept()` 内で TLS プロトコルバージョンを取得し、TLS 1.3 未満なら `RST_STREAM(PROTOCOL_ERROR)` を送信して `Err` を返す
- `ServerConnection` に `rustls::ServerConnection` への内部アクセス API (`with_tls`) を追加する。0065 (TLS Keying Material Exporter) も同じ API を再利用する

## 本 issue のスコープ外

- **TLS 1.2 + EMS 動的判定**: rustls 0.23 では `extended_master_secret_ack` フィールドが `pub(crate)` であり、外部クレートから問い合わせる公開 API が無い。本実装では当面 **TLS 1.3 のみ許可** とし、TLS 1.2 はたとえ EMS をネゴシエートしていても WebTransport 用には受け入れない。仕様要件 (TLS 1.3 または TLS 1.2+EMS のいずれか) より厳しいが、安全側に寄せる。将来 rustls が EMS 問い合わせ API を公開したら対応を検討する
- **`TlsServerConfig` API の変更**: 本 issue では `TlsServerConfig::new` / `from_der` のシグネチャも内部挙動も変更しない (`TlsServerConfig` は WebTransport 専用ではなく HTTP/2 平常通信でも使われるため)。テストで TLS 1.2 限定サーバーが必要な場合は、テストコード内で直接 `rustls::ServerConfig` を構築する
- **クライアント側の MUST NOT 強制**: 仕様は「Clients MUST NOT send WebTransport over HTTP/2 requests on connections that do not meet」と規定するが、現状 `crates/tokio-http2/src/client.rs` には WebTransport CONNECT 送信 API が存在しないため対象外。クライアント側 WebTransport API 追加時に併せて対応する
- **平文接続**: `Server::bind` (`server.rs` L33-L50) は必ず `TlsAcceptor` を経由するため、平文経路は型レベルで存在しない。完了条件には含めない
- **接続レベル `GOAWAY(INADEQUATE_SECURITY)`** (RFC 9113 §9.2.1 で MAY): 同接続上で WebTransport を使わない通常 HTTP/2 リクエストが流れる可能性を考慮し、本 issue ではストリームレベル拒否に留める

## 設計判断

### 1. TLS 1.3 強制

スコープ外で述べた rustls 0.23 の制約により、`accept()` 到達時点で「この接続で EMS がネゴシエートされたか」を判定できない。仕様より厳しく TLS 1.3 のみ許可する方針とする (代替案として `ServerConfig::require_ems = true` の検討もあるが、`TlsServerConfig` の共有を破壊するため不採用)。

### 2. 拒否時の動作: stream error of type PROTOCOL_ERROR

RFC 9113 Section 8.1.1 の MUST に従い、`accept()` 内で TLS 要件未達を検出したら CONNECT ストリームに `RST_STREAM(PROTOCOL_ERROR)` を送ってから `Err` を返す。レスポンスステータスは返さない (HEADERS 送信前にストリームをリセットする)。

`Error` バリアントは既存の `Error::InvalidArgument(String)` (`error.rs` L18) を使う。Origin 拒否 (`webtransport.rs` L131-L139) と同じ扱いで、新規バリアント追加は本 issue のスコープ外。

### 3. ServerConnection に共通 TLS アクセス API を追加

`ServerConnection` (`server.rs` L87) は内部に `Connection<TlsStream>` を private 保持しており、`TlsStream` (= `tokio_rustls::server::TlsStream<TcpStream>`) に届くには `Connection<S>::get_ref(&self) -> &S` (`connection.rs` L240) を経由する。さらに `tokio_rustls::server::TlsStream::get_ref(&self) -> (&IO, &rustls::ServerConnection)` の **`.1`** で `&rustls::ServerConnection` に到達する。

0063 と 0065 で同じ到達経路を使うため、`ServerConnection` に閉包受け取り型のヘルパーを共通インフラとして追加する。`pub(crate)` 可視性で、同じ `crates/tokio-http2/src/` 配下の `webtransport.rs` から呼べる。実装例は「## 解決方法 1」を参照。

### 4. accept() シグネチャは変更しない

本 issue では body 内で TLS チェックを追加するのみで、`WtServerRequest::accept(self, config: WtConfig, allowed_origin: Option<&[u8]>)` の現行シグネチャは変更しない。0064 (WebTransport-Init) もシグネチャ不変、0066 (サブプロトコル) で初めて引数追加となる。

### 5. チェック順序

`accept()` 内の各種チェックは以下の順で行う。本 issue は TLS を最先頭に確定し、後続 issue が組み込むチェック (WebTransport-Init パース 0064 など) は Origin の後ろに挿入する想定:

1. **TLS バージョンチェック** (本 issue で追加。最先頭)
2. Origin 検証 (既存、0062)
3. (将来 0064 で追加: WebTransport-Init パース)
4. `:status=200` 送信と `WtSession` 生成 (既存)
5. (将来 0066 で追加: WT-Protocol 検証と WT-Protocol レスポンスヘッダー)

TLS チェックを最先頭に置くのは、Origin 不一致が 403 HEADERS 送信に対し TLS 要件未達は `RST_STREAM(PROTOCOL_ERROR)` でストリーム自体を破棄するため、安全性と効率の両面で TLS を先に判定する。

`self.origin()` は `&self` を借りるのみで、`with_tls` も `&self` のみ。両者は部分ムーブ前 (`let Self { mut conn, stream_id, .. } = self;` より前) に並べて呼べる。順序は「TLS チェック → `self.origin()` 取得 → 部分ムーブ → Origin 判定 → ...」。

## 完了条件

- `ServerConnection` に `with_tls<F, R>(&self, f: F) -> R where F: FnOnce(&rustls::ServerConnection) -> R` (pub(crate)) が追加されていること
- `WtServerRequest::accept()` が TLS バージョンを取得し、`Some(rustls::ProtocolVersion::TLSv1_3)` 以外 (`Some(TLSv1_2)`, `Some(Unknown(_))`, `None`, その他) は全て拒否扱いで CONNECT ストリームに `RST_STREAM(PROTOCOL_ERROR)` を送ってから `Err(Error::InvalidArgument(...))` を返すこと
- 比較は `matches!(version, Some(rustls::ProtocolVersion::TLSv1_3))` の完全一致パターンを使い、`PartialOrd` 実装に依存しないこと (`rustls::ProtocolVersion` は `#[non_exhaustive]` のため、`<` 比較は将来の追加バリアントで意味が変わりうる)
- TLS 1.3 接続では従来通り `accept()` が成功すること
- 単体テストで TLS 1.3 成功・TLS 1.2 拒否の両方が検証されていること
- CHANGES.md `## develop` に `[CHANGE]` エントリを追加し、WebTransport セッション受理時に TLS 1.3 を要求するようになった旨を記載すること (`accept()` の挙動が変わるユーザー観察可能変更のため `[CHANGE]`)

## 解決方法

### 1. ServerConnection への共通 API 追加

`crates/tokio-http2/src/server.rs`:

```rust
impl ServerConnection {
    pub(crate) fn with_tls<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&rustls::ServerConnection) -> R,
    {
        let (_io, tls_conn) = self.conn.get_ref().get_ref();
        f(tls_conn)
    }
}
```

`self.conn.get_ref()` で `&TlsStream` (= `&tokio_rustls::server::TlsStream<TcpStream>`) を得て、その `get_ref()` でタプル `(&TcpStream, &rustls::ServerConnection)` の **`.1`** を取り出す。

### 2. WtServerRequest::accept() への TLS チェック追加

`crates/tokio-http2/src/webtransport.rs` の `accept()` 冒頭、Origin 検証より前に挿入する。あわせてファイル先頭の `use` 文に `use shiguredo_http2::ErrorCode;` を追加する (現状の `use shiguredo_http2::{Event, HeaderField, StreamId};` には `ErrorCode` が含まれていない)。`rustls::ProtocolVersion` は完全修飾で参照するため追加 `use` は不要。

```rust
// draft-ietf-webtrans-http2-14 Section 7 (L1425-L1438): TLS 1.3 を要求する。
// TLS 1.2 + EMS は rustls 0.23 の API 制約により動的判定不可のため、
// 当面サポートせず安全側に倒す。
let tls_version = self.conn.with_tls(|tls| tls.protocol_version());
if !matches!(tls_version, Some(rustls::ProtocolVersion::TLSv1_3)) {
    // RFC 9113 Section 8.1.1 (L2463-L2466) / Section 5.4.2:
    // malformed request は stream error of type PROTOCOL_ERROR で扱う。
    // ServerConnection::reset_stream は内部で flush するため、ここで即座に
    // ネットワークへ RST_STREAM が送出される (driver タスク未起動の状況でも問題ない)。
    let stream_id = self.stream_id;
    self.conn
        .reset_stream(stream_id, ErrorCode::ProtocolError)
        .await?;
    return Err(Error::InvalidArgument(format!(
        "WebTransport requires TLS 1.3 (got {tls_version:?})"
    )));
}
```

`accept()` 到達時には `Server::accept` (`server.rs` L58-L83) で TLS ハンドシェイクが完了しているため `protocol_version()` は通常 `Some(_)` を返す。防御的に `None` も拒否扱いとする。

なお `reset_stream` の `?` が `Err(Error::Io(_))` を返す経路があるが、その場合は I/O エラーとして上位に伝搬され、後続の `Err(Error::InvalidArgument(...))` には到達しない。これは I/O 失敗時の動作として妥当 (ストリームを破棄できない以上、接続レベルの異常を優先する)。

### 3. テスト戦略

`crates/tokio-http2/tests/test_webtransport.rs` に追加 (テストログのメッセージは AGENTS.md 規約に従い日本語):

- **TLS 1.3 成功ケース**: 既存テストと同じ経路 (`rcgen` で自己署名証明書を作り、`TlsServerConfig::new` でサーバー設定、`TlsClientConfig::insecure` でクライアント設定) で `accept()` が成功することを確認する
- **TLS 1.2 拒否ケース**: テスト内で `rustls::ServerConfig` と `rustls::ClientConfig` を **テストコード内で直接構築** し、`with_protocol_versions(&[&rustls::version::TLS12])` 相当 (rustls 0.23 の具体 API 呼び出しはテスト実装時に確認する) で TLS 1.2 限定にして起動する。`TlsServerConfig` 公開 API には新 API を追加しない (スコープ外)。CONNECT 送信 → `RST_STREAM(PROTOCOL_ERROR)` 受信を確認する

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 7 (Requirements on TLS Usage), L1425-L1438
- RFC 9113 Section 8.1.1 (Malformed Messages), L2442-L2482 (特に L2463-L2466 の MUST)
- RFC 9113 Section 5.4.2 (Stream Error Handling)
- RFC 9113 Section 9.2.1 (TLS 1.2 Features) — INADEQUATE_SECURITY 接続エラー (MAY) を採用しない判断の根拠

## 依存関係

- 後続: 0065 (TLS Keying Material Exporter) が本 issue で追加する `ServerConnection::with_tls` を再利用する
- 実装順序: 0063 → 0065
