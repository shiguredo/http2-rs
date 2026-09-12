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
    ///
    /// RFC 9000 Section 3.2 / draft-ietf-webtrans-http2-15 Section 6.7:
    /// ピアが上位 ID を先に開いた場合、同一型・同方向の下位 ID も開かれたものとして
    /// 通知される。したがって受信した capsule に現れていない ID についても
    /// 本イベントが送出されることがある。
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

/// WebTransport ストリーム ID を記録する上限付き集合の上限
///
/// `closed_streams` / `stop_sending_received_ids` で共用する。
/// 各集合は独立に上限を管理し、超えた場合は最も小さい ID を追い出す。追い出された ID は
/// 再作成・再処理を許す既知の制限がある。STOP_SENDING の重複・順序検証は
/// `stop_sending_received_ids` と `WtStream::stop_sending_received` の OR で判定するため、
/// `stop_sending_received_ids` から追い出された ID は、ストリームが生存していればフラグで
/// 検証できるが、削除済みであれば検証できない (`closed_streams` は判定に使わない)。
const STREAM_ID_RECORD_MAX_SIZE: usize = 10000;

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
    /// WT_STREAM 受信、およびピア開始 bidi の未知 ID への
    /// WT_STOP_SENDING / WT_MAX_STREAM_DATA 受信による再作成を拒否するために使用する。
    closed_streams: BoundedSet<WtStreamId>,
    /// WT_STOP_SENDING を受信したストリーム ID の集合
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.3 / Section 6.6: 2 回目の
    /// WT_STOP_SENDING の拒否と、WT_STOP_SENDING を受信した後の
    /// WT_MAX_STREAM_DATA の拒否に使用する。`WtStream::stop_sending_received` は
    /// ストリーム削除で失われるため、削除後も検証できるよう受理した時点で記録する。
    /// ピアが WT_STOP_SENDING を送らないセッションでは伸びない。
    stop_sending_received_ids: BoundedSet<WtStreamId>,
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
    ///
    /// ローカル開始 bidi ストリームの未作成範囲の下端 (採番済み範囲の上端)。
    /// `stream_id < next_bidi_stream_id` が作成済み、`stream_id >= next_bidi_stream_id`
    /// が未作成と一致する ([`Self::is_uncreated_local_id`] を参照)。
    next_bidi_stream_id: WtStreamId,
    /// 次の単方向ストリーム ID
    ///
    /// ローカル開始 uni ストリームの未作成範囲の下端。bidi と同じ不変条件を持つ。
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
            closed_streams: BoundedSet::new(STREAM_ID_RECORD_MAX_SIZE),
            stop_sending_received_ids: BoundedSet::new(STREAM_ID_RECORD_MAX_SIZE),
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

    /// ロールから見てピア開始のストリーム ID かどうかを返す
    #[must_use]
    const fn is_peer_initiated(&self, stream_id: WtStreamId) -> bool {
        match self.role {
            Role::Client => stream::stream_id::is_server_initiated(stream_id),
            Role::Server => stream::stream_id::is_client_initiated(stream_id),
        }
    }

    /// ロールと ID の下位ビットのみから、ローカルから見て受信専用
    /// (ピア開始 uni) のストリーム ID かどうかを返す
    ///
    /// `streams` の有無は参照しないため、未作成・削除済みの ID でも判定できる
    /// (RFC 9000 Section 2.1)。
    #[must_use]
    const fn is_receive_only_id(&self, stream_id: WtStreamId) -> bool {
        self.is_peer_initiated(stream_id) && stream::stream_id::is_unidirectional(stream_id)
    }

    /// ローカル開始 ID のうち、採番カウンタ (次に開く ID) 以上の
    /// 未作成のストリーム ID かどうかを返す
    ///
    /// `next_bidi_stream_id` / `next_uni_stream_id` は `WtSession::open_stream` でのみ
    /// 進み、その直後に `streams` へ挿入されるため、`stream_id` がカウンタ以上であれば
    /// 未作成と判定できる。`streams` の有無は参照しないため、削除済みの ID
    /// (採番済み範囲内) は未作成と判定されない (RFC 9000 Section 19.5 / Section 19.10)。
    #[must_use]
    const fn is_uncreated_local_id(&self, stream_id: WtStreamId) -> bool {
        if self.is_peer_initiated(stream_id) {
            return false;
        }
        if stream::stream_id::is_bidirectional(stream_id) {
            stream_id >= self.next_bidi_stream_id
        } else {
            stream_id >= self.next_uni_stream_id
        }
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
    ///
    /// 受信専用ストリーム (ピア開始 uni) への送信は `stream_state_error` を返す
    /// (draft-ietf-webtrans-http2-15 Section 6.4 / RFC 9000 Section 2.1)。
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

        // draft-ietf-webtrans-http2-15 Section 6.4 / RFC 9000 Section 2.1:
        // 単方向ストリームは開始側のみが送信できる。送信パートを持たない
        // 受信専用ストリーム (ピア開始 uni) への WT_STREAM 送信は WT_STREAM_STATE_ERROR
        if !stream.has_send_part() {
            return Err(WtError::stream_state_error(
                "cannot send on a receive-only stream",
            ));
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
    /// 受信専用ストリーム (ピア開始 uni) への WT_RESET_STREAM 送信は
    /// `stream_state_error` を返す (draft-ietf-webtrans-http2-15 Section 6.2 /
    /// RFC 9000 Section 19.4)。
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

        // draft-ietf-webtrans-http2-15 Section 6.2 / RFC 9000 Section 19.4:
        // 送信パートを持たない受信専用ストリーム (ピア開始 uni) は
        // WT_RESET_STREAM を送信できる側ではない
        if !stream.has_send_part() {
            return Err(WtError::stream_state_error(
                "cannot send WT_RESET_STREAM on a receive-only stream",
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
    /// 受信パートを持たない送信専用ストリーム (ローカル開始 uni) への
    /// WT_STOP_SENDING の送信、および同一ストリームへの 2 回目の送信は
    /// `stream_state_error` を返す。受信状態が `ResetRead` のストリームへの
    /// 送信も `stream_state_error` を返す
    /// (draft-ietf-webtrans-http2-15 Section 5.2 / Section 6.3 /
    /// RFC 9000 Section 3.3 / Section 19.5)。
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

        // draft-ietf-webtrans-http2-15 Section 5.2 / Section 6.3 /
        // RFC 9000 Section 3.3 / Section 19.5:
        // WT_STOP_SENDING は受信側の操作であり、受信パートを持たない
        // 送信専用ストリーム (ローカル開始 uni) へは送ることができない
        if !stream.has_recv_part() {
            return Err(WtError::stream_state_error(
                "cannot send WT_STOP_SENDING on a send-only stream",
            ));
        }

        // draft-ietf-webtrans-http2-15 Section 6.3:
        // WT_STOP_SENDING を複数回送信してはならない
        if stream.stop_sending_sent() {
            return Err(WtError::stream_state_error(
                "cannot send WT_STOP_SENDING: already sent",
            ));
        }

        // RFC 9000 Section 3.3 / Section 19.5:
        // STOP_SENDING を送れるのは RESET_STREAM を受け取っていない状態に限られ、
        // `ResetRead` のストリームへは送ることができない。Section 19.5 は "Recv" /
        // "Size Known" に限定し、Section 3.5 はその 2 状態での送信を SHOULD とするが
        // (RESET_STREAM 受信済みへの送信は SHOULD NOT)、本 API は Section 3.3 の MAY に
        // 従い `DataRecvd` / `DataRead` では受理する。RFC 9000 の "Reset Recvd" は
        // RESET_STREAM の受信とアプリへの通知を同時に行うため経由しない
        // (`WtStream::recv_reset` は `ResetRead` へ直接遷移する)
        if stream.recv_state() == RecvState::ResetRead {
            return Err(WtError::stream_state_error(
                "cannot send WT_STOP_SENDING on a reset stream",
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
    /// draft-ietf-webtrans-http2-15 Section 6.6: 指定ストリームの受信可能バイト数を通知し、
    /// 広告した上限を `WtStream::recv_max` にも反映する。`WtStream::recv_max` を
    /// 更新しないと、ピアが広告どおりに送ったデータを `WtStream::recv_data` が拒否する。
    ///
    /// 同一ストリームに対して既に `WT_STOP_SENDING` を送信済みの場合は
    /// `stream_state_error` を返す (Section 6.6 の MUST)。`maximum` が varint 上限
    /// (2^62-1) を超える場合、および現在の受信上限より小さい場合は
    /// `flow_control_error` を返す (RFC 9000 Section 16、減少を広告されたピアは
    /// Section 6.6 の MUST でセッションを閉じる)。
    /// 受信パートを持たない送信専用ストリーム (ローカル開始 uni) への
    /// WT_MAX_STREAM_DATA の送信、および受信状態が `Recv` でないストリームへの
    /// 送信は `stream_state_error` を返す
    /// (draft-ietf-webtrans-http2-15 Section 5.2 / Section 6.6 /
    /// RFC 9000 Section 3.3 / Section 19.10)。
    ///
    /// 減少の判定は `WtStream::recv_max` を基準にするため、`WtConfig` には
    /// SETTINGS で広告した初期値を設定しておくこと (未広告の項目の初期値は
    /// draft-ietf-webtrans-http2-15 Section 11.2 により 0 として扱われる)。
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
            .get_mut(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;

        // draft-ietf-webtrans-http2-15 Section 5.2 / Section 6.6 /
        // RFC 9000 Section 3.3 / Section 19.10:
        // WT_MAX_STREAM_DATA は受信側の操作であり、受信パートを持たない
        // 送信専用ストリーム (ローカル開始 uni) へは送ることができない
        if !stream.has_recv_part() {
            return Err(WtError::stream_state_error(
                "cannot send WT_MAX_STREAM_DATA on a send-only stream",
            ));
        }

        // draft-ietf-webtrans-http2-15 Section 6.6:
        // WT_STOP_SENDING 送信後に WT_MAX_STREAM_DATA を送ってはならない (MUST NOT)
        if stream.stop_sending_sent() {
            return Err(WtError::stream_state_error(
                "cannot send WT_MAX_STREAM_DATA: WT_STOP_SENDING already sent",
            ));
        }

        // RFC 9000 Section 3.3 / Section 19.10: 受信状態が `Recv` でなければ送れない
        Self::check_max_stream_data_recv_state(stream)?;

        // draft-ietf-webtrans-http2-15 Section 6.6:
        // 以前に広告した値より小さい Maximum Stream Data を受信したピアは
        // WT_FLOW_CONTROL_ERROR でセッションを閉じる (MUST) ため、減少は送信しない
        // (RFC 9000 Section 4.1 は減少の広告を許容し送信側の無視を MUST とするが、
        //  本 API は draft の MUST に従い送信前に拒否する。同値は draft が
        //  禁じていないため従来どおり送信する)
        if maximum < stream.recv_max() {
            return Err(WtError::flow_control_error(format!(
                "WT_MAX_STREAM_DATA value {maximum} is less than the advertised maximum {}",
                stream.recv_max()
            )));
        }
        // 広告した上限をローカルの受信上限にも反映する。反映しないと、ピアが
        // 広告どおりに送ったデータを `WtStream::recv_data` が拒否する
        stream.update_recv_max(maximum)?;

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
    /// `WtStream::recv_max` に `increment` を加えた値を広告する。更新と送信は
    /// `WtSession::send_max_stream_data` が行う。
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.6:
    /// `WT_STOP_SENDING` 送信済みのストリームでは `stream_state_error` を返す。
    /// 受信パートを持たない送信専用ストリーム (ローカル開始 uni) と、
    /// 受信状態が `Recv` でないストリームでは `stream_state_error` を返す
    /// (draft-ietf-webtrans-http2-15 Section 5.2 /
    /// Section 6.6 / RFC 9000 Section 3.3 / Section 19.10)。
    pub fn grow_stream_recv_window(
        &mut self,
        stream_id: WtStreamId,
        increment: u64,
    ) -> WtResult<()> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or_else(|| WtError::invalid_stream_id("stream not found"))?;
        // draft-ietf-webtrans-http2-15 Section 5.2 / Section 6.6 /
        // RFC 9000 Section 3.3 / Section 19.10:
        // 受信パートを持たないストリームの受信ウィンドウは拡張できない
        if !stream.has_recv_part() {
            return Err(WtError::stream_state_error(
                "cannot grow stream recv window on a send-only stream",
            ));
        }
        // draft-ietf-webtrans-http2-15 Section 6.6:
        // WT_STOP_SENDING 送信後に WT_MAX_STREAM_DATA を送ってはならない (MUST NOT)
        if stream.stop_sending_sent() {
            return Err(WtError::stream_state_error(
                "cannot grow stream recv window: WT_STOP_SENDING already sent",
            ));
        }
        // ここまでの検証はいずれも `recv_max` を更新する前に拒否し、
        // ローカル状態の不整合を避ける
        Self::check_max_stream_data_recv_state(stream)?;

        // `recv_max` の更新と WT_MAX_STREAM_DATA の送信は send_max_stream_data が行う
        let new_max = stream.recv_max().saturating_add(increment);
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
                // クローズ済みまたはリセット済みのストリーム、および受信パートを持たない
                // ローカル開始 uni ストリームへの WT_RESET_STREAM は WT_STREAM_STATE_ERROR
                // (RFC 9000 Section 19.4: 送信専用ストリームへの RESET_STREAM は不正)
                if !(stream.can_recv() && stream.has_recv_part()) {
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
                // draft-ietf-webtrans-http2-15 Section 5.2 / RFC 9000 Section 19.5:
                // 受信専用 ID (ピア開始 uni) への WT_STOP_SENDING は
                // WT_STREAM_STATE_ERROR。ストリームが未作成・削除済みでも
                // ID から受信専用と判定できる。送信側がいないため
                // WT_RESET_STREAM の自動応答も行わない
                if self.is_receive_only_id(stream_id) {
                    return Err(WtError::stream_state_error(
                        "WT_STOP_SENDING received for receive-only stream",
                    ));
                }

                // draft-ietf-webtrans-http2-15 Section 3.4 / Section 5.2 /
                // RFC 9000 Section 2.1 / Section 19.5:
                // 未作成のローカル開始 ID への WT_STOP_SENDING は WT_STREAM_STATE_ERROR
                // (QUIC の connection error を draft-ietf-webtrans-http2-15 Section 3.4 に
                // 従いストリームエラーで伝える)。削除済みの ID (採番済み範囲内) は
                // 作成済みのため対象外
                if self.is_uncreated_local_id(stream_id) {
                    return Err(WtError::stream_state_error(
                        "WT_STOP_SENDING received for locally-initiated stream that has not been created",
                    ));
                }

                // draft-ietf-webtrans-http2-15 Section 6.3:
                // 2 回目の WT_STOP_SENDING は WT_STREAM_STATE_ERROR。ストリーム生成より
                // 前に検証し、拒否時にストリームと WtEvent::StreamOpened を残さない
                if self.stop_sending_received(stream_id) {
                    return Err(WtError::stream_state_error(
                        "duplicate WT_STOP_SENDING received",
                    ));
                }

                // draft-ietf-webtrans-http2-15 Section 5.2 / RFC 9000 Section 3.2:
                // ピア開始 bidi の未知 ID への WT_STOP_SENDING はそのストリームを開く。
                // 生成後も以降の検証・自動応答は同じ経路を通る
                self.create_implicit_peer_bidi_stream(stream_id)?;

                // 借用回避: 可変借用ブロックを抜けてから reset_stream を呼ぶ
                let should_reset = self.streams.get(&stream_id).is_some_and(|s| s.can_send());

                if let Some(stream) = self.streams.get_mut(&stream_id) {
                    stream.set_stop_sending_received();
                }
                self.stop_sending_received_ids.insert(stream_id);

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
                // draft-ietf-webtrans-http2-15 Section 5.2 / RFC 9000 Section 19.10:
                // 受信専用 ID (ピア開始 uni) への WT_MAX_STREAM_DATA は
                // WT_STREAM_STATE_ERROR。ストリームが未作成・削除済みでも
                // ID から受信専用と判定できる
                if self.is_receive_only_id(stream_id) {
                    return Err(WtError::stream_state_error(
                        "WT_MAX_STREAM_DATA received for receive-only stream",
                    ));
                }

                // draft-ietf-webtrans-http2-15 Section 3.4 / Section 5.2 /
                // RFC 9000 Section 2.1 / Section 19.10:
                // 未作成のローカル開始 ID への WT_MAX_STREAM_DATA は WT_STREAM_STATE_ERROR
                // (QUIC の connection error を draft-ietf-webtrans-http2-15 Section 3.4 に
                // 従いストリームエラーで伝える)。削除済みの ID (採番済み範囲内) は
                // 作成済みのため対象外
                if self.is_uncreated_local_id(stream_id) {
                    return Err(WtError::stream_state_error(
                        "WT_MAX_STREAM_DATA received for locally-initiated stream that has not been created",
                    ));
                }

                // draft-ietf-webtrans-http2-15 Section 6.6:
                // ピアから WT_STOP_SENDING を受信した後の WT_MAX_STREAM_DATA は
                // WT_STREAM_STATE_ERROR。ストリーム生成より前に検証し、
                // 拒否時にストリームと WtEvent::StreamOpened を残さない
                if self.stop_sending_received(stream_id) {
                    return Err(WtError::stream_state_error(
                        "WT_MAX_STREAM_DATA received after WT_STOP_SENDING",
                    ));
                }

                // draft-ietf-webtrans-http2-15 Section 5.2 / RFC 9000 Section 3.2:
                // ピア開始 bidi の未知 ID への WT_MAX_STREAM_DATA はそのストリームを開く。
                // 生成されなかった場合 (削除済み ID) は下の更新が行われない
                self.create_implicit_peer_bidi_stream(stream_id)?;

                if let Some(stream) = self.streams.get_mut(&stream_id) {
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
                // 存在しないストリーム、受信側がデータ受信不能な状態のストリーム、および
                // 受信パートを持たないローカル開始 uni ストリームへの
                // WT_STREAM_DATA_BLOCKED は WT_STREAM_STATE_ERROR
                // (RFC 9000 Section 19.13)
                let stream = self.streams.get(&stream_id).ok_or_else(|| {
                    WtError::stream_state_error(
                        "WT_STREAM_DATA_BLOCKED received for unknown stream",
                    )
                })?;
                if !(stream.can_recv() && stream.has_recv_part()) {
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
        let is_peer_initiated = self.is_peer_initiated(stream_id);
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

            self.create_peer_streams_up_to(stream_id, bidirectional)?;
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

    /// ピア開始ストリームを生成して登録する
    ///
    /// 呼び出し元が受信ストリーム数の上限を検証済みであることを前提とする
    /// (`WtSession::create_peer_streams_up_to`)。
    ///
    /// draft-ietf-webtrans-http2-15 Section 11.2 / Section 4.3.1:
    /// ピア開始ストリームの `send_max` はピアの BIDI_LOCAL / UNI (ピア視点で local = ピア開始)、
    /// `recv_max` はローカルの BIDI_REMOTE / UNI (ローカル視点で remote = ピア開始) を使う。
    /// ピア開始 uni は受信専用のため `send_max` は使われない。
    fn create_peer_stream(&mut self, stream_id: WtStreamId, bidirectional: bool) {
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
    }

    /// ピア開始ストリームを、指定 ID 以下の同一型 ID のうち未作成のものについて生成する
    ///
    /// RFC 9000 Section 3.2: 「Before a stream is created, all streams of the same type
    /// with lower-numbered stream IDs MUST be created.」、RFC 9000 Section 2.1:
    /// 「A stream ID that is used out of order results in all streams of that type with
    /// lower-numbered stream IDs also being opened.」、および
    /// draft-ietf-webtrans-http2-15 Section 6.7:
    /// 「Opening a stream with a given ID implicitly opens all streams of the same type
    /// and direction with lower stream IDs.」に従い、未作成のものを小さい順に生成して
    /// `WtEvent::StreamOpened` を送出する。1 回の呼び出しで複数の
    /// `WtEvent::StreamOpened` が送出され得る。
    ///
    /// RFC 9000 Section 4.6, draft-ietf-webtrans-http2-15 Section 6.7:
    /// 指定 ID が受信ストリーム数の上限を超える場合は、下位 ID も含めて 1 件も作成せず
    /// `flow_control_error` を返す。
    ///
    /// 既に `streams` にある ID と、削除済みで `closed_streams` に記録済みの ID は
    /// 作成しない。後者は既に作成済みのストリームが閉じたものであり、
    /// 再作成すると `WtEvent::StreamOpened` が再送出されて
    /// クローズ済みストリームを再作成しない方針 (draft-ietf-webtrans-http2-15 Section 6.4)
    /// に反する。`closed_streams` は上限付きのため、記録から追い出された ID は
    /// 再作成され得る (既知の制限)。
    fn create_peer_streams_up_to(
        &mut self,
        stream_id: WtStreamId,
        bidirectional: bool,
    ) -> WtResult<()> {
        // ローカル開始 ID を渡すと、カウンタを進めずにローカル ID を `streams` へ
        // 入れてしまい `WtSession::is_uncreated_local_id` の前提が崩れる。
        // 呼び出し元はピア開始 ID のみを渡す契約である
        debug_assert!(self.is_peer_initiated(stream_id));
        // 上限超過時は 1 件も作成せずに拒否する。途中まで作成すると `process` の
        // エラー後もストリームと `WtEvent::StreamOpened` が残り、呼び出し側が
        // エラーを無視した場合に存在しないストリームを受理したのと同じ状態になる
        if !self.flow_control.can_accept_stream(stream_id) {
            return Err(WtError::flow_control_error("peer exceeded stream limit"));
        }

        // 同一型の最初の ID (RFC 9000 Section 2.1 のストリーム型)
        let first = stream_id & 0x03;
        let mut id = first;
        while id <= stream_id {
            if !self.streams.contains_key(&id) && !self.closed_streams.contains(&id) {
                self.create_peer_stream(id, bidirectional);
            }
            id = stream::stream_id::next(id);
        }
        Ok(())
    }

    /// 未知のピア開始 bidi ID であればストリームを生成する
    ///
    /// draft-ietf-webtrans-http2-15 Section 5.2: WebTransport ストリームは QUIC の
    /// ストリーム状態を mirror する。RFC 9000 Section 3.2 はピア開始 bidi への
    /// MAX_STREAM_DATA / STOP_SENDING の受信でそのストリームが開かれると定める。
    /// 生成されるのは双方向ストリームであり、送信パートは `Ready` のままとなる。
    ///
    /// 呼び出し元が受信専用 ID (ピア開始 uni) を拒否済みであることを前提とする。
    /// 開始主体がローカル側の ID と、`closed_streams` に記録済みの削除済み ID は
    /// 生成しない。後者を再作成すると閉じたストリームの `WtEvent::StreamOpened` が
    /// 再送出されるため生成しない。生成しなかった場合、呼び出し元の後続処理は
    /// ストリーム不在として扱う (RFC 9000 Section 19.10 がエラーとするのは未作成の
    /// ローカル開始ストリームと受信専用ストリームのみであり、削除済みのピア開始 bidi は
    /// 該当しない。RFC 9000 Section 3.3 は遅延配送により任意の状態で受信し得るとする)。
    ///
    /// RFC 9000 Section 3.2 は WT_RESET_STREAM / WT_STREAM_DATA_BLOCKED についても
    /// 受信パートの生成を定めるが、draft-ietf-webtrans-http2-15 Section 6.2 / Section 6.9 は
    /// 有効でない状態のストリームへの受信を WT_STREAM_STATE_ERROR とするため、
    /// 本メソッドは WT_STOP_SENDING / WT_MAX_STREAM_DATA のみを対象とする。
    fn create_implicit_peer_bidi_stream(&mut self, stream_id: WtStreamId) -> WtResult<()> {
        if !self.is_peer_initiated(stream_id) {
            return Ok(());
        }
        // 呼び出し元が受信専用 ID を拒否済みのため、ここに来る ID は双方向である。
        // 契約が呼び出し元の検証順序に依存していることを明示する
        debug_assert!(stream::stream_id::is_bidirectional(stream_id));
        if self.streams.contains_key(&stream_id) {
            return Ok(());
        }
        if self.closed_streams.contains(&stream_id) {
            return Ok(());
        }
        self.create_peer_streams_up_to(stream_id, true)
    }

    /// ピアから WT_STOP_SENDING を受信済みの ID かどうかを判定する
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.3: 2 回目の WT_STOP_SENDING は
    /// WT_STREAM_STATE_ERROR を返す。Section 6.6: WT_MAX_STREAM_DATA と WT_STOP_SENDING は
    /// いずれもデータ受信側が送る操作であり (RFC 9000 Section 3.3)、WT_STOP_SENDING を
    /// 送った側が WT_MAX_STREAM_DATA を送ってはならないため、受信側は「ピアから
    /// WT_STOP_SENDING を受けた ID」への WT_MAX_STREAM_DATA を拒否する。両者は同じ
    /// 判定を共有する。
    ///
    /// ストリームが存在すれば `WtStream` のフラグを、削除済みなら受理時に記録した集合を
    /// 参照する。記録は「受理した事実」を表し、ストリーム ID は仕様上再利用されない
    /// (RFC 9000 Section 2.1) ため、両者を OR で評価しても誤判定しない。
    #[must_use]
    fn stop_sending_received(&self, stream_id: WtStreamId) -> bool {
        self.stop_sending_received_ids.contains(&stream_id)
            || self
                .streams
                .get(&stream_id)
                .is_some_and(WtStream::stop_sending_received)
    }

    /// `Recv` 状態でないストリームへ WT_MAX_STREAM_DATA を送らないことを検証する
    ///
    /// draft-ietf-webtrans-http2-15 Section 5.2: WebTransport ストリームの状態は
    /// QUIC ストリームの状態を mirror する。RFC 9000 Section 3.3 / Section 19.10:
    /// MAX_STREAM_DATA を送れるのは受信状態が `Recv` のストリームに限られる。
    fn check_max_stream_data_recv_state(stream: &WtStream) -> WtResult<()> {
        if stream.recv_state() != RecvState::Recv {
            return Err(WtError::stream_state_error(
                "cannot send WT_MAX_STREAM_DATA: stream is not in the Recv state",
            ));
        }
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
