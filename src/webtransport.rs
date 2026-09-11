//! WebTransport over HTTP/2 (draft-ietf-webtrans-http2-15)
//!
//! # 概要
//!
//! WebTransport セッションは HTTP/2 Extended CONNECT ストリーム上で動作し、
//! Capsule Protocol でデータを多重化する。
//!
//! # 注意
//!
//! 本モジュールは draft-ietf-webtrans-http2-15 に基づく実装であり、
//! draft の改訂や RFC 化に伴い仕様が変更される可能性がある。
//!
//! # 参照仕様
//!
//! - draft-ietf-webtrans-http2-15 (WebTransport over HTTP/2)
//! - RFC 9297 (HTTP Datagrams and the Capsule Protocol)
//! - RFC 8441 (Bootstrapping WebSockets with HTTP/2 - Extended CONNECT)
//! - RFC 9000 Section 2.1 (Stream Types and Identifiers)
//! - RFC 9000 Section 3 (Stream States)
//! - RFC 9000 Section 16 (Variable-Length Integer Encoding)

pub mod capsule;
pub mod error;
pub mod exporter;
pub mod flow_control;
pub mod init;
pub mod protocols;
pub mod stream;
pub mod varint;

use std::collections::{HashMap, VecDeque};

use crate::bounded_set::BoundedSet;
use crate::connection::Role;
use crate::webtransport::capsule::{MAX_APPLICATION_ERROR_CODE, MAX_CLOSE_REASON_LEN};

/// RFC 9110 Section 5.6.2: tchar の定義
///
/// `"!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA`
pub(crate) fn is_tchar(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
    ) || b.is_ascii_alphanumeric()
}

pub use capsule::{Capsule, CapsuleDecoder, CapsuleEncoder, capsule_type};
pub use error::{WtError, WtErrorKind, WtResult};
pub use exporter::serialize_exporter_context;
pub use flow_control::WtFlowControl;
pub use init::WtInit;
pub use protocols::{WtAvailableProtocols, serialize_wt_protocol};
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
        data: Vec<u8>,
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
    DatagramReceived { data: Vec<u8> },
    /// セッションがドレイン中
    SessionDraining,
    /// セッションがクローズされた
    SessionClosed { error_code: u32, reason: String },
}

/// WebTransport 設定
///
/// [`Default`] はローカル広告値の既定値である。ピア用 config をピアの広告値から
/// 構築する場合は [`WtConfig::peer_default`] から開始し、未広告項目がローカル既定値に
/// ならないようにする。
#[derive(Debug, Clone)]
pub struct WtConfig {
    /// セッションレベルの初期最大データ量
    pub initial_max_data: u64,
    /// 双方向ストリームの初期最大データ量 (自身が開始したストリーム)
    ///
    /// draft-ietf-webtrans-http2-15 Section 4.3.1:
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL に対応する。
    pub initial_max_stream_data_bidi_local: u64,
    /// 双方向ストリームの初期最大データ量 (ピアが開始したストリーム)
    ///
    /// draft-ietf-webtrans-http2-15 Section 4.3.1:
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE に対応する。
    pub initial_max_stream_data_bidi_remote: u64,
    /// 単方向ストリームの初期最大データ量
    pub initial_max_stream_data_uni: u64,
    /// 双方向ストリームの初期最大数
    pub initial_max_streams_bidi: u64,
    /// 単方向ストリームの初期最大数
    pub initial_max_streams_uni: u64,
}

/// ローカル広告値の既定値を返す
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

impl WtConfig {
    /// **ピア用 `WtConfig`** を仕様の Initial Value (全て 0) で生成する
    ///
    /// draft-ietf-webtrans-http2-15 Section 11.2: `SETTINGS_WT_INITIAL_MAX_*` を
    /// 広告しないピアの初期値は 0 であり、[`Self::default`] のローカル広告用の
    /// 既定値をピアへ適用してはならない。ピアの広告値は [`Self::overlay_settings`] と
    /// [`Self::apply_init_as_peer`] で反映する。
    #[must_use]
    pub const fn peer_default() -> Self {
        Self {
            initial_max_data: 0,
            initial_max_stream_data_bidi_local: 0,
            initial_max_stream_data_bidi_remote: 0,
            initial_max_stream_data_uni: 0,
            initial_max_streams_bidi: 0,
            initial_max_streams_uni: 0,
        }
    }

