//! HTTP/2 サーバー

use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use shiguredo_http2::{ErrorCode, Event, HeaderField, Limits, StreamId};

use crate::connection::Connection;
use crate::error::{Error, Result};
use crate::tls::TlsServerConfig;

/// TLS ストリーム型
type TlsStream = tokio_rustls::server::TlsStream<TcpStream>;

/// HTTP/2 サーバー
pub struct Server {
    listener: TcpListener,
    tls_acceptor: TlsAcceptor,
    local_addr: SocketAddr,
    limits: Limits,
}

impl Server {
    /// サーバーを作成してバインド
    ///
    /// # Arguments
    ///
    /// * `addr` - バインドするアドレス
    /// * `tls_config` - TLS 設定
    /// * `limits` - HTTP/2 制限設定
    pub async fn bind(
        addr: SocketAddr,
        tls_config: TlsServerConfig,
        limits: Limits,
    ) -> Result<Self> {
        let listener = TcpListener::bind(addr).await.map_err(Error::Io)?;

        let local_addr = listener.local_addr().map_err(Error::Io)?;

        let tls_acceptor = TlsAcceptor::from(tls_config.inner());

        Ok(Self {
            listener,
            tls_acceptor,
            local_addr,
            limits,
        })
    }

    /// ローカルアドレスを取得
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// 接続を受け入れ
    pub async fn accept(&self) -> Result<ServerConnection> {
        // TCP 接続を受け入れ
        let (tcp_stream, remote_addr) = self.listener.accept().await.map_err(Error::Io)?;

        // TLS ハンドシェイク
        let tls_stream = self
            .tls_acceptor
            .accept(tcp_stream)
            .await
            .map_err(|e| Error::Tls(Box::new(e)))?;

        // HTTP/2 コネクション作成
        let mut conn = Connection::server(tls_stream, self.limits.clone());

        // コネクションプリフェイス受信
        conn.recv_preface().await?;

        // 初期 SETTINGS 送信
        conn.initiate().await?;

        Ok(ServerConnection {
            conn,
            local_addr: self.local_addr,
            remote_addr,
        })
    }
}

/// サーバー側の HTTP/2 コネクション
pub struct ServerConnection {
    conn: Connection<TlsStream>,
    local_addr: SocketAddr,
    remote_addr: SocketAddr,
}

impl ServerConnection {
    /// ローカルアドレスを取得
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// リモートアドレスを取得
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// レスポンスを送信
    pub async fn send_response(
        &mut self,
        stream_id: StreamId,
        headers: Vec<HeaderField>,
        end_stream: bool,
    ) -> Result<()> {
        self.conn
            .send_response(stream_id, headers, end_stream)
            .await
    }

    /// データを送信
    pub async fn send_data(
        &mut self,
        stream_id: StreamId,
        data: Vec<u8>,
        end_stream: bool,
    ) -> Result<()> {
        self.conn.send_data(stream_id, data, end_stream).await
    }

    /// イベントを取得
    pub fn poll_event(&mut self) -> Option<Event> {
        self.conn.poll_event()
    }

    /// イベントを待機
    pub async fn next_event(&mut self) -> Result<Event> {
        self.conn.next_event().await
    }

    /// 送信データをフラッシュ
    pub async fn flush(&mut self) -> Result<()> {
        self.conn.flush().await
    }

    /// データを受信
    pub async fn recv(&mut self) -> Result<usize> {
        self.conn.recv().await
    }

    /// イベントループを 1 回実行
    pub async fn drive(&mut self) -> Result<()> {
        self.conn.drive().await
    }

    /// トレーラーを送信
    ///
    /// RFC 9113 Section 8.1: トレーラーは END_STREAM 付きの HEADERS フレームで送信する。
    /// 最終レスポンス送信後にのみ使用する。
    pub async fn send_trailers(
        &mut self,
        stream_id: StreamId,
        headers: Vec<HeaderField>,
    ) -> Result<()> {
        self.conn.send_trailers(stream_id, headers).await
    }

    /// ストリームをリセット
    pub async fn reset_stream(&mut self, stream_id: StreamId, error_code: ErrorCode) -> Result<()> {
        self.conn.reset_stream(stream_id, error_code).await
    }

    /// WINDOW_UPDATE を送信
    pub async fn send_window_update(&mut self, stream_id: StreamId, increment: u32) -> Result<()> {
        self.conn.send_window_update(stream_id, increment).await
    }

    /// GOAWAY を送信して接続を終了
    pub async fn shutdown(&mut self) -> Result<()> {
        self.conn.send_goaway(ErrorCode::NoError, vec![]).await
    }

    /// 内部の `rustls::ServerConnection` を参照する閉包を実行する
    ///
    /// `Server::accept()` で TLS ハンドシェイクが完了している前提。
    /// WebTransport の TLS バージョン検査 (draft-ietf-webtrans-http2-15 Section 7) や
    /// TLS Keying Material Exporter といった TLS 直接アクセスが必要な処理から呼ぶ。
    pub(crate) fn with_tls<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&rustls::ServerConnection) -> R,
    {
        let (_io, tls_conn) = self.conn.get_ref().get_ref();
        f(tls_conn)
    }
}
