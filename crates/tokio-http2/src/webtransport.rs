//! WebTransport over HTTP/2 サーバー API
//!
//! draft-ietf-webtrans-http2-15 に基づく WebTransport サーバー実装。
//! Extended CONNECT (`:protocol=webtransport`) で確立されたセッション上で、
//! Capsule Protocol によって bidi / uni ストリームと DATAGRAM を多重化する。
//!
//! # アーキテクチャ
//!
//! `WtServerRequest::accept()` 時に単一の driver タスクを spawn し、
//! `WtServerSession` / `WtBidiStream` / `WtUniRecvStream` / `WtUniSendStream` は
//! mpsc/oneshot で driver と通信する。driver は HTTP/2 コネクションと
//! `shiguredo_http2::webtransport::WtSession` の橋渡しを担う。

use std::collections::{HashMap, HashSet};

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use shiguredo_http2::webtransport::{
    WtAvailableProtocols, WtConfig, WtErrorKind, WtEvent, WtInit, WtSession, WtSessionState,
    WtStreamId, serialize_exporter_context, serialize_wt_protocol,
    stream::stream_id as wt_stream_id,
};
use shiguredo_http2::{ErrorCode, Event, HeaderField, Role, StreamId};

use crate::error::{Error, Result};
use crate::server::ServerConnection;

/// WebTransport CONNECT 擬似ヘッダーの値
pub const WEBTRANSPORT_PROTOCOL: &[u8] = b"webtransport";

/// WebTransport セッション要求 (サーバー側)
///
/// `ServerConnection::next_event()` で Extended CONNECT (`:method=CONNECT` かつ
/// `:protocol=webtransport`) を受信した後、`from_connection` に接続を渡して
/// 作成する。その後 `accept()` か `reject()` を呼ぶ。
pub struct WtServerRequest {
    conn: ServerConnection,
    stream_id: StreamId,
    headers: Vec<HeaderField>,
}

impl WtServerRequest {
    /// 既に Extended CONNECT HEADERS を受け取った接続から要求を作成する
    #[must_use]
    pub fn from_connection(
        conn: ServerConnection,
        stream_id: StreamId,
        headers: Vec<HeaderField>,
    ) -> Self {
        Self {
            conn,
            stream_id,
            headers,
        }
    }

    /// CONNECT ストリーム ID を取得する
    #[must_use]
    pub const fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    /// リクエストヘッダーを取得する
    #[must_use]
    pub fn headers(&self) -> &[HeaderField] {
        &self.headers
    }

    /// `:path` 擬似ヘッダーを取得する
    #[must_use]
    pub fn path(&self) -> Option<&[u8]> {
        self.header(b":path")
    }

    /// `:authority` 擬似ヘッダーを取得する
    #[must_use]
    pub fn authority(&self) -> Option<&[u8]> {
        self.header(b":authority")
    }

    /// `:scheme` 擬似ヘッダーを取得する
    #[must_use]
    pub fn scheme(&self) -> Option<&[u8]> {
        self.header(b":scheme")
    }

    /// `origin` ヘッダーを取得する
    #[must_use]
    pub fn origin(&self) -> Option<&[u8]> {
        self.header(b"origin")
    }

    /// `WebTransport-Init` ヘッダー値 (RFC 8941 Dictionary バイト列) を取得する
    ///
    /// draft-ietf-webtrans-http2-15 Section 4.3.2 (L583-L590) で規定される
    /// 初期フロー制御値のヘッダー。HTTP/2 では field name は小文字なので
    /// `webtransport-init` (lowercase) で照合する。
    ///
    /// 既知の制限: 同名ヘッダーが複数あった場合は最初の 1 個のみを返し、
    /// RFC 8941 §4.2 (L1042-L1046) が MUST 要求する comma-concat 結合は未対応。
    /// 通常のクライアント実装が複数行を送ることは稀だが、必要に応じて将来対応する。
    #[must_use]
    pub fn webtransport_init(&self) -> Option<&[u8]> {
        self.header(b"webtransport-init")
    }

    /// `WT-Available-Protocols` ヘッダー値を取得する
    ///
    /// draft-ietf-webtrans-http2-15 Section 3.3 / draft-ietf-webtrans-http3
    /// Application Protocol Negotiation。HTTP/2 では field name は小文字なので
    /// `wt-available-protocols` で照合する。
    ///
    /// 既知の制限: 同名ヘッダーが複数あった場合は最初の 1 個のみを返す。
    #[must_use]
    pub fn wt_available_protocols(&self) -> Option<&[u8]> {
        self.header(b"wt-available-protocols")
    }

