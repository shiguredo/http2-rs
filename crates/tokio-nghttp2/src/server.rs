//! HTTP/2 サーバー

use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use shiguredo_nghttp2::{Header, Http2Event, SessionOptions, SettingsId, StreamId};

use crate::error::{Error, Result};

use crate::connection::Connection;
use crate::tls::TlsServerConfig;

/// TLS ストリーム型
type TlsStream = tokio_rustls::server::TlsStream<TcpStream>;

/// HTTP/2 サーバー
pub struct Server {
    listener: TcpListener,
    tls_acceptor: TlsAcceptor,
    local_addr: SocketAddr,
}

impl Server {
    /// サーバーを作成してバインド
    ///
    /// # Arguments
    ///
    /// * `addr` - バインドするアドレス
    /// * `tls_config` - TLS 設定
    pub async fn bind(addr: SocketAddr, tls_config: TlsServerConfig) -> Result<Self> {
        let listener = TcpListener::bind(addr).await.map_err(Error::Io)?;

        let local_addr = listener.local_addr().map_err(Error::Io)?;

        let tls_acceptor = TlsAcceptor::from(tls_config.inner());

        Ok(Self {
            listener,
            tls_acceptor,
            local_addr,
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
        let mut conn = Connection::server(tls_stream)?;

        // 初期 SETTINGS 送信
        // nghttp2 は recv() 時に connection preface を自動検証する
        conn.submit_settings(&[]).await?;

        Ok(ServerConnection {
            conn,
            local_addr: self.local_addr,
            remote_addr,
        })
    }

    /// 接続を受け入れ (SessionOptions 付き)
    pub async fn accept_with_options(&self, options: &SessionOptions) -> Result<ServerConnection> {
        // TCP 接続を受け入れ
        let (tcp_stream, remote_addr) = self.listener.accept().await.map_err(Error::Io)?;

        // TLS ハンドシェイク
        let tls_stream = self
            .tls_acceptor
            .accept(tcp_stream)
            .await
            .map_err(|e| Error::Tls(Box::new(e)))?;

        // HTTP/2 コネクション作成 (オプション付き)
        let mut conn = Connection::server_with_options(tls_stream, options)?;

        // 初期 SETTINGS 送信
        conn.submit_settings(&[]).await?;

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
        headers: &[Header],
        end_stream: bool,
    ) -> Result<()> {
        self.conn
            .submit_response(stream_id, headers, end_stream)
            .await
    }

    /// DATA を送信
    pub async fn send_data(
        &mut self,
        stream_id: StreamId,
        data: &[u8],
        end_stream: bool,
    ) -> Result<()> {
        self.conn.submit_data(stream_id, data, end_stream).await
    }

    /// トレーラー用の DATA を送信
    pub async fn send_data_for_trailer(&mut self, stream_id: StreamId, data: &[u8]) -> Result<()> {
        self.conn.submit_data_for_trailer(stream_id, data).await
    }

    /// トレーラーを送信
    pub async fn send_trailer(&mut self, stream_id: StreamId, headers: &[Header]) -> Result<()> {
        self.conn.submit_trailer(stream_id, headers).await
    }

    /// 追加の HEADERS フレームを送信 (情報レスポンス等)
    pub async fn send_headers(
        &mut self,
        stream_id: StreamId,
        headers: &[Header],
        end_stream: bool,
    ) -> Result<()> {
        self.conn
            .submit_headers(stream_id, headers, end_stream)
            .await
    }

    /// イベントを取得
    pub fn poll_event(&mut self) -> Option<Http2Event> {
        self.conn.poll_event()
    }

    /// イベントを待機
    pub async fn next_event(&mut self) -> Result<Http2Event> {
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

    /// RST_STREAM を送信
    pub async fn reset_stream(
        &mut self,
        stream_id: StreamId,
        error_code: shiguredo_nghttp2::ErrorCode,
    ) -> Result<()> {
        self.conn.submit_rst_stream(stream_id, error_code).await
    }

    /// GOAWAY を送信して接続を終了
    pub async fn shutdown(&mut self, last_stream_id: StreamId) -> Result<()> {
        self.conn
            .submit_goaway(last_stream_id, shiguredo_nghttp2::ErrorCode::NoError, &[])
            .await
    }

    /// セッションを終了する (GOAWAY 送信)
    pub async fn terminate(&mut self, error_code: shiguredo_nghttp2::ErrorCode) -> Result<()> {
        self.conn.terminate_session(error_code).await
    }

    /// graceful shutdown (GOAWAY with last_stream_id=2^31-1 を送信)
    pub async fn shutdown_graceful(&mut self) -> Result<()> {
        self.conn.submit_shutdown_notice().await
    }

    /// リモートの SETTINGS 値を取得
    pub fn get_remote_settings(&self, id: SettingsId) -> u32 {
        self.conn.get_remote_settings(id)
    }

    /// ローカルの SETTINGS 値を取得
    pub fn get_local_settings(&self, id: SettingsId) -> u32 {
        self.conn.get_local_settings(id)
    }

    /// 最後のエラーメッセージを取得
    pub fn last_error_message(&self) -> Option<&str> {
        self.conn.last_error_message()
    }
}
