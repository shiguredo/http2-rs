//! WebTransport over HTTP/2 (draft-ietf-webtrans-http2-14)
//!
//! # 概要
//!
//! WebTransport セッションは HTTP/2 Extended CONNECT ストリーム上で動作し、
//! Capsule Protocol でデータを多重化する。
//!
//! # 参照仕様
//!
//! - draft-ietf-webtrans-http2-14 (WebTransport over HTTP/2)
//! - RFC 9297 (HTTP Datagrams and the Capsule Protocol)
//! - RFC 8441 (Bootstrapping WebSockets with HTTP/2 - Extended CONNECT)
//! - RFC 9000 Section 2.1 (Stream Types and Identifiers)
//! - RFC 9000 Section 3 (Stream States)
//! - RFC 9000 Section 16 (Variable-Length Integer Encoding)

pub mod capsule;
pub mod error;
pub mod flow_control;
pub mod stream;
pub mod varint;

use std::collections::{HashMap, VecDeque};

use bytes::{Bytes, BytesMut};

use crate::connection::Role;

pub use capsule::{Capsule, CapsuleDecoder, CapsuleEncoder, capsule_type};
pub use error::{WtError, WtErrorKind, WtResult};
pub use flow_control::WtFlowControl;
pub use stream::{RecvState, SendState, WtStream, WtStreamId};
pub use varint::{MAX_VALUE, decode as varint_decode, encode as varint_encode, encoded_len};

/// WebTransport セッション状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WtSessionState {
    /// 初期状態
    #[default]
    Initial,
    /// アクティブ（通常の通信中）
    Active,
    /// ドレイン中（新規ストリーム受付停止）
    Draining,
    /// クローズ済み
    Closed,
}

/// WebTransport イベント
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WtEvent {
    /// ストリームが開かれた
    StreamOpened {
        stream_id: WtStreamId,
        bidirectional: bool,
    },
    /// ストリームデータを受信
    StreamData {
        stream_id: WtStreamId,
        data: Bytes,
        fin: bool,
    },
    /// ストリームがリセットされた
    StreamReset {
        stream_id: WtStreamId,
        error_code: u64,
    },
    /// ストリーム送信停止要求を受信
    StopSending {
        stream_id: WtStreamId,
        error_code: u64,
    },
    /// データグラムを受信
    DatagramReceived { data: Bytes },
    /// セッションがドレイン中
    SessionDraining,
    /// セッションがクローズされた
    SessionClosed { error_code: u32, reason: String },
}

/// WebTransport 設定
#[derive(Debug, Clone)]
pub struct WtConfig {
    /// セッションレベルの初期最大データ量
    pub initial_max_data: u64,
    /// 双方向ストリームの初期最大データ量 (自身が開始したストリーム)
    ///
    /// draft-ietf-webtrans-http2-14 Section 4.3.1:
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL に対応する。
    pub initial_max_stream_data_bidi_local: u64,
    /// 双方向ストリームの初期最大データ量 (ピアが開始したストリーム)
    ///
    /// draft-ietf-webtrans-http2-14 Section 4.3.1:
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE に対応する。
    pub initial_max_stream_data_bidi_remote: u64,
    /// 単方向ストリームの初期最大データ量
    pub initial_max_stream_data_uni: u64,
    /// 双方向ストリームの初期最大数
    pub initial_max_streams_bidi: u64,
    /// 単方向ストリームの初期最大数
    pub initial_max_streams_uni: u64,
}

impl Default for WtConfig {
    fn default() -> Self {
        Self {
            initial_max_data: 1_048_576,                  // 1 MiB
            initial_max_stream_data_bidi_local: 262_144,  // 256 KiB
            initial_max_stream_data_bidi_remote: 262_144, // 256 KiB
            initial_max_stream_data_uni: 262_144,         // 256 KiB
            initial_max_streams_bidi: 100,
            initial_max_streams_uni: 100,
        }
    }
}

