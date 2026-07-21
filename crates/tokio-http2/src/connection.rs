//! HTTP/2 コネクション

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use shiguredo_http2::{
    Connection as Http2Connection, ErrorCode, Event, HeaderField, Limits, Role, Settings, StreamId,
};

use crate::error::{Error, Result};

/// HTTP/2 コネクション
///
/// Sans I/O の Connection をラップし、非同期 I/O を提供する。
pub struct Connection<S> {
    stream: S,
    inner: Http2Connection,
    recv_buf: Vec<u8>,
}

impl<S> Connection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// クライアントコネクションを作成
    pub fn client(stream: S, limits: Limits) -> Self {
        let inner = Http2Connection::client(limits);
        Self {
            stream,
            inner,
            recv_buf: vec![0u8; 16384],
        }
    }

    /// サーバーコネクションを作成
    pub fn server(stream: S, limits: Limits) -> Self {
        let inner = Http2Connection::server(limits);
        Self {
            stream,
            inner,
            recv_buf: vec![0u8; 16384],
        }
    }

    /// コネクションの役割を取得
    pub fn role(&self) -> Role {
        self.inner.role()
    }

    /// コネクションプリフェイスを送信（クライアント）
    pub async fn send_preface(&mut self) -> Result<()> {
        if self.inner.role() != Role::Client {
            return Err(Error::InvalidArgument(
                "send_preface is for client only".to_string(),
            ));
        }

        // HTTP/2 コネクションプリフェイスを送信
        self.stream
            .write_all(crate::CONNECTION_PREFACE)
            .await
            .map_err(Error::Io)?;

        // 内部の Connection にプリフェイス送信済みをマーク
        self.inner.mark_preface_sent();

        Ok(())
    }

    /// コネクションプリフェイスを受信（サーバー）
    pub async fn recv_preface(&mut self) -> Result<()> {
        if self.inner.role() != Role::Server {
            return Err(Error::InvalidArgument(
                "recv_preface is for server only".to_string(),
            ));
        }

        let mut preface = [0u8; 24];
        self.stream
            .read_exact(&mut preface)
            .await
            .map_err(Error::Io)?;

        if preface != crate::CONNECTION_PREFACE {
            return Err(Error::Protocol(shiguredo_http2::Error::connection_error(
                shiguredo_http2::ErrorCode::ProtocolError,
                "invalid connection preface",
            )));
        }

        // 内部の Connection にプリフェイス受信済みをマーク
        self.inner.mark_preface_received();

        Ok(())
    }

    /// 接続を開始する（SETTINGS 送信）
    pub async fn initiate(&mut self) -> Result<()> {
        self.inner.send_settings()?;
        self.flush().await
    }

    /// リクエストを送信（クライアント）
    pub async fn send_request(
        &mut self,
        headers: Vec<HeaderField>,
        end_stream: bool,
    ) -> Result<StreamId> {
        let stream_id = self.inner.start_stream(headers, end_stream)?;
        self.flush().await?;
        Ok(stream_id)
    }

    /// レスポンスを送信（サーバー）
    pub async fn send_response(
        &mut self,
        stream_id: StreamId,
        headers: Vec<HeaderField>,
        end_stream: bool,
    ) -> Result<()> {
        self.inner.send_response(stream_id, headers, end_stream)?;
        self.flush().await
    }

    /// データを送信
    pub async fn send_data(
        &mut self,
        stream_id: StreamId,
        data: Vec<u8>,
        end_stream: bool,
    ) -> Result<()> {
        self.inner.send_data(stream_id, data, end_stream)?;
        self.flush().await
    }

    /// ストリームをリセット
    pub async fn reset_stream(&mut self, stream_id: StreamId, error_code: ErrorCode) -> Result<()> {
        self.inner.reset_stream(stream_id, error_code)?;
        self.flush().await
    }

    /// PING を送信
    pub async fn send_ping(&mut self, opaque_data: [u8; 8]) -> Result<()> {
        self.inner.send_ping(opaque_data)?;
        self.flush().await
    }

    /// GOAWAY を送信
    pub async fn send_goaway(&mut self, error_code: ErrorCode, debug_data: Vec<u8>) -> Result<()> {
        self.inner.send_goaway(error_code, debug_data)?;
        self.flush().await
    }

    /// トレーラーを送信
    ///
    /// RFC 9113 Section 8.1: トレーラーは END_STREAM 付きの HEADERS フレームで送信する。
    /// 最終レスポンス送信後にのみ送信可能。
    pub async fn send_trailers(
        &mut self,
        stream_id: StreamId,
        headers: Vec<HeaderField>,
    ) -> Result<()> {
        self.inner.send_trailers(stream_id, headers)?;
        self.flush().await
    }

    /// WINDOW_UPDATE を送信
    pub async fn send_window_update(&mut self, stream_id: StreamId, increment: u32) -> Result<()> {
        self.inner.send_window_update(stream_id, increment)?;
        self.flush().await
    }

    /// イベントを取得
    pub fn poll_event(&mut self) -> Option<Event> {
        self.inner.poll_event()
    }

    /// 送信データをフラッシュ
    pub async fn flush(&mut self) -> Result<()> {
        let mut has_data = false;
        while let Some(data) = self.inner.poll_output() {
            self.stream.write_all(&data).await.map_err(Error::Io)?;
            has_data = true;
        }
        if has_data {
            self.stream.flush().await.map_err(Error::Io)?;
        }
        Ok(())
    }

    /// データを受信して処理
    pub async fn recv(&mut self) -> Result<usize> {
        let n = self
            .stream
            .read(&mut self.recv_buf)
            .await
            .map_err(Error::Io)?;

        if n == 0 {
            return Err(Error::ConnectionClosed);
        }

        self.inner.feed(&self.recv_buf[..n])?;
        self.inner.process()?;

        Ok(n)
    }

    /// イベントループを 1 回実行
    ///
    /// 送信データをフラッシュし、受信データを処理する。
    pub async fn drive(&mut self) -> Result<()> {
        // 送信
        self.flush().await?;

        // 受信
        self.recv().await?;

        Ok(())
    }

    /// イベントを待機
    ///
    /// イベントが発生するまでループし、最初のイベントを返す。
    pub async fn next_event(&mut self) -> Result<Event> {
        loop {
            // 送信データをフラッシュ（イベントを返却する前に必ずフラッシュ）
            self.flush().await?;

            // キューにあるイベントをチェック
            if let Some(event) = self.poll_event() {
                return Ok(event);
            }

            // 受信
            self.recv().await?;
        }
    }

    /// ローカル SETTINGS への参照を取得する
    #[must_use]
    pub fn local_settings(&self) -> &Settings {
        self.inner.local_settings()
    }

    /// リモート SETTINGS への参照を取得する
    #[must_use]
    pub fn remote_settings(&self) -> &Settings {
        self.inner.remote_settings()
    }

    /// 内部のストリームへの参照を取得
    pub fn get_ref(&self) -> &S {
        &self.stream
    }

    /// 内部のストリームへの可変参照を取得
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// コネクションを分解してストリームを取得
    pub fn into_inner(self) -> S {
        self.stream
    }
}