    fn header(&self, name: &[u8]) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|h| h.name() == name)
            .map(shiguredo_http2::HeaderField::value)
    }

    /// セッションを受け入れる
    ///
    /// `:status=200` レスポンスを送信し、`WtSession` を生成して
    /// 双方向エコーやストリーム受信が可能な状態にする。
    ///
    /// `allowed_origin` に `Some` を指定すると、リクエストに Origin ヘッダーが
    /// ある場合に検証する (draft-ietf-webtrans-http2-15 Section 3.2 MUST)。
    /// Origin が一致しない場合は 403 を返す。Origin が欠落している場合は
    /// 検証をスキップする。`None` を指定すると検証をスキップする
    /// (非 Web context 向け)。
    ///
    /// `selected_protocol` に `Some` を指定すると、リクエストの
    /// `WT-Available-Protocols` に含まれる値であることを検証し、
    /// レスポンスに `wt-protocol` (RFC 8941 sf-string) を付与する
    /// (draft-ietf-webtrans-http2-15 Section 3.3)。
    pub async fn accept(
        mut self,
        mut config: WtConfig,
        allowed_origin: Option<&[u8]>,
        selected_protocol: Option<&[u8]>,
    ) -> Result<WtServerSession> {
        // draft-ietf-webtrans-http2-15 Section 7 (L1483-L1487):
        // WebTransport over HTTP/2 は TLS 1.3 か、TLS 1.2 + extended master secret を要求する。
        // rustls 0.23 は extended master secret のネゴシエーション状態を外部公開していないため、
        // 動的判定不可。安全側に倒して TLS 1.3 のみを許可する (仕様より厳しい)。
        // 将来 `TLSv1_4` 等の新バリアントが追加された場合は本箇所の見直しが必要。
        // `ProtocolVersion` は `#[non_exhaustive]` のため `matches!` で完全一致比較する。
        let tls_version = self.conn.with_tls(|tls| tls.protocol_version());
        if !matches!(tls_version, Some(rustls::ProtocolVersion::TLSv1_3)) {
            // RFC 9113 Section 8.1.1 (L2463-L2465) / Section 5.4.2:
            // malformed request は stream error of type PROTOCOL_ERROR で処理する。
            self.conn
                .reset_stream(self.stream_id, ErrorCode::ProtocolError)
                .await?;
            return Err(Error::InvalidArgument(format!(
                "WebTransport requires TLS 1.3 (got {})",
                describe_tls_version(tls_version),
            )));
        }

        // draft-ietf-webtrans-http2-15 Section 3.2:
        // `:scheme` は `https` でなければならない (MUST)。case-insensitive。
        // 違反は stream error of type PROTOCOL_ERROR。
        let scheme_ok = self
            .scheme()
            .is_some_and(|s| s.eq_ignore_ascii_case(b"https"));
        if !scheme_ok {
            self.conn
                .reset_stream(self.stream_id, ErrorCode::ProtocolError)
                .await?;
            return Err(Error::InvalidArgument(
                "WebTransport requires :scheme=https".into(),
            ));
        }

        // self の部分ムーブ前に &self 借用が必要な値を取得する。
        let origin = self.origin().map(|o| o.to_vec());
        let init_bytes = self.webtransport_init().map(|v| v.to_vec());
        let available_bytes = self.wt_available_protocols().map(|v| v.to_vec());

        let Self {
            mut conn,
            stream_id,
            ..
        } = self;

        // draft-ietf-webtrans-http2-15 Section 3.2:
        // Origin ヘッダーがある場合に MUST verify。失敗は SHOULD 403。
        // 欠落時の必須検証は書かれていない (draft-14 の無条件 MUST verify から変更)。
        if let Some(allowed) = allowed_origin {
            // Origin ヘッダーがある場合のみ照合。欠落時は検証スキップ (accept 継続可)。
            if let Some(actual) = origin {
                // RFC 6454 Section 7: Origin = scheme "://" host [ ":" port ]
                // ASCII case-insensitive で比較する
                if !actual.eq_ignore_ascii_case(allowed) {
                    let response = vec![HeaderField::from_static(b":status", b"403")];
                    conn.send_response(stream_id, response, true).await?;
                    return Err(Error::InvalidArgument(format!(
                        "origin rejected: allowed={}, actual={}",
                        String::from_utf8_lossy(allowed),
                        String::from_utf8_lossy(&actual),
                    )));
                }
            }
        }

        // draft-ietf-webtrans-http2-15 Section 4.3.1:
        // セッション確立時は ACK 済みの自広告 SETTINGS 初期値を適用する。
        config.overlay_settings(conn.local_settings());

        // draft-ietf-webtrans-http2-15 Section 4.3.1:
        // ピア (クライアント) の SETTINGS からピア用 WtConfig を構築する。
        // send_max はピアの広告値を使う (Section 4.3.1)
        let mut peer_config = WtConfig::default();
        peer_config.overlay_settings(conn.remote_settings());

        // draft-ietf-webtrans-http2-15 Section 4.3 (L509-L530) / Section 4.3.2 (L583-L590):
        // WebTransport-Init はクライアントが送信するヘッダーであり、クライアントの広告値を含む。
        // ピア用 config では bl → initial_max_stream_data_bidi_local、
        // br → initial_max_stream_data_bidi_remote と対応するため apply_init_as_peer を使う。
        // パース失敗・型不一致・値範囲外は MUST 4xx 拒否。
        if let Some(bytes) = init_bytes {
            match WtInit::parse(&bytes) {
                Ok(init) => peer_config.apply_init_as_peer(&init),
                Err(e) => {
                    let response = vec![HeaderField::from_static(b":status", b"400")];
                    conn.send_response(stream_id, response, true).await?;
                    return Err(Error::from(e));
                }
            }
        }

        // draft-ietf-webtrans-http2-15 Section 3.3 / draft-ietf-webtrans-http3:
        // WT-Available-Protocols は RFC 8941 List of String。
        // パース失敗は仕様上 ignore (= ヘッダー不在扱い)。
        let available = available_bytes
            .as_deref()
            .and_then(|bytes| WtAvailableProtocols::parse(bytes).ok());

        if let Some(protocol_bytes) = selected_protocol {
            // ASCII printable 検証 (sf-string の値域)
            for &b in protocol_bytes {
                if !(0x20..=0x7E).contains(&b) {
                    return Err(Error::InvalidArgument(
                        "selected_protocol contains non-printable byte".into(),
                    ));
                }
            }
            // クライアントリスト含有検証 (仕様 MUST)。失敗時はレスポンスを送らない。
            let listed = available
                .as_ref()
                .map(|av| av.protocols.iter().any(|p| p.as_bytes() == protocol_bytes))
                .unwrap_or(false);
            if !listed {
                return Err(Error::InvalidArgument(
                    "selected_protocol is not listed in WT-Available-Protocols".into(),
                ));
            }
        }

        // draft-ietf-webtrans-http2-15 Section 3.2:
        // WebTransport セッション確立時はサーバーが 2xx ステータスを返し、
        // END_STREAM は立てない (Capsule Protocol で通信を継続する)。
        let mut response = vec![HeaderField::from_static(b":status", b"200")];
        if let Some(protocol_bytes) = selected_protocol {
            let serialized = serialize_wt_protocol(protocol_bytes).map_err(Error::from)?;
            response.push(
                HeaderField::new(b"wt-protocol", &serialized).map_err(|e| {
                    Error::InvalidArgument(format!("build wt-protocol header: {e}"))
                })?,
            );
        }
        conn.send_response(stream_id, response, false).await?;

        // WtSession を作成して Active 状態にする
        let mut wt_session = WtSession::server(config, peer_config);
        wt_session.initiate()?;

        // Actor チャネル
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (bidi_tx, bidi_rx) = mpsc::unbounded_channel();
        let (uni_tx, uni_rx) = mpsc::unbounded_channel();
        let (datagram_tx, datagram_rx) = mpsc::unbounded_channel();

        let selected_protocol_owned = selected_protocol.map(|p| p.to_vec());
        let driver_cmd_tx = cmd_tx.clone();
        let driver = tokio::spawn(async move {
            let mut state = DriverState {
                conn,
                connect_stream_id: stream_id,
                wt_session,
                bidi_tx,
                uni_tx,
                datagram_tx,
                cmd_rx,
                cmd_tx: driver_cmd_tx,
                stream_channels: HashMap::new(),
                peer_closed_bidi_count: 0,
                peer_closed_uni_count: 0,
                responded_end_stream_on_close: false,
                stop_sending_sent_streams: HashSet::new(),
            };
            state.run().await
        });

        Ok(WtServerSession {
            session_id: u64::from(stream_id.as_u32()),
            selected_protocol: selected_protocol_owned,
            cmd_tx,
            bidi_rx,
            uni_rx,
            datagram_rx,
            driver: Some(driver),
        })
    }

    /// セッションを拒否する
    ///
    /// 指定した HTTP ステータスで END_STREAM 付きレスポンスを送信する。
    ///
    /// # Errors
    ///
    /// `status` が有効な HTTP ステータスコード (RFC 9110 §15: 100..=599) でない場合は
    /// `Error::Io` を返す。
    pub async fn reject(self, status: u16) -> Result<()> {
        let Self {
            mut conn,
            stream_id,
            ..
        } = self;
        if !(100..=599).contains(&status) {
            return Err(crate::error::Error::Io(std::io::Error::other(format!(
                "reject: status code must be in 100..=599 (RFC 9110 §15), got {status}"
            ))));
        }
        let status_str = status.to_string();
        let response = vec![
            HeaderField::new(":status", &status_str)
                .expect("3-digit numeric status produces a valid :status header"),
        ];
        conn.send_response(stream_id, response, true).await?;
        Ok(())
    }
}