/// WebTransport セッション (Sans I/O)
///
/// HTTP/2 CONNECT ストリーム上で動作する WebTransport セッションを管理する。
#[derive(Debug)]
pub struct WtSession {
    /// 接続の役割
    role: Role,
    /// 設定
    config: WtConfig,
    /// セッション状態
    state: WtSessionState,
    /// ストリーム一覧
    streams: HashMap<WtStreamId, WtStream>,
    /// フロー制御
    flow_control: WtFlowControl,
    /// Capsule デコーダー
    capsule_decoder: CapsuleDecoder,
    /// Capsule エンコーダー
    capsule_encoder: CapsuleEncoder,
    /// 出力バッファ
    ///
    /// `BytesMut` を使い、`poll_output` で `split().freeze()` により
    /// zero-copy に `Bytes` を取り出す。
    output_buffer: BytesMut,
    /// イベントキュー
    events: VecDeque<WtEvent>,
    /// 次の双方向ストリーム ID
    next_bidi_stream_id: WtStreamId,
    /// 次の単方向ストリーム ID
    next_uni_stream_id: WtStreamId,
}

impl WtSession {
    /// クライアントセッションを生成する
    #[must_use]
    pub fn client(config: WtConfig) -> Self {
        Self::new(Role::Client, config)
    }

    /// サーバーセッションを生成する
    #[must_use]
    pub fn server(config: WtConfig) -> Self {
        Self::new(Role::Server, config)
    }

    /// 新しいセッションを生成する
    fn new(role: Role, config: WtConfig) -> Self {
        let is_client = role == Role::Client;
        let flow_control = WtFlowControl::new(
            config.initial_max_data,
            config.initial_max_streams_bidi,
            config.initial_max_streams_uni,
        );

        Self {
            role,
            config,
            state: WtSessionState::Initial,
            streams: HashMap::new(),
            flow_control,
            capsule_decoder: CapsuleDecoder::new(),
            capsule_encoder: CapsuleEncoder::new(),
            output_buffer: BytesMut::new(),
            events: VecDeque::new(),
            next_bidi_stream_id: stream::stream_id::first(is_client, true),
            next_uni_stream_id: stream::stream_id::first(is_client, false),
        }
    }

    /// 接続の役割を取得する
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// セッション状態を取得する
    #[must_use]
    pub const fn state(&self) -> WtSessionState {
        self.state
    }

