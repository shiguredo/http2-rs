//! HTTP/2 コネクション

use std::io;

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
    // 下位ストリームの flush が完了するまで、取り出した出力と書き込み位置を保持する。
    pending_output: Option<(Vec<u8>, usize)>,
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
            pending_output: None,
        }
    }

    /// サーバーコネクションを作成
    pub fn server(stream: S, limits: Limits) -> Self {
        let inner = Http2Connection::server(limits);
        Self {
            stream,
            inner,
            recv_buf: vec![0u8; 16384],
            pending_output: None,
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
    ///
    /// 1 回の呼び出しで渡せる `data` は送信バッファの固定容量 (65535 bytes) までであり、
    /// それ以上は全量拒否されるため、呼び出し側で分割して渡すこと。送信待ちデータが
    /// 滞留している場合は、これより小さくても累積で容量を超えると拒否される。これは
    /// RFC 9113 Section 6.9 のフロー制御とは独立したローカルなバッファ上限である。
    pub async fn send_data(
        &mut self,
        stream_id: StreamId,
        data: Vec<u8>,
        end_stream: bool,
    ) -> Result<()> {
        self.inner.send_data(stream_id, data, end_stream)?;
        self.flush().await
    }

    /// 指定ストリームに HTTP/2 フロー制御で保留中の DATA または END_STREAM があるかを返す
    ///
    /// `send_data` は送信ウィンドウ枯渇時にデータをバッファへ積むだけで `Ok(())` を
    /// 返すことがある。その呼び出しが成功した後、本メソッドでフロー制御の待機を判定できる。
    /// I/O のキャンセル後は、先に [`Self::flush`] で下位ストリームへの送信を再開する。
    #[must_use]
    pub fn has_pending_send_data(&self, stream_id: StreamId) -> bool {
        self.inner.has_pending_send_data(stream_id)
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
    ///
    /// キャンセル時は未送信の出力と下位ストリームのフラッシュ待ちを保持し、
    /// 次回の [`Self::flush`] または [`Self::next_event`] で送信を継続する。
    pub async fn flush(&mut self) -> Result<()> {
        loop {
            if self.pending_output.is_none() {
                self.pending_output = self.inner.poll_output().map(|data| (data, 0));
            }
            let Some((data, offset)) = self.pending_output.as_mut() else {
                return Ok(());
            };

            // write が完了するたびに位置を進め、キャンセル時も新しい出力より先に続きを書く。
            while *offset < data.len() {
                match self.stream.write(&data[*offset..]).await {
                    Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero).into()),
                    Ok(written) => *offset += written,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error.into()),
                }
            }
            self.stream.flush().await?;
            self.pending_output = None;
        }
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
    /// キャンセル時は未送信の出力と取得前のイベントを保持する。
    /// 次回の [`Self::flush`] または [`Self::next_event`] で送信を継続する。
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

    /// ACK 済み自広告 SETTINGS への参照を取得する
    ///
    /// 公開面は `ServerConnection::acked_settings` から利用する
    /// (draft-ietf-webtrans-http2-16 Section 4.3.1: WebTransport セッションの
    /// 初期値決定に利用する)。
    #[must_use]
    pub(crate) fn acked_settings(&self) -> &Settings {
        self.inner.acked_settings()
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
    ///
    /// フレーム化済みの出力を送信する場合は、分解前に [`Self::flush`] を完了させる。
    pub fn into_inner(self) -> S {
        self.stream
    }
}