/// WebTransport セッション (サーバー側)
///
/// driver タスクと mpsc/oneshot で通信する。セッション終了時は `close()` を
/// 呼ぶと driver が片付けをしてから落ちる。drop 時は cmd_tx が閉じ、
/// driver が自然に終了する。
pub struct WtServerSession {
    session_id: u64,
    selected_protocol: Option<Vec<u8>>,
    cmd_tx: mpsc::UnboundedSender<DriverCmd>,
    bidi_rx: mpsc::UnboundedReceiver<WtBidiStream>,
    uni_rx: mpsc::UnboundedReceiver<WtUniRecvStream>,
    datagram_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    driver: Option<JoinHandle<Result<()>>>,
}

impl WtServerSession {
    /// セッション ID (CONNECT ストリームの HTTP/2 Stream ID) を取得する
    #[must_use]
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    /// 選択したサブプロトコルを取得する
    ///
    /// `accept(..., selected_protocol: Some(_))` で受理した場合にその値を返す。
    #[must_use]
    pub fn selected_protocol(&self) -> Option<&[u8]> {
        self.selected_protocol.as_deref()
    }

    /// セッションを分解して bidi_rx / uni_rx / datagram_rx / handle を取り出す
    ///
    /// `tokio::select!` で並列に bidi / uni / datagram を扱いたい場合に使用する。
    pub fn into_parts(mut self) -> WtSessionParts {
        WtSessionParts {
            session_id: self.session_id,
            selected_protocol: self.selected_protocol.take(),
            bidi_rx: std::mem::replace(&mut self.bidi_rx, mpsc::unbounded_channel().1),
            uni_rx: std::mem::replace(&mut self.uni_rx, mpsc::unbounded_channel().1),
            datagram_rx: std::mem::replace(&mut self.datagram_rx, mpsc::unbounded_channel().1),
            handle: WtSessionHandle {
                cmd_tx: self.cmd_tx.clone(),
            },
            driver: self.driver.take().expect("driver must be present"),
        }
    }

    /// 次の双方向ストリームの到着を待つ
    pub async fn accept_bidi(&mut self) -> Option<WtBidiStream> {
        self.bidi_rx.recv().await
    }

    /// 次の単方向受信ストリームの到着を待つ
    pub async fn accept_uni(&mut self) -> Option<WtUniRecvStream> {
        self.uni_rx.recv().await
    }

    /// 次の DATAGRAM を待つ
    pub async fn recv_datagram(&mut self) -> Option<Vec<u8>> {
        self.datagram_rx.recv().await
    }

    /// 双方向ストリームをローカルから開く
    pub async fn open_bidi(&mut self) -> Result<WtBidiStream> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::OpenBidi { ack })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// 単方向送信ストリームをローカルから開く
    pub async fn open_uni(&mut self) -> Result<WtUniSendStream> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::OpenUni { ack })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// DATAGRAM を送信する
    pub async fn send_datagram(&mut self, data: Vec<u8>) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::SendDatagram { data, ack })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// TLS Keying Material Exporter で鍵素材を導出する
    ///
    /// draft-ietf-webtrans-http2-15 Section 5.3:
    /// ラベルは `EXPORTER-WebTransport`、コンテキストは
    /// `serialize_exporter_context(session_id, app_label, app_context)` の結果。
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

    /// セッションを `WT_CLOSE_SESSION` で終了する
    ///
    /// # Errors
    ///
    /// driver が既に終了している場合は `Error::ConnectionClosed` を返す。
    /// セッションが既に閉じている場合は `Error::WebTransport` を返す。
    /// 出力フラッシュや END_STREAM 送信に失敗した場合は、その実際のエラー
    /// (I/O / プロトコルエラー等) を返す。
    pub async fn close(mut self, error_code: u32, reason: &str) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::Close {
                error_code,
                reason: reason.to_string(),
                ack,
            })
            .map_err(|_| Error::ConnectionClosed)?;
        let res = rx.await.map_err(|_| Error::ConnectionClosed)?;
        // ack を受信できた時点で driver は必ず正常終了する
        // (Close 処理は ack 送信後に `false` でループを抜ける) ため、
        // `driver.await` の結果は無視してよい (上の `res` は無視しない)
        if let Some(driver) = self.driver.take() {
            let _ = driver.await;
        }
        res
    }

    /// セッションを `WT_DRAIN_SESSION` で drain する
    pub async fn drain(&mut self) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::Drain { ack })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }
}