    /// セッションがアクティブかどうかを返す
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self.state, WtSessionState::Active)
    }

    /// セッションがクローズされたかどうかを返す
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self.state, WtSessionState::Closed)
    }

    /// セッションを開始する
    pub fn initiate(&mut self) -> WtResult<()> {
        if self.state != WtSessionState::Initial {
            return Err(WtError::session_state_error("session already initiated"));
        }
        self.state = WtSessionState::Active;
        Ok(())
    }

    /// HTTP/2 DATA フレームペイロードを入力する
    ///
    /// # 戻り値
    ///
    /// 消費したバイト数を返す。
    pub fn feed(&mut self, data: &[u8]) -> WtResult<usize> {
        self.capsule_decoder.feed(data);
        Ok(data.len())
    }

    /// Capsule を処理してイベントを生成する
    pub fn process(&mut self) -> WtResult<()> {
        while let Some(capsule) = self.capsule_decoder.decode()? {
            self.handle_capsule(capsule)?;
        }
        Ok(())
    }

    /// 送信データを取得する
    ///
    /// 蓄積されたバイト列を `Bytes` で返す。`Bytes::clone()` は Arc inc なので、
    /// relay 配信時に N 個の宛先に送る場合のコピーを排除できる。
    #[must_use]
    pub fn poll_output(&mut self) -> Option<Bytes> {
        if self.output_buffer.is_empty() {
            None
        } else {
            Some(self.output_buffer.split().freeze())
        }
    }

    /// イベントを取得する
    #[must_use]
    pub fn poll_event(&mut self) -> Option<WtEvent> {
        self.events.pop_front()
    }

    /// 出力バッファにデータがあるかどうかを返す
    #[must_use]
    pub fn has_output(&self) -> bool {
        !self.output_buffer.is_empty()
    }

    /// 双方向ストリームを開く
    pub fn open_bidi_stream(&mut self) -> WtResult<WtStreamId> {
        self.open_stream(true)
    }

    /// 単方向ストリームを開く
    pub fn open_uni_stream(&mut self) -> WtResult<WtStreamId> {
        self.open_stream(false)
    }

    /// ストリームを開く
    fn open_stream(&mut self, bidirectional: bool) -> WtResult<WtStreamId> {
        // draft-ietf-webtrans-http2-14 Section 6.13: Draining 状態でも新規ストリーム開設を許可
        if !matches!(
            self.state,
            WtSessionState::Active | WtSessionState::Draining
        ) {
            return Err(WtError::session_state_error(
                "cannot open stream: session not active or draining",
            ));
        }

        // ストリーム数制限をチェック
        if bidirectional {
            if !self.flow_control.can_open_bidi_stream() {
                return Err(WtError::flow_control_error("bidi stream limit reached"));
            }
        } else if !self.flow_control.can_open_uni_stream() {
            return Err(WtError::flow_control_error("uni stream limit reached"));
        }

        let stream_id = if bidirectional {
            let id = self.next_bidi_stream_id;
            self.next_bidi_stream_id = stream::stream_id::next(id);
            id
        } else {
            let id = self.next_uni_stream_id;
            self.next_uni_stream_id = stream::stream_id::next(id);
            id
        };

        // draft-ietf-webtrans-http2-14 Section 4.3.1:
        // LOCAL はこのエンドポイントが開始したストリーム向けの初期フロー制御上限
        let initial_max_data = if bidirectional {
            self.config.initial_max_stream_data_bidi_local
        } else {
            self.config.initial_max_stream_data_uni
        };

        let stream = WtStream::new(stream_id, initial_max_data, bidirectional);
        self.streams.insert(stream_id, stream);

        // ストリーム数を更新
        self.flow_control.opened_stream(bidirectional);

        Ok(stream_id)
    }

    /// ストリームにデータを送信する
    ///
    /// `data` を `Bytes` で受け取り、そのまま `Capsule::WtStream` に格納する。
    /// `Bytes::clone()` は Arc inc なので、relay で 1 つの `Bytes` を複数の
    /// `WtSession` に配布するときのコピーを排除できる。
    pub fn send_stream_data(
        &mut self,
        stream_id: WtStreamId,
        data: Bytes,
        fin: bool,
    ) -> WtResult<()> {
        // draft-ietf-webtrans-http2-14 Section 6.13: Draining 状態でもデータ送信を許可
        if !matches!(
            self.state,
            WtSessionState::Active | WtSessionState::Draining
        ) {
            return Err(WtError::session_state_error(
                "cannot send data: session not active or draining",
            ));
        }

        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;

        // 送信状態をチェック
        if !stream.can_send() {
            return Err(WtError::stream_state_error("cannot send on this stream"));
        }

        let data_len = data.len() as u64;

        // WT_STREAM Capsule をエンコード
        let capsule = Capsule::WtStream {
            stream_id,
            data,
            fin,
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());

        // 送信状態を更新
        stream.send_data(data_len, fin)?;

        // フロー制御を更新
        self.flow_control.consume_send(data_len)?;

        Ok(())
    }

    /// ストリームをリセットする
    pub fn reset_stream(&mut self, stream_id: WtStreamId, error_code: u64) -> WtResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;

        // draft-ietf-webtrans-http2-14 Section 6.2:
        // クローズ済みまたはリセット済みのストリームでは WT_RESET_STREAM を送信してはならない
        if !stream.can_send() {
            return Err(WtError::stream_state_error(
                "cannot send WT_RESET_STREAM: stream not in valid send state",
            ));
        }

        let reliable_size = stream.send_offset();

        // WT_RESET_STREAM Capsule をエンコード
        let capsule = Capsule::WtResetStream {
            stream_id,
            error_code,
            reliable_size,
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());

        // 送信状態を更新
        stream.send_reset();

        Ok(())
    }

    /// ストリーム送信停止を要求する
    pub fn stop_sending(&mut self, stream_id: WtStreamId, error_code: u64) -> WtResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;

        // draft-ietf-webtrans-http2-14 Section 6.3:
        // WT_STOP_SENDING を複数回送信してはならない
        if stream.stop_sending_sent() {
            return Err(WtError::stream_state_error(
                "cannot send WT_STOP_SENDING: already sent",
            ));
        }

        // WT_STOP_SENDING Capsule をエンコード
        let capsule = Capsule::WtStopSending {
            stream_id,
            error_code,
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());

        // 送信済みフラグを設定
        stream.set_stop_sending_sent();

        Ok(())
    }

    /// データグラムを送信する
    ///
    /// `data` を `Bytes` で受け取り、そのまま `Capsule::Datagram` に格納する。
    pub fn send_datagram(&mut self, data: Bytes) -> WtResult<()> {
        // draft-ietf-webtrans-http2-14 Section 6.13: Draining 状態でもデータグラム送信を許可
        if !matches!(
            self.state,
            WtSessionState::Active | WtSessionState::Draining
        ) {
            return Err(WtError::session_state_error(
                "cannot send datagram: session not active or draining",
            ));
        }

        // DATAGRAM Capsule をエンコード
        let capsule = Capsule::Datagram { data };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());

        Ok(())
    }

    /// セッションを終了する
    pub fn close(&mut self, error_code: u32, reason: &str) -> WtResult<()> {
        if self.state == WtSessionState::Closed {
            return Err(WtError::session_state_error("session already closed"));
        }

        // WT_CLOSE_SESSION Capsule をエンコード
        let capsule = Capsule::WtCloseSession {
            error_code,
            reason: reason.to_string(),
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());

        self.state = WtSessionState::Closed;

        Ok(())
    }

    /// セッションレベルのフロー制御上限を増やす `WT_MAX_DATA` を送信する
    ///
    /// draft-ietf-webtrans-http2-14 Section 6.5: 受信側が受信可能なバイト数を通知する。
    /// 単調増加でなければならない (現在値より小さい値を指定すると `flow_control_error`)。
    pub fn send_max_data(&mut self, maximum: u64) -> WtResult<()> {
        let capsule = Capsule::WtMaxData { maximum };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());
        Ok(())
    }

    /// ストリームレベルのフロー制御上限を増やす `WT_MAX_STREAM_DATA` を送信する
    ///
    /// draft-ietf-webtrans-http2-14 Section 6.6: 指定ストリームの受信可能バイト数を通知する。
    pub fn send_max_stream_data(&mut self, stream_id: WtStreamId, maximum: u64) -> WtResult<()> {
        let capsule = Capsule::WtMaxStreamData { stream_id, maximum };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());
        Ok(())
    }

    /// ストリーム数上限を増やす `WT_MAX_STREAMS` を送信する
    ///
    /// draft-ietf-webtrans-http2-14 Section 6.7: ピアが新規ストリームを開ける上限を通知する。
    pub fn send_max_streams(&mut self, maximum: u64, bidirectional: bool) -> WtResult<()> {
        let capsule = Capsule::WtMaxStreams {
            maximum,
            bidirectional,
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());
        Ok(())
    }

    /// フロー制御への参照を取得する
    #[must_use]
    pub const fn flow_control(&self) -> &WtFlowControl {
        &self.flow_control
    }

    /// フロー制御への可変参照を取得する
    pub fn flow_control_mut(&mut self) -> &mut WtFlowControl {
        &mut self.flow_control
    }

    /// 指定ストリームへの参照を取得する
    #[must_use]
    pub fn stream(&self, stream_id: WtStreamId) -> Option<&WtStream> {
        self.streams.get(&stream_id)
    }

    /// セッション受信ウィンドウを拡張し、`WT_MAX_DATA` を自動送信する
    pub fn grow_recv_window(&mut self, increment: u64) -> WtResult<()> {
        self.flow_control.add_recv_max(increment)?;
        let new_max = self.flow_control.recv_max();
        self.send_max_data(new_max)
    }

    /// ストリーム受信ウィンドウを拡張し、`WT_MAX_STREAM_DATA` を自動送信する
    pub fn grow_stream_recv_window(
        &mut self,
        stream_id: WtStreamId,
        increment: u64,
    ) -> WtResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;
        let new_max = stream.recv_max().saturating_add(increment);
        stream.update_recv_max(new_max);
        self.send_max_stream_data(stream_id, new_max)
    }

    /// ローカル側ストリーム上限を拡張し、`WT_MAX_STREAMS` を自動送信する
    pub fn grow_max_streams(&mut self, increment: u64, bidirectional: bool) -> WtResult<()> {
        self.flow_control
            .add_max_streams_local(increment, bidirectional);
        let new_max = if bidirectional {
            self.flow_control.max_streams_bidi_local()
        } else {
            self.flow_control.max_streams_uni_local()
        };
        self.send_max_streams(new_max, bidirectional)
    }

    /// セッション設定への参照を取得する
    #[must_use]
    pub const fn config(&self) -> &WtConfig {
        &self.config
    }

    /// セッションをドレインする
    pub fn drain(&mut self) -> WtResult<()> {
        if self.state != WtSessionState::Active {
            return Err(WtError::session_state_error(
                "cannot drain: session not active",
            ));
        }

        // WT_DRAIN_SESSION Capsule をエンコード
        let capsule = Capsule::WtDrainSession;
        self.capsule_encoder.encode(&capsule);
        self.output_buffer
            .extend_from_slice(&self.capsule_encoder.take());

        self.state = WtSessionState::Draining;

        Ok(())
    }

    /// Capsule を処理する
    fn handle_capsule(&mut self, capsule: Capsule) -> WtResult<()> {
        match capsule {
            Capsule::Datagram { data } => {
                self.events.push_back(WtEvent::DatagramReceived { data });
            }
            Capsule::WtStream {
                stream_id,
                data,
                fin,
            } => {
                self.handle_stream_data(stream_id, data, fin)?;
            }
            Capsule::WtResetStream {
                stream_id,
                error_code,
                reliable_size,
            } => {
                if let Some(stream) = self.streams.get_mut(&stream_id) {
                    // draft-ietf-webtrans-http2-14 Section 6.2:
                    // クローズ済みまたはリセット済みのストリームへの WT_RESET_STREAM は
                    // WEBTRANSPORT_STREAM_STATE_ERROR
                    if !stream.can_recv() {
                        return Err(WtError::stream_state_error(
                            "WT_RESET_STREAM received for stream not in valid state",
                        ));
                    }
                    // draft-ietf-webtrans-http2-14 Section 6.2: Reliable Size 検証
                    // reliable_size が既に受信したオフセットより小さい場合はセッションエラー
                    if reliable_size < stream.recv_offset() {
                        return Err(WtError::stream_state_error(format!(
                            "WT_RESET_STREAM reliable_size {} is less than recv_offset {}",
                            reliable_size,
                            stream.recv_offset()
                        )));
                    }
                    stream.recv_reset();
                }
                self.events.push_back(WtEvent::StreamReset {
                    stream_id,
                    error_code,
                });
            }
            Capsule::WtStopSending {
                stream_id,
                error_code,
            } => {
                if let Some(stream) = self.streams.get_mut(&stream_id) {
                    // draft-ietf-webtrans-http2-14 Section 6.3:
                    // 2 回目の WT_STOP_SENDING は WEBTRANSPORT_STREAM_STATE_ERROR
                    if stream.stop_sending_received() {
                        return Err(WtError::stream_state_error(
                            "duplicate WT_STOP_SENDING received",
                        ));
                    }
                    stream.set_stop_sending_received();
                }

                self.events.push_back(WtEvent::StopSending {
                    stream_id,
                    error_code,
                });
            }
            Capsule::WtMaxData { maximum } => {
                self.flow_control.update_send_max(maximum)?;
            }
            Capsule::WtMaxStreamData { stream_id, maximum } => {
                if let Some(stream) = self.streams.get_mut(&stream_id) {
                    // draft-ietf-webtrans-http2-14 Section 6.6:
                    // WT_STOP_SENDING を送信した後の WT_MAX_STREAM_DATA は
                    // WEBTRANSPORT_STREAM_STATE_ERROR
                    if stream.stop_sending_sent() {
                        return Err(WtError::stream_state_error(
                            "WT_MAX_STREAM_DATA received after WT_STOP_SENDING",
                        ));
                    }
                    stream.update_send_max(maximum)?;
                }
            }
            Capsule::WtMaxStreams {
                maximum,
                bidirectional,
            } => {
                self.flow_control
                    .update_max_streams(maximum, bidirectional)?;
            }
            Capsule::WtDataBlocked { maximum: _ } => {
                // ピアがブロックされていることを通知
                // 必要に応じて WT_MAX_DATA を送信する
            }
            Capsule::WtStreamDataBlocked {
                stream_id,
                maximum: _,
            } => {
                // draft-ietf-webtrans-http2-14 Section 6.9:
                // クローズ済みまたはリセット済みのストリームでは
                // WEBTRANSPORT_STREAM_STATE_ERROR
                if let Some(stream) = self.streams.get(&stream_id)
                    && !stream.can_recv()
                    && !stream.can_send()
                {
                    return Err(WtError::stream_state_error(
                        "WT_STREAM_DATA_BLOCKED received for stream not in valid state",
                    ));
                }
            }
            Capsule::WtStreamsBlocked {
                maximum: _,
                bidirectional: _,
            } => {
                // ピアがストリーム数制限でブロックされていることを通知
            }
            Capsule::WtCloseSession { error_code, reason } => {
                // Closed 状態は吸収状態: 既に Closed なら無視
                if self.state != WtSessionState::Closed {
                    self.state = WtSessionState::Closed;
                    self.events
                        .push_back(WtEvent::SessionClosed { error_code, reason });
                }
            }
            Capsule::WtDrainSession => {
                // Closed 状態は吸収状態: 既に Closed または Draining なら無視
                // (Draining -> Draining は冪等性を維持)
                if self.state == WtSessionState::Active {
                    self.state = WtSessionState::Draining;
                    self.events.push_back(WtEvent::SessionDraining);
                }
            }
            Capsule::Padding { .. } | Capsule::Unknown { .. } => {
                // 無視
            }
        }

        Ok(())
    }

    /// ストリームデータを処理する
    fn handle_stream_data(
        &mut self,
        stream_id: WtStreamId,
        data: Bytes,
        fin: bool,
    ) -> WtResult<()> {
        let is_new_stream = !self.streams.contains_key(&stream_id);

        // draft-ietf-webtrans-http2-14 Section 6.4: empty capsule チェック
        // 空の WT_STREAM capsule は以下の場合のみ許可:
        // - 新規ストリームの開始時
        // - FIN フラグが設定されている場合
        if data.is_empty() && !is_new_stream && !fin {
            // 既存ストリームで FIN なしの空データはエラー
            // ただし、has_received_data でストリームが「実際に」データを受信したかをチェック
            if self
                .streams
                .get(&stream_id)
                .is_some_and(|stream| stream.has_received_data())
            {
                return Err(WtError::stream_state_error(
                    "empty WT_STREAM capsule without FIN on existing stream",
                ));
            }
        }

        if is_new_stream {
            // draft-ietf-webtrans-http2-14 Section 5, RFC 9000 Section 2.1:
            // ストリーム ID の開始主体がピア側であることを検証する。
            // ローカル開始用 ID をピアが注入すると状態管理の一貫性が崩れる。
            let is_peer_initiated = match self.role {
                Role::Client => stream::stream_id::is_server_initiated(stream_id),
                Role::Server => stream::stream_id::is_client_initiated(stream_id),
            };
            if !is_peer_initiated {
                return Err(WtError::stream_state_error(
                    "received stream with locally-initiated stream ID",
                ));
            }

            // RFC 9000 Section 4.6, draft-ietf-webtrans-http2-14 Section 5.2:
            // stream ID に基づいてストリーム上限を検証する。
            // 順序外の stream ID は下位 ID も全て開いた扱いになる。
            if !self.flow_control.can_accept_stream(stream_id) {
                return Err(WtError::flow_control_error("peer exceeded stream limit"));
            }

            let bidirectional = stream::stream_id::is_bidirectional(stream_id);
            // draft-ietf-webtrans-http2-14 Section 4.3.1:
            // REMOTE はピアが開始したストリーム向けの初期フロー制御上限
            let initial_max_data = if bidirectional {
                self.config.initial_max_stream_data_bidi_remote
            } else {
                self.config.initial_max_stream_data_uni
            };

            let stream = WtStream::new(stream_id, initial_max_data, bidirectional);
            self.streams.insert(stream_id, stream);

            self.events.push_back(WtEvent::StreamOpened {
                stream_id,
                bidirectional,
            });
        }

        // 直前の insert または contains_key チェックで存在が保証されている
        let stream = self
            .streams
            .get_mut(&stream_id)
            .expect("stream must exist after insert or lookup");

        // 受信状態を更新
        stream.recv_data(data.len() as u64, fin)?;

        // データを受信したことを記録 (empty capsule チェック用)
        if !data.is_empty() {
            stream.set_has_received_data();
        }

        // フロー制御を更新
        self.flow_control.consume_recv(data.len() as u64)?;

        self.events.push_back(WtEvent::StreamData {
            stream_id,
            data,
            fin,
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_session_creation() {
        let session = WtSession::client(WtConfig::default());
        assert_eq!(session.role(), Role::Client);
        assert_eq!(session.state(), WtSessionState::Initial);
    }

    #[test]
    fn test_server_session_creation() {
        let session = WtSession::server(WtConfig::default());
        assert_eq!(session.role(), Role::Server);
        assert_eq!(session.state(), WtSessionState::Initial);
    }

    #[test]
    fn test_session_initiate() {
        let mut session = WtSession::client(WtConfig::default());
        session.initiate().unwrap();
        assert_eq!(session.state(), WtSessionState::Active);
    }

    #[test]
    fn test_open_bidi_stream() {
        let mut session = WtSession::client(WtConfig::default());
        session.initiate().unwrap();

        let stream_id = session.open_bidi_stream().unwrap();
        assert!(stream::stream_id::is_client_initiated(stream_id));
        assert!(stream::stream_id::is_bidirectional(stream_id));
    }

    #[test]
    fn test_open_uni_stream() {
        let mut session = WtSession::client(WtConfig::default());
        session.initiate().unwrap();

        let stream_id = session.open_uni_stream().unwrap();
        assert!(stream::stream_id::is_client_initiated(stream_id));
        assert!(stream::stream_id::is_unidirectional(stream_id));
    }

    #[test]
    fn test_send_datagram() {
        let mut session = WtSession::client(WtConfig::default());
        session.initiate().unwrap();

        session.send_datagram(Bytes::from_static(b"hello")).unwrap();
        assert!(session.has_output());
    }

    #[test]
    fn test_close_session() {
        let mut session = WtSession::client(WtConfig::default());
        session.initiate().unwrap();

        session.close(0, "normal close").unwrap();
        assert_eq!(session.state(), WtSessionState::Closed);
    }

    #[test]
    fn test_drain_session() {
        let mut session = WtSession::client(WtConfig::default());
        session.initiate().unwrap();

        session.drain().unwrap();
        assert_eq!(session.state(), WtSessionState::Draining);
    }
}
