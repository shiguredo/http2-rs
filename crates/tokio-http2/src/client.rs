//! HTTP/2 クライアント

use std::net::SocketAddr;

use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use shiguredo_http2::{ErrorCode, Event, HeaderField, Limits, StreamId};

use crate::connection::Connection;
use crate::error::{Error, Result};
use crate::tls::TlsClientConfig;

/// TLS ストリーム型
type TlsStream = tokio_rustls::client::TlsStream<TcpStream>;

/// HTTP/2 クライアント
pub struct Client {
    conn: Connection<TlsStream>,
    local_addr: SocketAddr,
    remote_addr: SocketAddr,
}

impl Client {
    /// サーバーに接続
    ///
    /// # Arguments
    ///
    /// * `addr` - 接続先アドレス
    /// * `server_name` - サーバー名 (SNI)
    /// * `tls_config` - TLS 設定
    /// * `limits` - HTTP/2 制限設定
    pub async fn connect(
        addr: SocketAddr,
        server_name: &str,
        tls_config: TlsClientConfig,
        limits: Limits,
    ) -> Result<Self> {
        // TCP 接続
        let tcp_stream = TcpStream::connect(addr).await.map_err(Error::Io)?;

        let local_addr = tcp_stream.local_addr().map_err(Error::Io)?;

        // TLS ハンドシェイク
        let connector = TlsConnector::from(tls_config.inner());
        let server_name =
            ServerName::try_from(server_name.to_string()).map_err(|e| Error::Tls(Box::new(e)))?;

        let tls_stream = connector
            .connect(server_name, tcp_stream)
            .await
            .map_err(|e| Error::Tls(Box::new(e)))?;

        // HTTP/2 コネクション作成
        let mut conn = Connection::client(tls_stream, limits);

        // コネクションプリフェイス送信
        conn.send_preface().await?;

        // 初期 SETTINGS 送信
        conn.initiate().await?;

        Ok(Self {
            conn,
            local_addr,
            remote_addr: addr,
        })
    }

    /// サーバーに接続（証明書検証なし、テスト用）
    pub async fn connect_insecure(
        addr: SocketAddr,
        server_name: &str,
        limits: Limits,
    ) -> Result<Self> {
        let tls_config = TlsClientConfig::insecure()?;
        Self::connect(addr, server_name, tls_config, limits).await
    }

    /// ローカルアドレスを取得
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// リモートアドレスを取得
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// リクエストを送信
    pub async fn send_request(
        &mut self,
        headers: Vec<HeaderField>,
        end_stream: bool,
    ) -> Result<StreamId> {
        self.conn.send_request(headers, end_stream).await
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

    /// PING を送信
    pub async fn ping(&mut self, data: [u8; 8]) -> Result<()> {
        self.conn.send_ping(data).await
    }

    /// GOAWAY を送信して接続を終了
    pub async fn shutdown(&mut self) -> Result<()> {
        self.conn.send_goaway(ErrorCode::NoError, vec![]).await
    }
}