impl Drop for WtServerSession {
    fn drop(&mut self) {
        if let Some(driver) = self.driver.take() {
            driver.abort();
        }
    }
}

/// `WtServerSession::into_parts` で分解された構成要素
pub struct WtSessionParts {
    /// セッション ID
    pub session_id: u64,
    /// 選択したサブプロトコル (`accept` で指定した場合)
    pub selected_protocol: Option<Vec<u8>>,
    /// 対向からの双方向ストリーム到着チャネル
    pub bidi_rx: mpsc::UnboundedReceiver<WtBidiStream>,
    /// 対向からの単方向ストリーム到着チャネル
    pub uni_rx: mpsc::UnboundedReceiver<WtUniRecvStream>,
    /// DATAGRAM 受信チャネル
    pub datagram_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    /// 送信系 API を提供するハンドル
    pub handle: WtSessionHandle,
    /// driver タスクの JoinHandle。drop で abort される。
    pub driver: JoinHandle<Result<()>>,
}

/// `WtServerSession` の送信系 API を提供するハンドル
///
/// `into_parts` で複製され、複数の async タスクから送信操作を行える。
/// 内部的には driver タスクへの mpsc sender のみを保持する。
#[derive(Clone)]
pub struct WtSessionHandle {
    cmd_tx: mpsc::UnboundedSender<DriverCmd>,
}

impl WtSessionHandle {
    /// 双方向ストリームをローカルから開く
    pub async fn open_bidi(&self) -> Result<WtBidiStream> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::OpenBidi { ack })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// 単方向送信ストリームをローカルから開く
    pub async fn open_uni(&self) -> Result<WtUniSendStream> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::OpenUni { ack })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// DATAGRAM を送信する
    pub async fn send_datagram(&self, data: Vec<u8>) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::SendDatagram { data, ack })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// TLS Keying Material Exporter で鍵素材を導出する
    ///
    /// 詳細は [`WtServerSession::export_keying_material`] を参照。
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

    /// セッションを `WT_CLOSE_SESSION` で終了する
    pub async fn close(&self, error_code: u32, reason: &str) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::Close {
                error_code,
                reason: reason.to_string(),
                ack,
            })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// セッションを `WT_DRAIN_SESSION` で drain する
    pub async fn drain(&self) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCmd::Drain { ack })
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }
}

/// WebTransport 双方向ストリーム
pub struct WtBidiStream {
    stream_id: WtStreamId,
    cmd_tx: mpsc::UnboundedSender<DriverCmd>,
    data_rx: mpsc::UnboundedReceiver<StreamPacket>,
    recv_finished: bool,
}

impl WtBidiStream {
    /// ストリーム ID を取得する
    #[must_use]
    pub const fn stream_id(&self) -> WtStreamId {
        self.stream_id
    }

    /// データを送信する (fin=true で END_STREAM)
    pub async fn send(&self, data: Vec<u8>, fin: bool) -> Result<()> {
        send_stream_data(&self.cmd_tx, self.stream_id, data, fin).await
    }

    /// 受信データを待つ。`Ok(None)` は FIN で正常終了。
    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        if self.recv_finished {
            return Ok(None);
        }
        match self.data_rx.recv().await {
            Some(packet) => match packet {
                StreamPacket::Data { data, fin } => {
                    if fin {
                        self.recv_finished = true;
                    }
                    Ok(Some(data))
                }
                StreamPacket::Reset { error_code } => {
                    self.recv_finished = true;
                    Err(Error::InvalidArgument(format!(
                        "stream reset by peer (error_code={error_code})"
                    )))
                }
            },
            None => {
                self.recv_finished = true;
                Ok(None)
            }
        }
    }

    /// 送信側に STOP_SENDING を送る
    pub async fn stop_sending(&self, error_code: u64) -> Result<()> {
        stop_sending(&self.cmd_tx, self.stream_id, error_code).await
    }

    /// ストリームをリセットする
    pub async fn reset(&self, error_code: u64) -> Result<()> {
        reset_stream(&self.cmd_tx, self.stream_id, error_code).await
    }
}

/// WebTransport 単方向受信ストリーム
pub struct WtUniRecvStream {
    stream_id: WtStreamId,
    cmd_tx: mpsc::UnboundedSender<DriverCmd>,
    data_rx: mpsc::UnboundedReceiver<StreamPacket>,
    recv_finished: bool,
}

impl WtUniRecvStream {
    /// ストリーム ID を取得する
    #[must_use]
    pub const fn stream_id(&self) -> WtStreamId {
        self.stream_id
    }

    /// 受信データを待つ。`Ok(None)` は FIN で正常終了。
    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        if self.recv_finished {
            return Ok(None);
        }
        match self.data_rx.recv().await {
            Some(packet) => match packet {
                StreamPacket::Data { data, fin } => {
                    if fin {
                        self.recv_finished = true;
                    }
                    Ok(Some(data))
                }
                StreamPacket::Reset { error_code } => {
                    self.recv_finished = true;
                    Err(Error::InvalidArgument(format!(
                        "stream reset by peer (error_code={error_code})"
                    )))
                }
            },
            None => {
                self.recv_finished = true;
                Ok(None)
            }
        }
    }

    /// 送信側に STOP_SENDING を送る
    pub async fn stop_sending(&self, error_code: u64) -> Result<()> {
        stop_sending(&self.cmd_tx, self.stream_id, error_code).await
    }
}