    /// **ピア用 `WtConfig`** に Init ヘッダー由来の値を max マージする
    ///
    /// WebTransport-Init は送信者 (クライアント) が自分の受信上限を伝えるヘッダーであり、
    /// 受信側はこれをピア用 config にマージする。`u`/`bl`/`br` のマッピングは:
    /// - `u` → `initial_max_stream_data_uni`
    /// - `bl` → `initial_max_stream_data_bidi_local` (ピア自身が開始するストリームの送信上限)
    /// - `br` → `initial_max_stream_data_bidi_remote` (ピア視点で remote = ローカル開始の送信上限)
    ///
    /// draft-ietf-webtrans-http2-15 Section 4.3 (L524-L528) の MUST 規則に従い、
    /// SETTINGS 値と Init 値の大きい方を採用する。
    pub fn apply_init_as_peer(&mut self, init: &WtInit) {
        if let Some(u) = init.u {
            self.initial_max_stream_data_uni = self.initial_max_stream_data_uni.max(u);
        }
        if let Some(bl) = init.bl {
            self.initial_max_stream_data_bidi_local =
                self.initial_max_stream_data_bidi_local.max(bl);
        }
        if let Some(br) = init.br {
            self.initial_max_stream_data_bidi_remote =
                self.initial_max_stream_data_bidi_remote.max(br);
        }
    }

    /// HTTP/2 SETTINGS 由来の初期フロー制御値を上書き適用する
    ///
    /// draft-ietf-webtrans-http2-15 Section 4.3.1:
    /// セッション確立時は ACK 済み SETTINGS の初期値を使う。
    /// `Some` のパラメータのみ上書きし、`None` (未広告) は既存値を維持する。
    /// Init ヘッダーとの max マージは [`Self::apply_init_as_peer`] で別途行う。
    pub fn overlay_settings(&mut self, settings: &crate::settings::Settings) {
        if let Some(v) = settings.wt_initial_max_data() {
            self.initial_max_data = u64::from(v);
        }
        if let Some(v) = settings.wt_initial_max_stream_data_uni() {
            self.initial_max_stream_data_uni = u64::from(v);
        }
        if let Some(v) = settings.wt_initial_max_stream_data_bidi_local() {
            self.initial_max_stream_data_bidi_local = u64::from(v);
        }
        if let Some(v) = settings.wt_initial_max_stream_data_bidi_remote() {
            self.initial_max_stream_data_bidi_remote = u64::from(v);
        }
        if let Some(v) = settings.wt_initial_max_streams_bidi() {
            self.initial_max_streams_bidi = u64::from(v);
        }
        if let Some(v) = settings.wt_initial_max_streams_uni() {
            self.initial_max_streams_uni = u64::from(v);
        }
    }
}

/// クローズ済み WebTransport ストリーム ID 集合の上限
///
/// 上限を超えた場合は最も小さい ID を追い出す。追い出された ID への
/// WT_STREAM は再作成を許す既知の制限がある。
const CLOSED_STREAMS_MAX_SIZE: usize = 10000;

/// WebTransport セッション (Sans I/O)
///
/// HTTP/2 CONNECT ストリーム上で動作する WebTransport セッションを管理する。
#[derive(Debug)]
pub struct WtSession {
    /// 接続の役割
    role: Role,
    /// ローカル設定 (ローカルが広告したフロー制御値)
    config: WtConfig,
    /// ピア設定 (ピアが広告したフロー制御値)
    peer_config: WtConfig,
    /// セッション状態
    state: WtSessionState,
    /// ストリーム一覧
    streams: HashMap<WtStreamId, WtStream>,
    /// クローズ済みストリーム ID の集合
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.4: クローズ済みストリームへの
    /// WT_STREAM 受信を拒否するために使用する。
    closed_streams: BoundedSet<WtStreamId>,
    /// フロー制御
    flow_control: WtFlowControl,
    /// Capsule デコーダー
    capsule_decoder: CapsuleDecoder,
    /// Capsule エンコーダー
    capsule_encoder: CapsuleEncoder,
    /// 出力バッファ
    output_buffer: VecDeque<u8>,
    /// イベントキュー
    events: VecDeque<WtEvent>,
    /// 次の双方向ストリーム ID
    next_bidi_stream_id: WtStreamId,
    /// 次の単方向ストリーム ID
    next_uni_stream_id: WtStreamId,
}

