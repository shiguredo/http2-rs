//! HTTP/2 コネクション

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use shiguredo_nghttp2::{Header, Http2Event, Session, SessionRole, StreamId};

use crate::error::{Error, Result};

/// HTTP/2 コネクション
///
/// Sans I/O の Session をラップし、非同期 I/O を提供する。
pub struct Connection<S> {
    stream: S,
    session: Session,
    recv_buf: Vec<u8>,
}

impl<S> Connection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// クライアントコネクションを作成
    pub fn client(stream: S) -> Result<Self> {
        let session = Session::client()?;
        Ok(Self {
            stream,
            session,
            recv_buf: vec![0u8; 16384],
        })
    }

    /// サーバーコネクションを作成
    pub fn server(stream: S) -> Result<Self> {
        let session = Session::server()?;
        Ok(Self {
            stream,
            session,
            recv_buf: vec![0u8; 16384],
        })
    }

    /// セッションの役割を取得
    pub fn role(&self) -> SessionRole {
        self.session.role()
    }

    /// コネクションプリフェイスを送信（クライアント）
    pub async fn send_preface(&mut self) -> Result<()> {
        if self.session.role() != SessionRole::Client {
            return Err(Error::InvalidArgument(
                "send_preface is for client only".to_string(),
            ));
        }

        // HTTP/2 コネクションプリフェイスを送信
        self.stream
            .write_all(crate::CONNECTION_PREFACE)
            .await
            .map_err(Error::Io)?;

        Ok(())
    }

    /// コネクションプリフェイスを受信（サーバー）
    pub async fn recv_preface(&mut self) -> Result<()> {
        if self.session.role() != SessionRole::Server {
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
            return Err(Error::InvalidArgument(
                "invalid connection preface".to_string(),
            ));
        }

        Ok(())
    }

    /// SETTINGS フレームを送信
    pub async fn submit_settings(&mut self, settings: &[(u16, u32)]) -> Result<()> {
        self.session.submit_settings(settings)?;
        self.flush().await
    }

    /// リクエストを送信（クライアント）
    pub async fn submit_request(
        &mut self,
        headers: &[Header],
        data: Option<&[u8]>,
        end_stream: bool,
    ) -> Result<StreamId> {
        let stream_id = self.session.submit_request(headers, data, end_stream)?;
        self.flush().await?;
        Ok(stream_id)
    }

    /// レスポンスを送信（サーバー）
    pub async fn submit_response(
        &mut self,
        stream_id: StreamId,
        headers: &[Header],
        end_stream: bool,
    ) -> Result<()> {
        self.session
            .submit_response(stream_id, headers, end_stream)?;
        self.flush().await
    }

    /// RST_STREAM を送信
    pub async fn submit_rst_stream(
        &mut self,
        stream_id: StreamId,
        error_code: shiguredo_nghttp2::ErrorCode,
    ) -> Result<()> {
        self.session.submit_rst_stream(stream_id, error_code)?;
        self.flush().await
    }

    /// GOAWAY を送信
    pub async fn submit_goaway(
        &mut self,
        last_stream_id: StreamId,
        error_code: shiguredo_nghttp2::ErrorCode,
        debug_data: &[u8],
    ) -> Result<()> {
        self.session
            .submit_goaway(last_stream_id, error_code, debug_data)?;
        self.flush().await
    }

    /// PING を送信
    pub async fn submit_ping(&mut self, opaque_data: &[u8; 8]) -> Result<()> {
        self.session.submit_ping(opaque_data)?;
        self.flush().await
    }

    /// WINDOW_UPDATE を送信
    pub async fn submit_window_update(
        &mut self,
        stream_id: StreamId,
        increment: i32,
    ) -> Result<()> {
        self.session.submit_window_update(stream_id, increment)?;
        self.flush().await
    }

    /// DATA を送信
    pub async fn submit_data(
        &mut self,
        stream_id: StreamId,
        data: &[u8],
        end_stream: bool,
    ) -> Result<()> {
        self.session.submit_data(stream_id, data, end_stream)?;
        self.flush().await
    }

    /// イベントを取得
    pub fn poll_event(&mut self) -> Option<Http2Event> {
        self.session.poll_event()
    }

    /// 送信データがあるか
    pub fn want_write(&self) -> bool {
        self.session.want_write()
    }

    /// 受信待ちか
    pub fn want_read(&self) -> bool {
        self.session.want_read()
    }

    /// 送信データをフラッシュ
    pub async fn flush(&mut self) -> Result<()> {
        let data = self.session.send()?;
        if !data.is_empty() {
            self.stream.write_all(&data).await.map_err(Error::Io)?;
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

        Ok(self.session.recv(&self.recv_buf[..n])?)
    }

    /// データを受信して処理（タイムアウト付き）
    pub async fn recv_timeout(&mut self, timeout: std::time::Duration) -> Result<usize> {
        match tokio::time::timeout(timeout, self.recv()).await {
            Ok(result) => result,
            Err(_) => Err(Error::Timeout),
        }
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
    pub async fn next_event(&mut self) -> Result<Http2Event> {
        loop {
            // まずキューにあるイベントをチェック
            if let Some(event) = self.poll_event() {
                return Ok(event);
            }

            // 送信データをフラッシュ
            self.flush().await?;

            // 受信
            self.recv().await?;
        }
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