/// WebTransport 単方向送信ストリーム
pub struct WtUniSendStream {
    stream_id: WtStreamId,
    cmd_tx: mpsc::UnboundedSender<DriverCmd>,
}

impl WtUniSendStream {
    /// ストリーム ID を取得する
    #[must_use]
    pub const fn stream_id(&self) -> WtStreamId {
        self.stream_id
    }

    /// データを送信する (fin=true で END_STREAM)
    pub async fn send(&self, data: Vec<u8>, fin: bool) -> Result<()> {
        send_stream_data(&self.cmd_tx, self.stream_id, data, fin).await
    }

    /// ストリームをリセットする
    pub async fn reset(&self, error_code: u64) -> Result<()> {
        reset_stream(&self.cmd_tx, self.stream_id, error_code).await
    }
}

async fn send_stream_data(
    cmd_tx: &mpsc::UnboundedSender<DriverCmd>,
    stream_id: WtStreamId,
    data: Vec<u8>,
    fin: bool,
) -> Result<()> {
    let (ack, rx) = oneshot::channel();
    cmd_tx
        .send(DriverCmd::SendStreamData {
            stream_id,
            data,
            fin,
            ack,
        })
        .map_err(|_| Error::ConnectionClosed)?;
    rx.await.map_err(|_| Error::ConnectionClosed)?
}

async fn stop_sending(
    cmd_tx: &mpsc::UnboundedSender<DriverCmd>,
    stream_id: WtStreamId,
    error_code: u64,
) -> Result<()> {
    let (ack, rx) = oneshot::channel();
    cmd_tx
        .send(DriverCmd::StopSending {
            stream_id,
            error_code,
            ack,
        })
        .map_err(|_| Error::ConnectionClosed)?;
    rx.await.map_err(|_| Error::ConnectionClosed)?
}

async fn reset_stream(
    cmd_tx: &mpsc::UnboundedSender<DriverCmd>,
    stream_id: WtStreamId,
    error_code: u64,
) -> Result<()> {
    let (ack, rx) = oneshot::channel();
    cmd_tx
        .send(DriverCmd::ResetStream {
            stream_id,
            error_code,
            ack,
        })
        .map_err(|_| Error::ConnectionClosed)?;
    rx.await.map_err(|_| Error::ConnectionClosed)?
}

/// Driver に対するコマンド
enum DriverCmd {
    SendStreamData {
        stream_id: WtStreamId,
        data: Vec<u8>,
        fin: bool,
        ack: oneshot::Sender<Result<()>>,
    },
    OpenBidi {
        ack: oneshot::Sender<Result<WtBidiStream>>,
    },
    OpenUni {
        ack: oneshot::Sender<Result<WtUniSendStream>>,
    },
    SendDatagram {
        data: Vec<u8>,
        ack: oneshot::Sender<Result<()>>,
    },
    ResetStream {
        stream_id: WtStreamId,
        error_code: u64,
        ack: oneshot::Sender<Result<()>>,
    },
    StopSending {
        stream_id: WtStreamId,
        error_code: u64,
        ack: oneshot::Sender<Result<()>>,
    },
    Close {
        error_code: u32,
        reason: String,
        ack: oneshot::Sender<Result<()>>,
    },
    Drain {
        ack: oneshot::Sender<Result<()>>,
    },
    ExportKeyingMaterial {
        app_label: Vec<u8>,
        app_context: Vec<u8>,
        length: usize,
        ack: oneshot::Sender<Result<Vec<u8>>>,
    },
}

/// Stream チャネルに流すメッセージ
enum StreamPacket {
    Data { data: Vec<u8>, fin: bool },
    Reset { error_code: u64 },
}

struct DriverState {
    conn: ServerConnection,
    connect_stream_id: StreamId,
    wt_session: WtSession,
    bidi_tx: mpsc::UnboundedSender<WtBidiStream>,
    uni_tx: mpsc::UnboundedSender<WtUniRecvStream>,
    datagram_tx: mpsc::UnboundedSender<Vec<u8>>,
    cmd_rx: mpsc::UnboundedReceiver<DriverCmd>,
    cmd_tx: mpsc::UnboundedSender<DriverCmd>,
    stream_channels: HashMap<WtStreamId, mpsc::UnboundedSender<StreamPacket>>,
    /// ピア側から開いて閉じた双方向ストリームの数 (WT_MAX_STREAMS 自動発行用)
    peer_closed_bidi_count: u64,
    /// ピア側から開いて閉じた単方向ストリームの数 (WT_MAX_STREAMS 自動発行用)
    peer_closed_uni_count: u64,
    /// WT_CLOSE_SESSION 受信時に END_STREAM で応答済みか
    responded_end_stream_on_close: bool,
    /// WT_STOP_SENDING を送信したストリーム ID の集合
    ///
    /// STOP_SENDING 後の在路データを破棄し、ストリームウィンドウ拡張を行わない
    /// ために使用する。ピアからの FIN / リセット受信で受信方向が終端したら削除する。
    /// ピア開始 uni ストリームの FIN 付き `StreamData` は `WtSession::poll_event` が
    /// 先にストリームを削除するため `WtSession::stream` では判定できず、ドライバ側で
    /// 記録する必要がある。
    stop_sending_sent_streams: HashSet<WtStreamId>,
}