impl WtSession {
    /// クライアントセッションを生成する
    ///
    /// `config` はローカルが広告するフロー制御値、`peer_config` はピアが広告したフロー制御値。
    #[must_use]
    pub fn client(config: WtConfig, peer_config: WtConfig) -> Self {
        Self::new(Role::Client, config, peer_config)
    }

    /// サーバーセッションを生成する
    ///
    /// `config` はローカルが広告するフロー制御値、`peer_config` はピアが広告したフロー制御値。
    #[must_use]
    pub fn server(config: WtConfig, peer_config: WtConfig) -> Self {
        Self::new(Role::Server, config, peer_config)
    }

    /// 新しいセッションを生成する
    fn new(role: Role, config: WtConfig, peer_config: WtConfig) -> Self {
        let is_client = role == Role::Client;
        // send_max にはピアが広告した値、recv_max にはローカルが広告した値を使う
        // (draft-ietf-webtrans-http2-15 Section 4.3.1)
        let flow_control = WtFlowControl::new(
            peer_config.initial_max_data,
            config.initial_max_data,
            config.initial_max_streams_bidi,
            peer_config.initial_max_streams_bidi,
            config.initial_max_streams_uni,
            peer_config.initial_max_streams_uni,
        );

        Self {
            role,
            config,
            peer_config,
            state: WtSessionState::Initial,
            streams: HashMap::new(),
            closed_streams: BoundedSet::new(CLOSED_STREAMS_MAX_SIZE),
            flow_control,
            capsule_decoder: CapsuleDecoder::new(),
            capsule_encoder: CapsuleEncoder::new(),
            output_buffer: VecDeque::new(),
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
        self.capsule_decoder.feed(data)?;
        Ok(data.len())
    }

    /// Capsule を処理してイベントを生成する
    ///
    /// セッションが `WtSessionState::Closed` の場合は受信 capsule を無視して
    /// `Ok(())` を返す (draft-ietf-webtrans-http2-15 Section 6.12)。
    pub fn process(&mut self) -> WtResult<()> {
        while let Some(capsule) = self.capsule_decoder.decode()? {
            self.handle_capsule(capsule)?;
        }
        Ok(())
    }

    /// 送信データを取得する
    #[must_use]
    pub fn poll_output(&mut self) -> Option<Vec<u8>> {
        if self.output_buffer.is_empty() {
            None
        } else {
            Some(self.output_buffer.drain(..).collect())
        }
    }

    /// イベントを取得する
    ///
    /// `StreamData { fin: true }` を pop した際に、対象ストリームの受信状態を
    /// DataRecvd → DataRead へ遷移させ、完全に閉じていればストリームを削除する。
    pub fn poll_event(&mut self) -> Option<WtEvent> {
        let event = self.events.pop_front()?;

        // draft-ietf-webtrans-http2-15 Section 5.2: FIN 付きデータを読み取った時点で
        // DataRecvd → DataRead へ遷移し、閉じたストリームを削除する
        if let WtEvent::StreamData {
            stream_id,
            fin: true,
            ..
        } = &event
        {
            if let Some(stream) = self.streams.get_mut(stream_id) {
                stream.mark_data_read();
            }
            self.remove_if_closed(*stream_id);
        }

        Some(event)
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
        // draft-ietf-webtrans-http2-15 Section 6.13: Draining 状態でも新規ストリーム開設を許可
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

        // draft-ietf-webtrans-http2-15 Section 11.2:
        // ローカル開始ストリームの send_max はピアの BIDI_REMOTE / UNI (ピア視点で remote = ローカル開始)
        // recv_max はローカルの BIDI_LOCAL / UNI (ローカルが開始したストリームの受信上限)
        let (send_max, recv_max) = if bidirectional {
            (
                self.peer_config.initial_max_stream_data_bidi_remote,
                self.config.initial_max_stream_data_bidi_local,
            )
        } else {
            (
                self.peer_config.initial_max_stream_data_uni,
                self.config.initial_max_stream_data_uni,
            )
        };

        let stream = WtStream::new(stream_id, send_max, recv_max, bidirectional, true);
        self.streams.insert(stream_id, stream);

        // ストリーム数を更新
        self.flow_control.opened_stream(bidirectional);

        Ok(stream_id)
    }

    /// ストリームにデータを送信する
    pub fn send_stream_data(
        &mut self,
        stream_id: WtStreamId,
        data: &[u8],
        fin: bool,
    ) -> WtResult<()> {
        // draft-ietf-webtrans-http2-15 Section 6.13: Draining 状態でもデータ送信を許可
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

        // エンコードより前にストリーム / セッション両方の送信上限を検査し、拒否時に
        // 部分的な状態変更を残さない。driver はストリームエラー後もセッションを継続する
        // ため、片方だけ検査して状態を進めると送信済みバイト数が実際の送信量とずれ、
        // WT_RESET_STREAM の Reliable Size 不一致や送信終端状態の誤遷移を招く
        // (draft-ietf-webtrans-http2-15 Section 6.2 / Section 6.5 / Section 6.6)
        let size = data.len() as u64;
        if size > self.flow_control.send_available() {
            return Err(WtError::flow_control_error("send window exhausted"));
        }
        stream.send_data(size, fin)?;
        self.flow_control.consume_send(size)?;

        // WT_STREAM Capsule をエンコード
        let capsule = Capsule::WtStream {
            stream_id,
            data: data.to_vec(),
            fin,
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer.extend(self.capsule_encoder.take());

        // 送信専用 uni ストリームは FIN 送信で即座に閉じる
        if fin {
            self.remove_if_closed(stream_id);
        }

        Ok(())
    }

    /// ストリームをリセットする
    ///
    /// `error_code` が 0xffffffff を超える場合は `flow_control_error` を返す
    /// (draft-ietf-webtrans-http2-15 Section 6.2 の MUST NOT)。
    pub fn reset_stream(&mut self, stream_id: WtStreamId, error_code: u64) -> WtResult<()> {
        // draft-ietf-webtrans-http2-15 Section 6.2:
        // Application Protocol Error Code は 0xffffffff 以下でなければならない (MUST NOT)。
        // 超過時は仕様違反の capsule を送信することになり、varint 上限 (2^62-1) を超える
        // 値は CapsuleEncoder 内で panic するため、事前に拒否する。
        // 受信側 (capsule.rs の decode_payload) は仕様どおり WT_ERROR 相当の
        // session_state_error で処理するが、送信 API の引数検証はローカルな入力エラーであり、
        // 他の範囲チェック (send_max_streams の 2^60 上限等) と同様に flow_control_error で
        // 統一する。
        if error_code > MAX_APPLICATION_ERROR_CODE {
            return Err(WtError::flow_control_error(format!(
                "WT_RESET_STREAM error code {error_code:#x} exceeds maximum {MAX_APPLICATION_ERROR_CODE:#x}"
            )));
        }

        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;

        // draft-ietf-webtrans-http2-15 Section 6.2:
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
        self.output_buffer.extend(self.capsule_encoder.take());

        // 送信状態を更新
        stream.send_reset();

        // 送信専用 uni ストリームはリセット送信で即座に閉じる
        self.remove_if_closed(stream_id);

        Ok(())
    }

    /// ストリーム送信停止を要求する
    ///
    /// `error_code` が 0xffffffff を超える場合は `flow_control_error` を返す
    /// (draft-ietf-webtrans-http2-15 Section 6.3 の MUST NOT)。
    pub fn stop_sending(&mut self, stream_id: WtStreamId, error_code: u64) -> WtResult<()> {
        // draft-ietf-webtrans-http2-15 Section 6.3:
        // Application Protocol Error Code は 0xffffffff 以下でなければならない (MUST NOT)。
        // 超過時は仕様違反の capsule を送信することになり、varint 上限 (2^62-1) を超える
        // 値は CapsuleEncoder 内で panic するため、事前に拒否する。
        // 受信側 (capsule.rs の decode_payload) は仕様どおり WT_ERROR 相当の
        // session_state_error で処理するが、送信 API の引数検証はローカルな入力エラーであり、
        // 他の範囲チェック (send_max_streams の 2^60 上限等) と同様に flow_control_error で
        // 統一する。
        if error_code > MAX_APPLICATION_ERROR_CODE {
            return Err(WtError::flow_control_error(format!(
                "WT_STOP_SENDING error code {error_code:#x} exceeds maximum {MAX_APPLICATION_ERROR_CODE:#x}"
            )));
        }

        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;

        // draft-ietf-webtrans-http2-15 Section 6.3:
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
        self.output_buffer.extend(self.capsule_encoder.take());

        // 送信済みフラグを設定
        stream.set_stop_sending_sent();

        Ok(())
    }

    /// データグラムを送信する
    pub fn send_datagram(&mut self, data: &[u8]) -> WtResult<()> {
        // draft-ietf-webtrans-http2-15 Section 6.13: Draining 状態でもデータグラム送信を許可
        if !matches!(
            self.state,
            WtSessionState::Active | WtSessionState::Draining
        ) {
            return Err(WtError::session_state_error(
                "cannot send datagram: session not active or draining",
            ));
        }

        // DATAGRAM Capsule をエンコード
        let capsule = Capsule::Datagram {
            data: data.to_vec(),
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer.extend(self.capsule_encoder.take());

        Ok(())
    }

    /// セッションを終了する
    pub fn close(&mut self, error_code: u32, reason: &str) -> WtResult<()> {
        if self.state == WtSessionState::Closed {
            return Err(WtError::session_state_error("session already closed"));
        }

        // draft-ietf-webtrans-http2-15 Section 6.12:
        // reason の長さは MUST NOT exceed 1024 bytes。
        // 超過時は UTF-8 文字境界で 1024 バイト以下に切り詰めて送る
        // (呼び出し側の利便性優先。draft-15 でも切り詰めは義務ではないが許容される)。
        let reason = if reason.len() > MAX_CLOSE_REASON_LEN {
            let bytes = reason.as_bytes();
            let mut end = MAX_CLOSE_REASON_LEN;
            // UTF-8 continuation byte (0b10xxxxxx) でない位置まで後退
            while end > 0 && bytes[end] & 0b1100_0000 == 0b1000_0000 {
                end -= 1;
            }
            // 安全: end は char 境界を指している
            &reason[..end]
        } else {
            reason
        };

        // WT_CLOSE_SESSION Capsule をエンコード
        let capsule = Capsule::WtCloseSession {
            error_code,
            reason: reason.to_string(),
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer.extend(self.capsule_encoder.take());

        self.state = WtSessionState::Closed;

        Ok(())
    }

    /// セッションレベルのフロー制御上限を増やす `WT_MAX_DATA` を送信する
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.5: 受信側が受信可能なバイト数を通知する。
    /// 単調増加でなければならない (現在値より小さい値を指定すると `flow_control_error`)。
    ///
    /// `maximum` が varint 上限 (2^62-1) を超える場合は `flow_control_error` を返す
    /// (RFC 9000 Section 16)。
    pub fn send_max_data(&mut self, maximum: u64) -> WtResult<()> {
        // RFC 9000 Section 16: Maximum は varint でエンコードされるため、上限 (2^62-1) を
        // 超える値は CapsuleEncoder 内で panic する。事前に拒否する。
        if maximum > MAX_VALUE {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_DATA value {maximum} exceeds varint maximum {MAX_VALUE}"
            )));
        }
        let capsule = Capsule::WtMaxData { maximum };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer.extend(self.capsule_encoder.take());
        Ok(())
    }

    /// ストリームレベルのフロー制御上限を増やす `WT_MAX_STREAM_DATA` を送信する
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.6: 指定ストリームの受信可能バイト数を通知する。
    ///
    /// 同一ストリームに対して既に `WT_STOP_SENDING` を送信済みの場合は
    /// `stream_state_error` を返す (Section 6.6 の MUST)。`maximum` が varint 上限
    /// (2^62-1) を超える場合は `flow_control_error` を返す (RFC 9000 Section 16)。
    pub fn send_max_stream_data(&mut self, stream_id: WtStreamId, maximum: u64) -> WtResult<()> {
        // RFC 9000 Section 16: Maximum は varint でエンコードされるため、上限 (2^62-1) を
        // 超える値は CapsuleEncoder 内で panic する。事前に拒否する。
        if maximum > MAX_VALUE {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_STREAM_DATA value {maximum} exceeds varint maximum {MAX_VALUE}"
            )));
        }

        let stream = self
            .streams
            .get(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;
        // draft-ietf-webtrans-http2-15 Section 6.6:
        // WT_STOP_SENDING 送信後に WT_MAX_STREAM_DATA を送ってはならない (MUST NOT)
        if stream.stop_sending_sent() {
            return Err(WtError::stream_state_error(
                "cannot send WT_MAX_STREAM_DATA: WT_STOP_SENDING already sent",
            ));
        }

        let capsule = Capsule::WtMaxStreamData { stream_id, maximum };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer.extend(self.capsule_encoder.take());
        Ok(())
    }

    /// ストリーム数上限を増やす `WT_MAX_STREAMS` を送信する
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.7: ピアが新規ストリームを開ける上限を通知する。
    /// 2^60 を超える値は送信できない。
    pub fn send_max_streams(&mut self, maximum: u64, bidirectional: bool) -> WtResult<()> {
        // draft-ietf-webtrans-http2-15 Section 6.7: 2^60 超過値の送信は不可
        if maximum > (1u64 << 60) {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_STREAMS value {} exceeds 2^60 limit",
                maximum
            )));
        }
        let capsule = Capsule::WtMaxStreams {
            maximum,
            bidirectional,
        };
        self.capsule_encoder.encode(&capsule);
        self.output_buffer.extend(self.capsule_encoder.take());
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
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.6:
    /// `WT_STOP_SENDING` 送信済みのストリームでは `stream_state_error` を返す。
    pub fn grow_stream_recv_window(
        &mut self,
        stream_id: WtStreamId,
        increment: u64,
    ) -> WtResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;
        // draft-ietf-webtrans-http2-15 Section 6.6:
        // WT_STOP_SENDING 送信後に WT_MAX_STREAM_DATA を送ってはならない (MUST NOT)
        // recv_max を更新する前に拒否し、ローカル状態の不整合を避ける
        if stream.stop_sending_sent() {
            return Err(WtError::stream_state_error(
                "cannot grow stream recv window: WT_STOP_SENDING already sent",
            ));
        }
        let new_max = stream.recv_max().saturating_add(increment);
        stream.update_recv_max(new_max)?;
        self.send_max_stream_data(stream_id, new_max)
    }

    /// ローカル側ストリーム上限を拡張し、`WT_MAX_STREAMS` を自動送信する
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.7: 2^60 を超える値は送信できない。
    pub fn grow_max_streams(&mut self, increment: u64, bidirectional: bool) -> WtResult<()> {
        self.flow_control
            .add_max_streams_local(increment, bidirectional);
        let new_max = if bidirectional {
            self.flow_control.max_streams_bidi_local()
        } else {
            self.flow_control.max_streams_uni_local()
        };
        // saturating_add で 2^60 を超えた場合にエラーを返す
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
        self.output_buffer.extend(self.capsule_encoder.take());

        self.state = WtSessionState::Draining;

        Ok(())
    }

    /// Capsule を処理する
    fn handle_capsule(&mut self, capsule: Capsule) -> WtResult<()> {
        // draft-ietf-webtrans-http2-15 Section 6.12: WT_CLOSE_SESSION 受信後は
        // END_STREAM 応答でストリームを閉じることを MUST とする。H2 draft は
        // Closed 後の capsule 処理を規定しないが、ここで Section 6.4 のストリーム
        // エラーを返すと `process` が Err になり、ドライバが END_STREAM 応答に
        // 到達できなくなるため、吸収状態として無視する。
        if self.state == WtSessionState::Closed {
            return Ok(());
        }
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
                // draft-ietf-webtrans-http2-15 Section 6.2 (L876-L882):
                // 存在しないストリームへの WT_RESET_STREAM は MUST でエラー。
                let stream = self.streams.get_mut(&stream_id).ok_or_else(|| {
                    WtError::stream_state_error(format!(
                        "WT_RESET_STREAM received for unknown stream {stream_id}"
                    ))
                })?;

                // draft-ietf-webtrans-http2-15 Section 6.2:
                // クローズ済みまたはリセット済みのストリームへの WT_RESET_STREAM は
                // WT_STREAM_STATE_ERROR
                if !stream.can_recv() {
                    return Err(WtError::stream_state_error(
                        "WT_RESET_STREAM received for stream not in valid state",
                    ));
                }
                // draft-ietf-webtrans-http2-15 Section 6.2: Reliable Size 検証
                // HTTP/2 上は順序保証があるため Reliable Size は送信済み総量と
                // 一致しなければならない (MUST equal)。過小は既達データと矛盾、
                // 過大は後続バイトを約束するが到着し得ない → いずれもセッションエラー
                if reliable_size != stream.recv_offset() {
                    return Err(WtError::stream_state_error(format!(
                        "WT_RESET_STREAM reliable_size {} does not match recv_offset {}",
                        reliable_size,
                        stream.recv_offset()
                    )));
                }
                stream.recv_reset();

                self.events.push_back(WtEvent::StreamReset {
                    stream_id,
                    error_code,
                });

                // リセット受信で閉じたストリームを削除する
                self.remove_if_closed(stream_id);
            }
            Capsule::WtStopSending {
                stream_id,
                error_code,
            } => {
                // 借用回避: 可変借用ブロックを抜けてから reset_stream を呼ぶ
                let should_reset = self.streams.get(&stream_id).is_some_and(|s| s.can_send());

                if let Some(stream) = self.streams.get_mut(&stream_id) {
                    // draft-ietf-webtrans-http2-15 Section 6.3:
                    // 2 回目の WT_STOP_SENDING は WT_STREAM_STATE_ERROR
                    if stream.stop_sending_received() {
                        return Err(WtError::stream_state_error(
                            "duplicate WT_STOP_SENDING received",
                        ));
                    }
                    stream.set_stop_sending_received();
                }

                // draft-ietf-webtrans-http2-15 Section 6.3 + RFC 9000 Section 3.5:
                // Ready または Send 状態のストリームには WT_RESET_STREAM を MUST 応答する。
                // error_code のコピーは RFC 9000 Section 3.5 の SHOULD に従う。
                if should_reset {
                    let _ = self.reset_stream(stream_id, error_code);
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
                    // draft-ietf-webtrans-http2-15 Section 6.6:
                    // WT_STOP_SENDING を送信した後の WT_MAX_STREAM_DATA は
                    // WT_STREAM_STATE_ERROR
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
                // draft-ietf-webtrans-http2-15 Section 6.9:
                // 存在しないストリーム、または受信側がデータ受信不能な状態の
                // ストリームへの WT_STREAM_DATA_BLOCKED は WT_STREAM_STATE_ERROR
                let stream = self.streams.get(&stream_id).ok_or_else(|| {
                    WtError::stream_state_error(
                        "WT_STREAM_DATA_BLOCKED received for unknown stream",
                    )
                })?;
                if !stream.can_recv() {
                    return Err(WtError::stream_state_error(
                        "WT_STREAM_DATA_BLOCKED received for stream not in valid state",
                    ));
                }
            }
            Capsule::WtStreamsBlocked {
                maximum,
                bidirectional: _,
            } => {
                // draft-ietf-webtrans-http2-15 Section 6.10:
                // Maximum Streams が 2^60 を超える場合は WT_FLOW_CONTROL_ERROR
                if maximum > (1u64 << 60) {
                    return Err(WtError::flow_control_error(format!(
                        "WT_STREAMS_BLOCKED maximum {} exceeds 2^60 limit",
                        maximum
                    )));
                }
                // ピアがストリーム数制限でブロックされていることを通知
            }
            Capsule::WtCloseSession { error_code, reason } => {
                // 冒頭のガードで Closed は除外済みのため、ここでは必ず遷移する
                self.state = WtSessionState::Closed;
                self.events
                    .push_back(WtEvent::SessionClosed { error_code, reason });
            }
            Capsule::WtDrainSession => {
                // Draining -> Draining は冪等性を維持する (Closed は冒頭ガードで除外済み)
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
        data: Vec<u8>,
        fin: bool,
    ) -> WtResult<()> {
        // draft-ietf-webtrans-http2-15 Section 6.4: クローズ済みまたはリセット済みの
        // ストリームへの WT_STREAM は WT_STREAM_STATE_ERROR のストリームエラーとする。
        // 記録から追い出された ID は対象外 (既知の制限)。
        if self.closed_streams.contains(&stream_id) {
            return Err(WtError::stream_state_error(format!(
                "WT_STREAM received for closed stream {stream_id}"
            )));
        }

        let is_new_stream = !self.streams.contains_key(&stream_id);

        // draft-ietf-webtrans-http2-15 Section 5.2, RFC 9000 Section 2.1:
        // ストリーム ID の開始主体と方向を判定する。
        let is_peer_initiated = match self.role {
            Role::Client => stream::stream_id::is_server_initiated(stream_id),
            Role::Server => stream::stream_id::is_client_initiated(stream_id),
        };
        let bidirectional = stream::stream_id::is_bidirectional(stream_id);

        // draft-ietf-webtrans-http2-15 Section 6.4: empty capsule チェック
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
            // draft-ietf-webtrans-http2-15 Section 5.2, RFC 9000 Section 2.1:
            // ストリーム ID の開始主体がピア側であることを検証する。
            // ローカル開始用 ID をピアが注入すると状態管理の一貫性が崩れる。
            if !is_peer_initiated {
                return Err(WtError::stream_state_error(
                    "received stream with locally-initiated stream ID",
                ));
            }

            // RFC 9000 Section 4.6, draft-ietf-webtrans-http2-15 Section 6.7:
            // stream ID に基づいてストリーム上限を検証する。
            // 順序外の stream ID は下位 ID も全て開いた扱いになる (RFC 9000 Section 2.1)。
            if !self.flow_control.can_accept_stream(stream_id) {
                return Err(WtError::flow_control_error("peer exceeded stream limit"));
            }

            // draft-ietf-webtrans-http2-15 Section 11.2:
            // ピア開始ストリームの send_max はピアの BIDI_LOCAL / UNI (ピア視点で local = ピア開始)
            // recv_max はローカルの BIDI_REMOTE / UNI (ローカル視点で remote = ピア開始)
            let (send_max, recv_max) = if bidirectional {
                (
                    self.peer_config.initial_max_stream_data_bidi_local,
                    self.config.initial_max_stream_data_bidi_remote,
                )
            } else {
                (
                    self.peer_config.initial_max_stream_data_uni,
                    self.config.initial_max_stream_data_uni,
                )
            };

            let stream = WtStream::new(stream_id, send_max, recv_max, bidirectional, false);
            self.streams.insert(stream_id, stream);

            self.events.push_back(WtEvent::StreamOpened {
                stream_id,
                bidirectional,
            });
        } else if !bidirectional && !is_peer_initiated {
            // draft-ietf-webtrans-http2-15 Section 6.4 / RFC 9000 Section 2.1 / Section 19.8:
            // ローカル開始 uni ストリームは送信専用であり、ピアからの WT_STREAM は
            // 受信が許可されない状態への受信として拒否する。ローカル開始 bidi と
            // ピア開始ストリームは受信可能なため従来どおり受理する。
            return Err(WtError::stream_state_error(
                "WT_STREAM received for locally-initiated unidirectional stream",
            ));
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

    /// ストリームが完全に閉じていれば HashMap から削除する
    ///
    /// draft-ietf-webtrans-http2-15 Section 5.2: HTTP/2 の順序配送により
    /// ACK が不要なため、終端状態への遷移は即座に行われる。
    /// 閉じたストリームの状態オブジェクトを保持し続ける必要はない。
    /// 削除した ID は `closed_streams` に記録し、後続 WT_STREAM を拒否できるようにする
    /// (draft-ietf-webtrans-http2-15 Section 6.4)。
    fn remove_if_closed(&mut self, stream_id: WtStreamId) {
        if self.streams.get(&stream_id).is_some_and(|s| s.is_closed()) {
            self.streams.remove(&stream_id);
            self.closed_streams.insert(stream_id);
        }
    }
}