impl DriverState {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                biased;
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(cmd) => {
                            if !self.handle_cmd(cmd).await {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                event = self.conn.next_event() => {
                    match event {
                        Ok(ev) => self.handle_event(ev).await?,
                        Err(Error::ConnectionClosed) => break,
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        Ok(())
    }

    /// コマンドを処理し、ループ継続可否を返す
    ///
    /// 処理結果 (成功・失敗) は必ず ack 経由で呼び出し側へ返す。driver を終了させる
    /// ケースも `false` で表現するため、本メソッドはエラーを返さない。
    async fn handle_cmd(&mut self, cmd: DriverCmd) -> bool {
        match cmd {
            DriverCmd::SendStreamData {
                stream_id,
                data,
                fin,
                ack,
            } => {
                let res = self
                    .wt_session
                    .send_stream_data(stream_id, &data, fin)
                    .map_err(Error::from);
                self.send_cmd_result(res, ack).await
            }
            DriverCmd::OpenBidi { ack } => {
                let res = match self.wt_session.open_bidi_stream() {
                    Ok(stream_id) => {
                        let (tx, rx) = mpsc::unbounded_channel();
                        self.stream_channels.insert(stream_id, tx);
                        Ok(WtBidiStream {
                            stream_id,
                            cmd_tx: self.cmd_tx.clone(),
                            data_rx: rx,
                            recv_finished: false,
                        })
                    }
                    Err(e) => Err(Error::from(e)),
                };
                self.send_cmd_result(res, ack).await
            }
            DriverCmd::OpenUni { ack } => {
                let res = match self.wt_session.open_uni_stream() {
                    Ok(stream_id) => Ok(WtUniSendStream {
                        stream_id,
                        cmd_tx: self.cmd_tx.clone(),
                    }),
                    Err(e) => Err(Error::from(e)),
                };
                self.send_cmd_result(res, ack).await
            }
            DriverCmd::SendDatagram { data, ack } => {
                let res = self.wt_session.send_datagram(&data).map_err(Error::from);
                self.send_cmd_result(res, ack).await
            }
            DriverCmd::ResetStream {
                stream_id,
                error_code,
                ack,
            } => {
                let res = self
                    .wt_session
                    .reset_stream(stream_id, error_code)
                    .map_err(Error::from);
                self.send_cmd_result(res, ack).await
            }
            DriverCmd::StopSending {
                stream_id,
                error_code,
                ack,
            } => {
                let res = self
                    .wt_session
                    .stop_sending(stream_id, error_code)
                    .map_err(Error::from);
                if res.is_ok()
                    && self
                        .wt_session
                        .stream(stream_id)
                        .is_some_and(|s| s.can_recv())
                {
                    // STOP_SENDING 後の在路データを破棄するため ID を記録する。
                    // 受信側が既に終端の場合は以降データが届かないため記録しない
                    // (記録すると削除されず長命セッションで残り続ける)。
                    self.stop_sending_sent_streams.insert(stream_id);
                }
                self.send_cmd_result(res, ack).await
            }
            DriverCmd::Close {
                error_code,
                reason,
                ack,
            } => {
                let mut res = self
                    .wt_session
                    .close(error_code, &reason)
                    .map_err(Error::from);
                if res.is_ok() {
                    // draft-ietf-webtrans-http2-15 Section 6.12 (L1405-L1406):
                    // WT_CLOSE_SESSION 送信後は MUST half-close the stream。このため
                    // 出力フラッシュに続けて空 DATA + END_STREAM を送信する。
                    // RFC 9113 Section 6.9.1: 空 DATA + END_STREAM はフロー制御ウィンドウ
                    // 空きなしでも送信可能。
                    // フラッシュや END_STREAM 送信の失敗は ack に載せて呼び出し側へ伝える。
                    // 注: 送信ウィンドウ枯渇時、sans-io 層はデータをバッファへ積むだけで
                    // Ok を返す (実際には未送信) ことがある。その場合も driver 終了で
                    // コネクションごと破棄され、close 処理の出力 (WT_CLOSE_SESSION /
                    // END_STREAM) はピアへ届かない。
                    if let Err(e) = self.flush_wt_output().await {
                        res = Err(e);
                    } else if let Err(e) = self
                        .conn
                        .send_data(self.connect_stream_id, vec![], true)
                        .await
                    {
                        res = Err(e);
                    } else {
                        self.responded_end_stream_on_close = true;
                    }
                }
                let _ = ack.send(res);
                // Close は結果に関わらずループを抜けて driver を終了する
                false
            }
            DriverCmd::Drain { ack } => {
                let res = self.wt_session.drain().map_err(Error::from);
                self.send_cmd_result(res, ack).await
            }
            DriverCmd::ExportKeyingMaterial {
                app_label,
                app_context,
                length,
                ack,
            } => {
                let res = (|| -> Result<Vec<u8>> {
                    if length == 0 {
                        return Err(Error::InvalidArgument(
                            "export length must be greater than zero".into(),
                        ));
                    }
                    let session_id = u64::from(self.connect_stream_id.as_u32());
                    let ctx = serialize_exporter_context(session_id, &app_label, &app_context)
                        .map_err(Error::from)?;
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
                true
            }
        }
    }

    /// セッション操作の結果を ack で必ず呼び出し側へ返す
    ///
    /// 本メソッドはフラッシュを伴うセッション操作 (ストリーム送信・ストリーム開設・
    /// DATAGRAM 送信・リセット・STOP_SENDING・drain) に使用する。END_STREAM 送信を
    /// 伴う Close と、WT 出力を生まない `ExportKeyingMaterial` は handle_cmd 内で
    /// 直接処理する。
    ///
    /// セッション操作が成功した場合は WT 出力をフラッシュする。フラッシュの失敗は
    /// ack にエラーとして載せて呼び出し側へ実際の失敗原因を伝えたうえで `false` を
    /// 返して driver を終了する。フラッシュ失敗はコネクションが使えない状態か、
    /// 送信バッファ (固定容量 65535) に収まらない出力を一度に送ろうとした状態
    /// (send buffer full) である。全量拒否のため capsule は積まれないが、
    /// `poll_output` で drain 済みの出力は破棄されるため、このまま継続すると
    /// データ欠落となる。終了が安全側の選択である。
    /// セッション操作自体の失敗 (例: クローズ済みストリームへの送信・送信ウィンドウ
    /// 枯渇) は ack に載せるだけで driver は継続する。ストリーム個別の失敗で
    /// コネクション全体を終了させる必要はないため。
    ///
    /// 返り値は `run()` のループ継続判定で、`true` は継続・`false` は終了を表す。
    /// エラーはすべて ack 経由で呼び出し側へ届くため、本メソッドはエラーを返さない。
    async fn send_cmd_result<T>(
        &mut self,
        res: Result<T>,
        ack: oneshot::Sender<Result<T>>,
    ) -> bool {
        let value = match res {
            Ok(value) => value,
            Err(e) => {
                let _ = ack.send(Err(e));
                return true;
            }
        };
        match self.flush_wt_output().await {
            Ok(()) => {
                let _ = ack.send(Ok(value));
                true
            }
            Err(e) => {
                let _ = ack.send(Err(e));
                false
            }
        }
    }

    async fn handle_event(&mut self, ev: Event) -> Result<()> {
        match ev {
            Event::DataReceived {
                stream_id,
                data,
                end_stream,
            } if stream_id == self.connect_stream_id => {
                // RFC 9113 Section 6.9.1: DATA 受信は接続およびストリームのフロー制御
                // ウィンドウから計上される。WebTransport セッションは単一の CONNECT
                // ストリーム上で大量データをやり取りするため、ピアが送り続けられるよう
                // 受信した分の WINDOW_UPDATE を即座に返す。
                let data_len_u32 =
                    u32::try_from(data.len()).expect("DATA payload fits in u32 per RFC 9113");

                if let Err(e) = self.wt_session.feed(&data) {
                    return Err(self.abort_session_with_wt_error(e).await);
                }
                if let Err(e) = self.wt_session.process() {
                    return Err(self.abort_session_with_wt_error(e).await);
                }

                while let Some(wt_ev) = self.wt_session.poll_event() {
                    self.dispatch_wt_event(wt_ev).await?;
                }

                // draft-ietf-webtrans-http2-15 Section 6.12 (L1407-L1409):
                // WT_CLOSE_SESSION 受信時は MUST close the stream with END_STREAM。
                // end_stream=true と WT_CLOSE_SESSION が同一 DATA フレームに
                // 含まれている場合でも、先に END_STREAM を返信する必要があるため
                // このチェックは end_stream 判定より前に置く。
                if self.wt_session.state() == WtSessionState::Closed
                    && !self.responded_end_stream_on_close
                {
                    self.conn
                        .send_data(self.connect_stream_id, vec![], true)
                        .await?;
                    self.responded_end_stream_on_close = true;
                    return Err(Error::ConnectionClosed);
                }

                // draft-ietf-webtrans-http2-15 Section 6: 受信時にフロー制御を更新する
                if let Err(e) = self.maybe_grow_session_window() {
                    return Err(self.abort_session_with_wt_error(e).await);
                }
                if let Err(e) = self.maybe_grow_max_streams(true) {
                    return Err(self.abort_session_with_wt_error(e).await);
                }
                if let Err(e) = self.maybe_grow_max_streams(false) {
                    return Err(self.abort_session_with_wt_error(e).await);
                }

                if data_len_u32 > 0 {
                    // RFC 9113 Section 6.9: 接続レベルの WINDOW_UPDATE は他ストリームの
                    // 受信余地を維持するため end_stream に関わらず送信する。
                    self.conn
                        .send_window_update(StreamId::Connection, data_len_u32)
                        .await?;
                    // RFC 9113 Section 6.9: end_stream のときは直後にストリームが
                    // closed になるためストリームレベルの WINDOW_UPDATE は不要。
                    if !end_stream {
                        self.conn
                            .send_window_update(self.connect_stream_id, data_len_u32)
                            .await?;
                    }
                }

                self.flush_wt_output().await?;

                if end_stream {
                    // CONNECT ストリーム自体の終了 -> セッション終了
                    return Err(Error::ConnectionClosed);
                }
            }
            Event::StreamReset { stream_id, .. } | Event::StreamClosed { stream_id }
                if stream_id == self.connect_stream_id =>
            {
                return Err(Error::ConnectionClosed);
            }
            _ => {
                // 他のイベントは無視 (単一 WT セッション専用の接続を想定)
            }
        }
        Ok(())
    }

    /// セッション終了系の `WtError` なら CONNECT に RST_STREAM を送ってから返す
    async fn abort_session_with_wt_error(
        &mut self,
        e: shiguredo_http2::webtransport::WtError,
    ) -> Error {
        if let Some(code) = wt_http2_error_code(e.kind()) {
            let _ = self.conn.reset_stream(self.connect_stream_id, code).await;
        }
        Error::from(e)
    }

    /// セッションレベルの受信ウィンドウを必要に応じて拡張する
    fn maybe_grow_session_window(
        &mut self,
    ) -> std::result::Result<(), shiguredo_http2::webtransport::WtError> {
        let initial = self.wt_session.config().initial_max_data;
        if self.wt_session.flow_control().should_send_max_data(initial) {
            self.wt_session.grow_recv_window(initial)?;
        }
        Ok(())
    }

    /// ストリームレベルの受信ウィンドウを必要に応じて拡張する
    fn maybe_grow_stream_window(
        &mut self,
        stream_id: WtStreamId,
    ) -> std::result::Result<(), shiguredo_http2::webtransport::WtError> {
        let (bidirectional, recv_available) = match self.wt_session.stream(stream_id) {
            Some(s) => (s.is_bidirectional(), s.recv_available()),
            None => return Ok(()),
        };
        let initial = if bidirectional {
            // draft-ietf-webtrans-http2-15 Section 11.2:
            // ローカル開始 bidi の recv_max は bidi_local、ピア開始 bidi は bidi_remote で
            // 初期化されるため、しきい値・拡張量も開始主体に応じて選ぶ
            // 現在の DriverState はサーバー専用のため Role::Client は到達しないが、
            // 将来のクライアントドライバ追加に備えて role ごとに判定する
            let locally_initiated = match self.wt_session.role() {
                Role::Client => wt_stream_id::is_client_initiated(stream_id),
                Role::Server => wt_stream_id::is_server_initiated(stream_id),
            };
            if locally_initiated {
                self.wt_session.config().initial_max_stream_data_bidi_local
            } else {
                self.wt_session.config().initial_max_stream_data_bidi_remote
            }
        } else {
            self.wt_session.config().initial_max_stream_data_uni
        };
        if recv_available < initial / 2 {
            self.wt_session
                .grow_stream_recv_window(stream_id, initial)?;
        }
        Ok(())
    }

    /// ストリーム数上限を必要に応じて拡張する
    fn maybe_grow_max_streams(
        &mut self,
        bidirectional: bool,
    ) -> std::result::Result<(), shiguredo_http2::webtransport::WtError> {
        let initial = if bidirectional {
            self.wt_session.config().initial_max_streams_bidi
        } else {
            self.wt_session.config().initial_max_streams_uni
        };
        if initial == 0 {
            return Ok(());
        }
        let closed = if bidirectional {
            self.peer_closed_bidi_count
        } else {
            self.peer_closed_uni_count
        };
        if closed * 2 >= initial {
            self.wt_session.grow_max_streams(closed, bidirectional)?;
            if bidirectional {
                self.peer_closed_bidi_count = 0;
            } else {
                self.peer_closed_uni_count = 0;
            }
        }
        Ok(())
    }

    async fn dispatch_wt_event(&mut self, ev: WtEvent) -> Result<()> {
        match ev {
            WtEvent::StreamOpened {
                stream_id,
                bidirectional,
            } => {
                let (tx, rx) = mpsc::unbounded_channel();
                self.stream_channels.insert(stream_id, tx);
                if bidirectional {
                    let _ = self.bidi_tx.send(WtBidiStream {
                        stream_id,
                        cmd_tx: self.cmd_tx.clone(),
                        data_rx: rx,
                        recv_finished: false,
                    });
                } else {
                    let _ = self.uni_tx.send(WtUniRecvStream {
                        stream_id,
                        cmd_tx: self.cmd_tx.clone(),
                        data_rx: rx,
                        recv_finished: false,
                    });
                }
            }
            WtEvent::StreamData {
                stream_id,
                data,
                fin,
            } => {
                // RFC 9000 Section 3.5: STOP_SENDING 送信後の受信データは
                // アプリへ配送せず破棄し、ストリームウィンドウも拡張しない。
                // ウィンドウ拡張の抑止は draft-ietf-webtrans-http2-15 Section 6.6 の
                // WT_MAX_STREAM_DATA 送信禁止 (MUST NOT) に従う。
                // 破棄しても connection / stream のフロー制御への計上は
                // sans-io 層 (`WtStream::recv_data` と `WtFlowControl::consume_recv`) で継続する。
                if self.stop_sending_sent_streams.contains(&stream_id) {
                    if fin {
                        self.stream_channels.remove(&stream_id);
                        self.account_peer_stream_closed(stream_id);
                        self.stop_sending_sent_streams.remove(&stream_id);
                    }
                    return Ok(());
                }
                if let Some(ch) = self.stream_channels.get(&stream_id) {
                    let _ = ch.send(StreamPacket::Data { data, fin });
                }
                // ストリームレベルのフロー制御を更新する
                if let Err(e) = self.maybe_grow_stream_window(stream_id) {
                    return Err(self.abort_session_with_wt_error(e).await);
                }
                if fin {
                    self.stream_channels.remove(&stream_id);
                    self.account_peer_stream_closed(stream_id);
                }
            }
            WtEvent::StreamReset {
                stream_id,
                error_code,
            } => {
                if let Some(ch) = self.stream_channels.remove(&stream_id) {
                    let _ = ch.send(StreamPacket::Reset { error_code });
                }
                self.account_peer_stream_closed(stream_id);
                self.stop_sending_sent_streams.remove(&stream_id);
            }
            WtEvent::StopSending { .. } => {
                // 現在の API では送信側にシグナルを伝達しない (将来の拡張)
            }
            WtEvent::DatagramReceived { data } => {
                let _ = self.datagram_tx.send(data);
            }
            WtEvent::SessionDraining | WtEvent::SessionClosed { .. } => {
                // 何もしない (ユーザーに close/drain を通知する手段は将来追加)
            }
        }
        Ok(())
    }

    /// ピアが開いたストリームのクローズをカウントする
    fn account_peer_stream_closed(&mut self, stream_id: WtStreamId) {
        // クライアント開始ストリームのみカウント (サーバーから見てピア発起)
        if !wt_stream_id::is_client_initiated(stream_id) {
            return;
        }
        if wt_stream_id::is_bidirectional(stream_id) {
            self.peer_closed_bidi_count = self.peer_closed_bidi_count.saturating_add(1);
        } else {
            self.peer_closed_uni_count = self.peer_closed_uni_count.saturating_add(1);
        }
    }

    async fn flush_wt_output(&mut self) -> Result<()> {
        while let Some(out) = self.wt_session.poll_output() {
            self.conn
                .send_data(self.connect_stream_id, out, false)
                .await?;
        }
        Ok(())
    }
}

/// セッション終了系 `WtErrorKind` を HTTP/2 エラーコードへ対応付ける
///
/// draft-ietf-webtrans-http2-15 Section 3.4 / Section 11.3:
/// セッションエラーは CONNECT ストリームの RST_STREAM で伝える。
fn wt_http2_error_code(kind: WtErrorKind) -> Option<ErrorCode> {
    match kind {
        WtErrorKind::StreamStateError => Some(ErrorCode::WtStreamStateError),
        WtErrorKind::FlowControlError => Some(ErrorCode::WtFlowControlError),
        WtErrorKind::SessionStateError => Some(ErrorCode::WtError),
        _ => None,
    }
}

/// TLS バージョンをエラー文字列に埋め込むための説明文字列に変換する
///
/// `rustls::ProtocolVersion` の `Debug` 実装に依存すると将来の表現変更で
/// 文言がブレるため、固定の英語ラベルにマップする。
fn describe_tls_version(version: Option<rustls::ProtocolVersion>) -> &'static str {
    match version {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLS 1.3",
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLS 1.2",
        Some(rustls::ProtocolVersion::TLSv1_0) => "TLS 1.0",
        Some(rustls::ProtocolVersion::TLSv1_1) => "TLS 1.1",
        Some(_) => "unsupported TLS version",
        None => "no TLS version negotiated",
    }
}
