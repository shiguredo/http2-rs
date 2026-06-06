//! HTTP/2 接続管理
//!
//! Sans I/O パターンで HTTP/2 接続を管理する。

use std::collections::{BTreeSet, HashMap, VecDeque};

use crate::error::{Error, ErrorCode, Result};
use crate::event::Event;
use crate::flow_control::{FlowControl, MAX_WINDOW_SIZE};
use crate::frame::error::{LastStreamId, WindowIncrement};
use crate::frame::{
    DataFrame, Frame, FrameDecoder, FrameEncoder, GoawayFrame, NonZeroStreamId, PingFrame,
    PriorityUpdateFrame, RstStreamFrame, SettingsFrame, StreamId, WindowUpdateFrame,
};
use crate::hpack::{Decoder as HpackDecoder, Encoder as HpackEncoder, HeaderField};
use crate::limits::Limits;
use crate::settings::{DEFAULT_INITIAL_WINDOW_SIZE, Setting, Settings};
use crate::stream::{Stream, StreamState};
use crate::validation;

mod headers;

/// 接続の役割
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// クライアント
    Client,
    /// サーバー
    Server,
}

/// 接続状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionState {
    /// 初期状態（接続プリフェイスを待っている）
    #[default]
    WaitingPreface,
    /// アクティブ（通常の通信中）
    Active,
    /// GOAWAY 送信済み
    GoawaySent,
    /// GOAWAY 受信済み
    GoawayReceived,
    /// クローズ済み
    Closed,
}

/// 上限付きクローズ済みストリーム ID 集合
///
/// ストリーム ID は単調増加するため、上限を超えた場合は最も小さいエントリ
/// （最も古いストリーム ID）を削除する。
#[derive(Debug)]
struct BoundedClosedStreams {
    inner: BTreeSet<u32>,
    max_size: usize,
}

impl BoundedClosedStreams {
    const DEFAULT_MAX_SIZE: usize = 10000;

    fn new() -> Self {
        Self {
            inner: BTreeSet::new(),
            max_size: Self::DEFAULT_MAX_SIZE,
        }
    }

    fn insert(&mut self, stream_id: u32) {
        self.inner.insert(stream_id);
        while self.inner.len() > self.max_size {
            let oldest = *self
                .inner
                .first()
                .expect("inner is non-empty because len > max_size > 0");
            self.inner.remove(&oldest);
        }
    }

    fn contains(&self, stream_id: &u32) -> bool {
        self.inner.contains(stream_id)
    }
}

/// HTTP/2 接続
#[derive(Debug)]
pub struct Connection {
    /// 接続の役割
    role: Role,
    /// 接続状態
    state: ConnectionState,
    /// ローカル設定
    local_settings: Settings,
    /// リモート設定
    remote_settings: Settings,
    /// 接続レベルのフロー制御
    flow_control: FlowControl,
    /// ストリーム一覧
    streams: HashMap<u32, Stream>,
    /// クローズ済みストリーム ID の集合
    ///
    /// RFC 9113 Section 5.1: マップから削除されたストリームの ID を追跡する。
    /// RST_STREAM 送信後や END_STREAM による正常クローズ後に到着する遅延フレームを
    /// 接続エラーではなく破棄として処理するために使用する。
    closed_streams: BoundedClosedStreams,
    /// 次のストリーム ID
    next_stream_id: u32,
    /// 最後に受信したストリーム ID
    last_recv_stream_id: u32,
    /// 最後に正常処理が完了したストリーム ID (GOAWAY 用)
    /// RFC 9113 Section 5.4.1: GOAWAY には正常に受信した最後のストリーム ID を載せる
    last_successful_stream_id: u32,
    /// HPACK エンコーダー
    hpack_encoder: HpackEncoder,
    /// HPACK デコーダー
    hpack_decoder: HpackDecoder,
    /// フレームデコーダー
    frame_decoder: FrameDecoder,
    /// フレームエンコーダー
    frame_encoder: FrameEncoder,
    /// 出力バッファ
    output_buffer: VecDeque<u8>,
    /// イベントキュー
    events: VecDeque<Event>,
    /// 未 ACK の SETTINGS フレーム数
    ///
    /// RFC 9113 Section 6.5: SETTINGS は接続中いつでも送信でき、
    /// ACK は最古の未 ACK SETTINGS に対する同期点として定義される。
    pending_settings_count: u32,
    /// 接続プリフェイスを受信したかどうか
    preface_received: bool,
    /// 接続プリフェイスを送信したかどうか
    preface_sent: bool,
    /// ヘッダーブロック継続中のストリーム ID
    header_continuation_stream: Option<u32>,
    /// ヘッダーブロックフラグメント
    header_block_fragment: Vec<u8>,
    /// ヘッダーブロック継続中の END_STREAM フラグ
    ///
    /// HEADERS フレームで END_HEADERS が未設定の場合、END_STREAM フラグを保存し、
    /// 最後の CONTINUATION フレームで使用する (RFC 9113 Section 6.2)。
    header_end_stream: bool,
    /// 最初に受信した NO_RFC7540_PRIORITIES の値
    ///
    /// RFC 9218 Section 2.1: この設定は接続中に変更できない。
    /// 最初の値を記録し、以後の変更を拒否する。
    initial_no_rfc7540_priorities: Option<bool>,
    /// ピアが SETTINGS_ENABLE_CONNECT_PROTOCOL=1 を送信したかどうか
    ///
    /// RFC 8441 §3: 一度 1 を送信した後に 0 を送信してはならない (MUST NOT)。
    /// このフラグで過去に true を受信したことを追跡し、ダウングレードを検出する。
    peer_sent_enable_connect_protocol: bool,
    /// 保留中の動的テーブルサイズ更新
    ///
    /// RFC 7541 Section 4.2: SETTINGS_HEADER_TABLE_SIZE 変更を受信した場合、
    /// 次のヘッダーブロック送信時に Dynamic Table Size Update をエンコードする。
    /// ヘッダーブロック間に複数回変化した場合、最小値と最終値の両方を送出する。
    /// (min, final) の形式で保持する。
    pending_table_size_update: Option<(u32, u32)>,
    /// 接続プリフェイス受信バッファ
    ///
    /// サーバーロールで feed() 経由のプリフェイス検証に使用する。
    /// 24 バイト蓄積された時点で検証し、preface_received を true にする。
    preface_buffer: Vec<u8>,
    /// 接続レベルの希望ウィンドウサイズ
    ///
    /// RFC 9113 Section 6.9.2: 接続フロー制御ウィンドウは WINDOW_UPDATE でのみ変更可能。
    /// `DEFAULT_INITIAL_WINDOW_SIZE` (65535) を超える場合は、接続確立直後に
    /// 差分の WINDOW_UPDATE を送信して受信ウィンドウを広告する。
    connection_window_size: u32,
    /// 接続確立時の WINDOW_UPDATE を送信済みかどうか
    ///
    /// `initiate()` と `send_settings()` の両経路から重複送信されるのを防ぐ。
    connection_window_update_sent: bool,
}

impl Connection {
    /// 新しい接続を生成する
    #[must_use]
    pub fn new(role: Role, limits: Limits) -> Self {
        let local_settings = Settings::from_limits(&limits);

        let next_stream_id = match role {
            Role::Client => 1,
            Role::Server => 2,
        };

        // RFC 9113 Section 6.9.2 / Section 5.2.1:
        // 接続レベルの送受信ウィンドウはプロトコル既定 (65535) で初期化する。
        // `connection_window_size` がデフォルトより大きい場合は、initiate() / send_settings()
        // で接続レベルの WINDOW_UPDATE を送信して受信ウィンドウを広告する。
        // SETTINGS_INITIAL_WINDOW_SIZE (0x04) はストリームレベルにのみ適用される。
        let flow_control = FlowControl::new(DEFAULT_INITIAL_WINDOW_SIZE);
        let connection_window_size = limits.connection_window_size().get();

        // RFC 9113 Section 6.5.2: 受信ヘッダーのデコード後サイズ上限 (SETTINGS_MAX_HEADER_LIST_SIZE)
        // はローカル設定で決まる。インデックス参照爆弾に対し、デコーダが展開中に逐次中断できるよう
        // 上限を渡す。local_settings は構築後に変更されないため、デコーダ側の上限と常に一致する。
        let mut hpack_decoder = HpackDecoder::new(limits.header_table_size() as usize);
        hpack_decoder
            .set_max_header_list_size(local_settings.max_header_list_size().map(|v| v as usize));

        Self {
            role,
            state: ConnectionState::WaitingPreface,
            local_settings,
            remote_settings: Settings::new(),
            flow_control,
            streams: HashMap::new(),
            closed_streams: BoundedClosedStreams::new(),
            next_stream_id,
            last_recv_stream_id: 0,
            last_successful_stream_id: 0,
            hpack_encoder: HpackEncoder::new(limits.header_table_size() as usize),
            hpack_decoder,
            frame_decoder: FrameDecoder::new(limits.max_frame_size().get()),
            frame_encoder: FrameEncoder::new(),
            output_buffer: VecDeque::new(),
            events: VecDeque::new(),
            pending_settings_count: 0,
            preface_received: false,
            preface_sent: false,
            header_continuation_stream: None,
            header_block_fragment: Vec::new(),
            header_end_stream: false,
            initial_no_rfc7540_priorities: None,
            peer_sent_enable_connect_protocol: false,
            pending_table_size_update: None,
            preface_buffer: Vec::new(),
            connection_window_size,
            connection_window_update_sent: false,
        }
    }

    /// クライアント接続を生成する
    #[must_use]
    pub fn client(limits: Limits) -> Self {
        Self::new(Role::Client, limits)
    }

    /// サーバー接続を生成する
    #[must_use]
    pub fn server(limits: Limits) -> Self {
        Self::new(Role::Server, limits)
    }

    /// 接続の役割を取得する
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// 接続状態を取得する
    #[must_use]
    pub const fn state(&self) -> ConnectionState {
        self.state
    }

    /// ローカル設定を取得する
    ///
    /// 送信済みの SETTINGS に対応する設定。WebTransport 初期設定
    /// (`wt_initial_max_*`) を含む拡張 SETTINGS もここから参照できる。
    #[must_use]
    pub const fn local_settings(&self) -> &Settings {
        &self.local_settings
    }

    /// リモート設定を取得する
    ///
    /// ピアから受信して ACK した SETTINGS に対応する設定。
    /// WebTransport セッションを張る前に `enable_connect_protocol` や
    /// `wt_initial_max_*` を確認する用途で使用する。
    #[must_use]
    pub const fn remote_settings(&self) -> &Settings {
        &self.remote_settings
    }

    /// 接続がアクティブかどうかを返す
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self.state, ConnectionState::Active)
    }

    /// 接続がクローズされたかどうかを返す
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self.state, ConnectionState::Closed)
    }

    /// テスト用: next_stream_id を指定値に設定する
    #[cfg(test)]
    pub(crate) fn set_next_stream_id(&mut self, id: u32) {
        self.next_stream_id = id;
    }

    /// 接続プリフェイスを送信する（クライアント）
    ///
    /// RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 0 以外に設定できない。
    /// そのため、サーバーの場合は ENABLE_PUSH を送信しない。
    pub fn initiate(&mut self) -> Result<()> {
        if self.preface_sent {
            return Ok(());
        }

        if self.role == Role::Client {
            // クライアントはプリフェイス文字列を送信する
            self.output_buffer.extend(crate::CONNECTION_PREFACE);
        }

        // SETTINGS フレームを送信する
        let mut settings_frame = SettingsFrame::new();
        for setting in self.local_settings.to_settings_list() {
            // RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 1 に設定できない
            if self.role == Role::Server && matches!(setting, Setting::EnablePush(_)) {
                continue;
            }
            settings_frame.add(setting);
        }
        self.send_frame(&Frame::Settings(settings_frame))?;
        self.pending_settings_count += 1;
        self.preface_sent = true;

        // RFC 9113 Section 6.9.2: 接続レベルのウィンドウは SETTINGS では変更できないため
        // デフォルト (65535) を超える受信ウィンドウは WINDOW_UPDATE で広告する。
        self.send_initial_connection_window_update()?;

        Ok(())
    }

    /// データを入力バッファに追加する
    ///
    /// サーバーロールでプリフェイス未受信の場合、先頭 24 バイトを接続プリフェイスとして
    /// 検証する (RFC 9113 Section 3.4)。プリフェイスが無効な場合は PROTOCOL_ERROR を返す。
    pub fn feed(&mut self, data: &[u8]) -> Result<usize> {
        // サーバーロールでプリフェイス未受信の場合、先頭バイトを検証する
        if self.role == Role::Server && !self.preface_received {
            let preface_len = crate::CONNECTION_PREFACE_LEN;
            let needed = preface_len - self.preface_buffer.len();
            let consume = data.len().min(needed);

            self.preface_buffer.extend_from_slice(&data[..consume]);

            // 蓄積分が接続プリフェイスのプレフィックスと一致するか検証する
            // 不一致を早期検出することで、不正なデータがフレームデコーダーに渡るのを防ぐ
            let buf_len = self.preface_buffer.len();
            if self.preface_buffer[..] != crate::CONNECTION_PREFACE[..buf_len] {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    "invalid client connection preface",
                ));
            }

            if buf_len < preface_len {
                // まだプリフェイス全体を受信していない
                return Ok(data.len());
            }

            self.preface_received = true;

            // プリフェイス以降のデータをフレームデコーダーに渡す
            let remaining = &data[consume..];
            if !remaining.is_empty() {
                self.frame_decoder.feed(remaining);
            }
        } else {
            self.frame_decoder.feed(data);
        }

        Ok(data.len())
    }

    /// 接続プリフェイス受信済みとしてマークする（外部でプリフェイスを処理した場合）
    pub fn mark_preface_received(&mut self) {
        self.preface_received = true;
    }

    /// 接続プリフェイス送信済みとしてマークする（外部でプリフェイスを送信した場合）
    pub fn mark_preface_sent(&mut self) {
        self.preface_sent = true;
    }

    /// SETTINGS フレームを送信する
    ///
    /// initiate() とは異なり、preface_sent のチェックを行わない。
    /// 外部でコネクションプリフェイスを処理した場合に使用する。
    ///
    /// RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 0 以外に設定できない。
    /// そのため、サーバーの場合は ENABLE_PUSH を送信しない。
    pub fn send_settings(&mut self) -> Result<()> {
        let mut settings_frame = SettingsFrame::new();
        for setting in self.local_settings.to_settings_list() {
            // RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 1 に設定できない
            if self.role == Role::Server && matches!(setting, Setting::EnablePush(_)) {
                continue;
            }
            settings_frame.add(setting);
        }
        self.send_frame(&Frame::Settings(settings_frame))?;
        self.pending_settings_count += 1;

        // RFC 9113 Section 6.9.2: 接続レベルのウィンドウは SETTINGS では変更できないため
        // デフォルト (65535) を超える受信ウィンドウは WINDOW_UPDATE で広告する。
        self.send_initial_connection_window_update()?;

        Ok(())
    }

    /// 接続確立時に接続レベル WINDOW_UPDATE を送信する
    ///
    /// `connection_window_size` がデフォルト (65535) より大きい場合のみ送信し、
    /// 初回送信後はフラグで二重送信を防ぐ。
    fn send_initial_connection_window_update(&mut self) -> Result<()> {
        if self.connection_window_update_sent {
            return Ok(());
        }
        if self.connection_window_size > DEFAULT_INITIAL_WINDOW_SIZE {
            let increment = self.connection_window_size - DEFAULT_INITIAL_WINDOW_SIZE;
            self.send_window_update(StreamId::Connection, increment)?;
        }
        // 既定値の場合でもフラグを立てて、後段の send_settings() で再評価しない。
        self.connection_window_update_sent = true;
        Ok(())
    }

    /// フレームを処理してイベントを生成する
    pub fn process(&mut self) -> Result<()> {
        loop {
            match self.frame_decoder.decode() {
                Ok(Some(frame)) => self.handle_frame(frame)?,
                Ok(None) => break,
                Err(e) if e.is_stream_error() => {
                    // RFC 9113 Section 5.4.2: ストリームエラーは RST_STREAM で処理し、
                    // 接続全体を落とさない
                    if let Some(stream_id) = self.frame_decoder.last_decoded_stream_id()
                        && let Some(error_code) = e.error_code()
                    {
                        // RFC 9113 Section 6.4: idle ストリームへの RST_STREAM は禁止
                        // されているため、idle ストリームの場合は接続エラーに昇格する
                        if self.is_idle_stream(stream_id) {
                            return Err(Error::connection_error(
                                error_code,
                                format!(
                                    "stream error on idle stream {} promoted to connection error",
                                    stream_id
                                ),
                            ));
                        }
                        self.reset_stream(StreamId::from_wire(stream_id), error_code)?;
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// イベントを取得する
    #[must_use]
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// 出力データを取得する
    #[must_use]
    pub fn poll_output(&mut self) -> Option<Vec<u8>> {
        if self.output_buffer.is_empty() {
            None
        } else {
            Some(self.output_buffer.drain(..).collect())
        }
    }

    /// 出力バッファにデータがあるかどうかを返す
    #[must_use]
    pub fn has_output(&self) -> bool {
        !self.output_buffer.is_empty()
    }

    /// 新しいストリームを開始する
    pub fn start_stream(
        &mut self,
        headers: Vec<HeaderField>,
        end_stream: bool,
    ) -> Result<StreamId> {
        // RFC 9113 Section 8.3.1: 送信前にリクエストヘッダーの妥当性を検証する
        validation::validate_request_headers(&headers)?;

        // RFC 9113 §8.4: サーバープッシュ非サポートのため、サーバーは新規ストリームを開始できない
        // (送信前ローカル検査であり wire 上の RST_STREAM は送信しない)
        if self.role == Role::Server {
            return Err(Error::protocol_error(
                "server cannot initiate streams (server push not supported)",
            ));
        }

        // RFC 9113 Section 6.8: GOAWAY 受信後は新規ストリームを開始できない
        if matches!(self.state, ConnectionState::GoawayReceived) {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "cannot start new stream after GOAWAY received",
            ));
        }

        // RFC 9113 Section 5.1.2: peer が設定した同時ストリーム上限を超えてはならない
        if let Some(max) = self.remote_settings.max_concurrent_streams() {
            let current_open = self
                .streams
                .values()
                .filter(|s| !s.state().is_closed())
                .count();
            if current_open >= max as usize {
                // RFC 9113 §5.1.2: REFUSED_STREAM は再試行可能性を示す
                // (送信前ローカル検査であり wire 上の RST_STREAM は送信しない)
                return Err(Error::stream_error(
                    ErrorCode::RefusedStream,
                    "max concurrent streams limit set by peer would be exceeded",
                ));
            }
        }

        // RFC 8441 Section 3, draft-ietf-webtrans-http2-14 Section 3.1:
        // :protocol を含むリクエストは、ピアが SETTINGS_ENABLE_CONNECT_PROTOCOL=1 を
        // 送信済みの場合のみ許可する。
        let has_protocol = headers
            .iter()
            .any(|h| h.name() == crate::validation::pseudo_headers::PROTOCOL);
        if has_protocol && !self.remote_settings.enable_connect_protocol() {
            return Err(Error::protocol_error(
                "cannot send :protocol without peer's SETTINGS_ENABLE_CONNECT_PROTOCOL=1",
            ));
        }

        // RFC 9113 Section 10.5.1: 送信ヘッダーリストサイズの上限チェック
        if let Some(max_size) = self.remote_settings.max_header_list_size() {
            let header_list_size = Self::calculate_header_list_size(&headers);
            if header_list_size > max_size as usize {
                // RFC 9113 §10.5.1: 送信前ローカル検査
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    format!(
                        "header list size {} exceeds peer's SETTINGS_MAX_HEADER_LIST_SIZE {}",
                        header_list_size, max_size
                    ),
                ));
            }
        }

        // RFC 9113 §5.1.1: ストリーム ID は unsigned 31-bit integer (最大 2^31 - 1)。
        // 枯渇した場合は新しい接続の確立が必要。
        if self.next_stream_id > crate::stream_id::STREAM_ID_MAX {
            return Err(Error::stream_error(
                ErrorCode::RefusedStream,
                "stream ID space exhausted, establish a new connection",
            ));
        }

        let stream_id_u32 = self.next_stream_id;
        let stream_id = StreamId::from_wire(stream_id_u32);
        let nz_stream_id = stream_id
            .non_zero()
            .expect("next_stream_id is always non-zero");
        self.next_stream_id += 2;

        // RFC 9113 §5.2: 送信ウィンドウはリモートの initial_window_size、
        // 受信ウィンドウはローカルの initial_window_size で初期化する
        let mut stream = Stream::new(
            stream_id,
            self.remote_settings.initial_window_size().get(),
            self.local_settings.initial_window_size().get(),
        );
        stream.state_machine_mut().send_headers(end_stream)?;
        stream.set_headers(headers.clone());

        // リクエストメソッドと :protocol を記録する
        if let Some(method_header) = headers
            .iter()
            .find(|h| h.name() == validation::pseudo_headers::METHOD)
        {
            stream.set_request_method(method_header.value().to_vec());
            let protocol_value = headers
                .iter()
                .find(|h| h.name() == validation::pseudo_headers::PROTOCOL)
                .map(|h| h.value().to_vec());
            stream.set_has_protocol(protocol_value.is_some());
            if let Some(proto) = protocol_value {
                stream.set_protocol(proto);
            }
        }

        self.streams.insert(stream_id_u32, stream);

        // HEADERS フレームを送信 (必要に応じて CONTINUATION に分割)
        let mut encoded_headers = Vec::new();
        self.hpack_encoder.encode(&mut encoded_headers, &headers);

        self.send_header_block(nz_stream_id, encoded_headers, end_stream)?;

        Ok(stream_id)
    }

    /// ストリームにデータを送信する
    ///
    /// フロー制御に従い、送信可能な分だけ送信する。
    /// 送信できないデータは内部バッファにキューイングされ、
    /// WINDOW_UPDATE 受信時に自動的に送信される。
    pub fn send_data(
        &mut self,
        stream_id: StreamId,
        data: Vec<u8>,
        end_stream: bool,
    ) -> Result<()> {
        let sid = stream_id.as_u32();

        // ストリームの存在確認と事前検証
        {
            let stream = self
                .streams
                .get_mut(&sid)
                .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

            // 既に END_STREAM をキューに積んだストリームへの追加 DATA は禁止。
            // state_machine の遷移は最後の DATA を実際に送信完了した時点で行うため
            // state はまだ Open/HalfClosedRemote のままだが、利用者の意図としては
            // 既に END_STREAM 宣言済みなので拒否する (RFC 9113 §5.1)。
            if stream.pending_end_stream() {
                return Err(Error::stream_error(
                    ErrorCode::StreamClosed,
                    "cannot send DATA after END_STREAM was queued for this stream",
                ));
            }

            // 状態チェック（end_stream=false でも送信可能な状態か検証する）
            stream.state_machine_mut().send_data(end_stream)?;
        }

        // データをキューに追加
        self.queue_data(sid, data, end_stream)?;

        // キューから送信可能な分を送信
        self.flush_stream_data(sid)?;

        // RFC 9113 Section 5.1: END_STREAM 付きフレームを実際に送信完了した場合のみ
        // ストリームを closed として削除する
        self.try_remove_closed_stream(sid);

        Ok(())
    }

    /// ストリームのデータをキューに追加する
    fn queue_data(&mut self, stream_id: u32, data: Vec<u8>, end_stream: bool) -> Result<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

        // 送信バッファにデータを追加
        let remaining = stream.send_buffer_mut().push(&data);
        if remaining > 0 {
            // バッファが満杯の場合はエラー
            return Err(Error::connection_error(
                ErrorCode::FlowControlError,
                "send buffer full",
            ));
        }

        // end_stream フラグを記録
        if end_stream {
            stream.set_pending_end_stream(true);
        }

        Ok(())
    }

    /// ストリームのキューからデータを送信する
    fn flush_stream_data(&mut self, stream_id: u32) -> Result<()> {
        loop {
            // 送信可能なサイズを計算
            let (send_size, pending_end_stream) = {
                let stream = match self.streams.get(&stream_id) {
                    Some(s) => s,
                    None => return Ok(()),
                };

                let buffer_len = stream.send_buffer().len();
                let pending_es = stream.pending_end_stream();
                // RFC 9113 Section 6.9.1: フロー制御ウィンドウが 0 でも
                // END_STREAM 付きの長さ 0 DATA フレームは送信してよい
                if buffer_len == 0 && !pending_es {
                    return Ok(());
                }
                if buffer_len == 0 && pending_es {
                    break;
                }

                // 接続レベルのウィンドウ
                let conn_window = self.flow_control.send_available();
                // ストリームレベルのウィンドウ
                let stream_window = stream.flow_control().send_available();
                // 最大フレームサイズ
                let max_frame = self.remote_settings.max_frame_size().get() as usize;

                // 送信可能なサイズ（ウィンドウとフレームサイズの最小値）
                let available = conn_window.min(stream_window).min(max_frame);

                if available == 0 {
                    // ウィンドウが枯渇している場合は送信しない
                    return Ok(());
                }

                let send_size = buffer_len.min(available);
                let pending_end_stream = stream.pending_end_stream();

                (send_size, pending_end_stream)
            };

            // データを取り出す
            // 直前の get で存在を確認済みのため expect で安全
            let data = {
                let stream = self.streams.get_mut(&stream_id).expect("stream must exist");
                stream.send_buffer_mut().pop(send_size)
            };

            // end_stream フラグを決定（バッファが空になり、pending_end_stream が true の場合）
            let remaining_after = {
                let stream = self.streams.get(&stream_id).expect("stream must exist");
                stream.send_buffer().len()
            };
            let end_stream = pending_end_stream && remaining_after == 0;

            // フロー制御を更新
            self.flow_control.consume_send(data.len())?;
            {
                let stream = self.streams.get_mut(&stream_id).expect("stream must exist");
                stream.flow_control_mut().consume_send(data.len())?;

                // end_stream を送信したらフラグをクリアし、状態機械を遷移させる
                if end_stream {
                    stream.set_pending_end_stream(false);
                    stream.state_machine_mut().complete_send_data(true)?;
                }
            }

            // DATA フレームを送信
            let sid =
                NonZeroStreamId::new(stream_id).expect("stream IDs in HashMap are always non-zero");
            let data_frame = DataFrame::new(sid, data).with_end_stream(end_stream);
            self.send_frame(&Frame::Data(data_frame))?;

            if end_stream || remaining_after == 0 {
                break;
            }
        }

        // RFC 9113 §6.9.1: 空 DATA + END_STREAM を送信する
        // ループから break で抜けた場合（buffer_len == 0 && pending_end_stream）
        if let Some(stream) = self.streams.get_mut(&stream_id)
            && stream.pending_end_stream()
            && stream.send_buffer().is_empty()
        {
            stream.set_pending_end_stream(false);
            stream.state_machine_mut().complete_send_data(true)?;
            let sid =
                NonZeroStreamId::new(stream_id).expect("stream IDs in HashMap are always non-zero");
            let data_frame = DataFrame::new(sid, vec![]).with_end_stream(true);
            self.send_frame(&Frame::Data(data_frame))?;
        }

        Ok(())
    }

    /// 全ストリームのキューからデータを送信する
    fn flush_all_stream_data(&mut self) -> Result<()> {
        // 送信待ちデータまたは送信待ち END_STREAM があるストリームを収集
        let stream_ids: Vec<u32> = self
            .streams
            .iter()
            .filter(|(_, s)| !s.send_buffer().is_empty() || s.pending_end_stream())
            .map(|(id, _)| *id)
            .collect();

        for stream_id in stream_ids {
            self.flush_stream_data(stream_id)?;
            self.try_remove_closed_stream(stream_id);
        }

        Ok(())
    }

    /// END_STREAM 送信済みかつ状態が Closed のストリームを削除する
    ///
    /// RFC 9113 Section 5.1: END_STREAM 付きフレームを実際に送信した時点で
    /// ストリームは closed に遷移する。送信バッファに未送信データが残っている間は
    /// ストリームを削除してはならない。
    fn try_remove_closed_stream(&mut self, stream_id: u32) {
        let should_remove = self.streams.get(&stream_id).is_some_and(|stream| {
            stream.state() == StreamState::Closed
                && stream.send_buffer().is_empty()
                && !stream.pending_end_stream()
        });
        if should_remove {
            self.events.push_back(Event::StreamClosed {
                stream_id: StreamId::from_wire(stream_id),
            });
            self.closed_streams.insert(stream_id);
            self.streams.remove(&stream_id);
        }
    }

    /// ストリームをリセットする
    pub fn reset_stream(&mut self, stream_id: StreamId, error_code: ErrorCode) -> Result<()> {
        // RFC 9113 §6.4: RST_STREAM は非ゼロストリーム ID に関連付けなければならない
        let nz_stream_id = stream_id.non_zero().ok_or_else(|| {
            Error::connection_error(
                ErrorCode::ProtocolError,
                "RST_STREAM requires non-zero stream ID",
            )
        })?;

        if let Some(stream) = self.streams.get_mut(&stream_id.as_u32()) {
            stream.state_machine_mut().send_rst_stream();
        }

        let rst_frame = RstStreamFrame::new(nz_stream_id, error_code.as_u32());
        self.send_frame(&Frame::RstStream(rst_frame))?;

        Ok(())
    }

    /// PING を送信する
    pub fn send_ping(&mut self, opaque_data: [u8; 8]) -> Result<()> {
        let ping_frame = PingFrame::new(opaque_data);
        self.send_frame(&Frame::Ping(ping_frame))?;
        Ok(())
    }

    /// GOAWAY を送信する
    pub fn send_goaway(&mut self, error_code: ErrorCode, debug_data: Vec<u8>) -> Result<()> {
        // last_successful_stream_id は 31-bit 範囲に必ず収まる
        let last_stream_id = LastStreamId::new(self.last_successful_stream_id)
            .expect("last_successful_stream_id is always valid for LastStreamId");
        let goaway_frame =
            GoawayFrame::new(last_stream_id, error_code.as_u32()).with_debug_data(debug_data);
        self.send_frame(&Frame::Goaway(goaway_frame))?;
        self.state = ConnectionState::GoawaySent;
        Ok(())
    }

    /// WINDOW_UPDATE を送信する
    pub fn send_window_update(&mut self, stream_id: StreamId, increment: u32) -> Result<()> {
        // RFC 9113 §6.9: increment は 1 以上 2^31-1 以下でなければならない
        if increment == 0 {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "WINDOW_UPDATE increment must be non-zero",
            ));
        }
        if increment > MAX_WINDOW_SIZE {
            return Err(Error::connection_error(
                ErrorCode::FlowControlError,
                "WINDOW_UPDATE increment exceeds maximum window size",
            ));
        }

        // ローカル状態を先に更新し、オーバーフロー検出時にフレームを送信しない
        if matches!(stream_id, StreamId::Connection) {
            self.flow_control.add_recv_window(increment)?;
        } else if let Some(stream) = self.streams.get_mut(&stream_id.as_u32()) {
            stream.flow_control_mut().add_recv_window(increment)?;
        }

        // 事前検査で範囲を保証済み
        let wi = WindowIncrement::new(increment)
            .expect("increment validated non-zero and <= MAX_WINDOW_SIZE");
        let window_update_frame = match stream_id.non_zero() {
            Some(nz) => WindowUpdateFrame::for_stream(nz, wi),
            None => WindowUpdateFrame::for_connection(wi),
        };
        self.send_frame(&Frame::WindowUpdate(window_update_frame))?;

        Ok(())
    }

    /// フレームを処理する
    fn handle_frame(&mut self, frame: Frame) -> Result<()> {
        // RFC 9113 Section 3.4: 接続プリフェイス検証
        if self.state == ConnectionState::WaitingPreface {
            // サーバーは client preface (24 octets) を受信済みでなければならない
            if self.role == Role::Server && !self.preface_received {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    "client connection preface not received",
                ));
            }
            // 最初のフレームは SETTINGS (ACK なし) でなければならない
            if !matches!(&frame, Frame::Settings(sf) if !sf.is_ack()) {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    "first frame must be SETTINGS",
                ));
            }
        }

        // ヘッダーブロック継続中は CONTINUATION のみ許可
        if let Some(expected_stream_id) = self.header_continuation_stream {
            match &frame {
                Frame::Continuation(cf) if cf.stream_id.as_u32() == expected_stream_id => {
                    // OK
                }
                _ => {
                    return Err(Error::connection_error(
                        ErrorCode::ProtocolError,
                        "expected CONTINUATION frame",
                    ));
                }
            }
        }

        match frame {
            Frame::Data(f) => self.handle_data(f)?,
            Frame::Headers(f) => self.handle_headers(f)?,
            Frame::Priority(_) => {
                // RFC 9113 Section 6.3: 非推奨だが相互運用性のため受信は処理する
                // 優先度情報は無視し、エラーを返さない
            }
            Frame::RstStream(f) => self.handle_rst_stream(f)?,
            Frame::Settings(f) => self.handle_settings(f)?,
            Frame::PushPromise { .. } => {
                // RFC 9113 Section 6.6: PUSH_PROMISE はサーバーのみが送信可能
                // このライブラリはサーバープッシュをサポートしないため、
                // 受信した場合は常に PROTOCOL_ERROR を返す
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    "PUSH_PROMISE not supported",
                ));
            }
            Frame::Ping(f) => self.handle_ping(f)?,
            Frame::Goaway(f) => self.handle_goaway(f)?,
            Frame::WindowUpdate(f) => self.handle_window_update(f)?,
            Frame::Continuation(f) => self.handle_continuation(f)?,
            Frame::PriorityUpdate(f) => self.handle_priority_update(f)?,
            Frame::Unknown { header, .. } => {
                // RFC 9113 Section 8.5: CONNECT 確立済みストリームでは
                // DATA/RST_STREAM/WINDOW_UPDATE/PRIORITY 以外のフレームはストリームエラー。
                // 未知フレームタイプもこの制約に該当する。
                if header.stream_id != 0
                    && let Some(stream) = self.streams.get(&header.stream_id)
                    && stream.connect_established()
                {
                    return Err(Error::stream_error(
                        ErrorCode::ProtocolError,
                        "unknown frame type not allowed on established CONNECT tunnel",
                    ));
                }
                // RFC 9113 Section 4.1: それ以外の未知フレームは無視する
            }
        }

        Ok(())
    }

    /// DATA フレームを処理する
    fn handle_data(&mut self, frame: DataFrame) -> Result<()> {
        let sid = frame.stream_id.as_u32();

        // RFC 9113 Section 5.1: アイドルストリームへのフレームは接続エラー
        self.check_not_idle_stream(sid, "DATA")?;

        // RFC 9113 Section 6.9.1: フロー制御はペイロード全体に適用
        // (Pad Length フィールド + データ + パディング)
        let flow_control_size = if let Some(pad_length) = frame.pad_length {
            1 + frame.data.len() + pad_length as usize
        } else {
            frame.data.len()
        };

        // RFC 9113 Section 6.9: フロー制御フレームの受信者は、接続エラーとして扱わない限り、
        // 常に接続フロー制御ウィンドウに計上しなければならない (MUST)。
        // ストリームエラーで落とす場合でも接続ウィンドウは減算する必要がある。
        self.flow_control.consume_recv(flow_control_size)?;

        // RFC 9113 Section 5.1: Closed 状態のストリームへの DATA は最小処理して破棄する。
        // 接続フロー制御ウィンドウへの計上は上記で完了済み。
        // check_not_idle_stream を通過してマップにないストリームは暗黙的にクローズ済み。
        if self.is_stream_closed(sid) || !self.streams.contains_key(&sid) {
            return Ok(());
        }

        let is_closed = {
            let stream = self
                .streams
                .get_mut(&sid)
                .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

            stream.state_machine_mut().recv_data(frame.end_stream)?;
            stream.flow_control_mut().consume_recv(flow_control_size)?;

            // RFC 9113 Section 8.1.1: コンテンツを持たないレスポンス (204/304/HEAD) に
            // 内容を持つ DATA フレームが含まれている場合は malformed として扱う
            // (no-content の定義は RFC 9110 Section 6.4.1)
            if stream.no_content() {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    "DATA received on response defined as having no content (204/304/HEAD)",
                ));
            }

            // RFC 9113 Section 8.1.1: Content-Length とボディサイズの一貫性チェック
            let data_len = frame.data.len() as u64;
            stream.add_received_content_length(data_len);

            if let Some(expected) = stream.expected_content_length() {
                let received = stream.received_content_length();
                // 受信データが Content-Length を超過
                if received > expected {
                    return Err(Error::stream_error(
                        ErrorCode::ProtocolError,
                        format!(
                            "content-length mismatch: received {} exceeds expected {}",
                            received, expected
                        ),
                    ));
                }
                // END_STREAM 時に Content-Length と一致しない
                if frame.end_stream && received != expected {
                    return Err(Error::stream_error(
                        ErrorCode::ProtocolError,
                        format!(
                            "content-length mismatch: received {} but expected {}",
                            received, expected
                        ),
                    ));
                }
            }

            frame.end_stream && stream.state() == StreamState::Closed
        };

        let stream_id = StreamId::from(frame.stream_id);
        self.events.push_back(Event::DataReceived {
            stream_id,
            data: frame.data,
            end_stream: frame.end_stream,
        });

        if is_closed {
            self.events.push_back(Event::StreamClosed { stream_id });
            self.closed_streams.insert(sid);
            self.streams.remove(&sid);
        }

        Ok(())
    }

    /// RST_STREAM フレームを処理する
    fn handle_rst_stream(&mut self, frame: RstStreamFrame) -> Result<()> {
        let sid = frame.stream_id.as_u32();

        // RFC 9113 §5.1: アイドルストリームへのフレームは接続エラー
        self.check_not_idle_stream(sid, "RST_STREAM")?;

        match self.streams.get_mut(&sid) {
            Some(stream) => {
                stream.state_machine_mut().recv_rst_stream();
                self.events.push_back(Event::StreamReset {
                    stream_id: StreamId::from(frame.stream_id),
                    error_code: ErrorCode::from_u32(frame.error_code),
                });
                self.closed_streams.insert(sid);
                self.streams.remove(&sid);
            }
            None => {
                // 暗黙的にクローズ済みストリームへの RST_STREAM は無視
            }
        }
        Ok(())
    }

    /// SETTINGS フレームを処理する
    fn handle_settings(&mut self, frame: SettingsFrame) -> Result<()> {
        if frame.is_ack() {
            // RFC 9113 Section 6.5: 対応する SETTINGS がない ACK は接続エラー
            if self.pending_settings_count == 0 {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    "received SETTINGS ACK without pending SETTINGS",
                ));
            }
            // SETTINGS ACK を受信 (最古の未 ACK SETTINGS に対応)
            self.pending_settings_count -= 1;
            if self.state == ConnectionState::WaitingPreface {
                self.state = ConnectionState::Active;
            }
            self.events.push_back(Event::SettingsReceived { ack: true });
        } else {
            // SETTINGS を受信
            // RFC 9113 Section 6.5.2: SETTINGS_INITIAL_WINDOW_SIZE 変更時に
            // 既存ストリームのウィンドウサイズを調整する
            let old_initial_window_size = self.remote_settings.initial_window_size().get();

            // HEADER_TABLE_SIZE の変更を追跡
            let old_header_table_size = self.remote_settings.header_table_size();

            for setting in frame.settings() {
                // RFC 9113 Section 8.4: サーバーはクライアントに ENABLE_PUSH=1 を送信できない
                if self.role == Role::Client && matches!(setting, Setting::EnablePush(true)) {
                    return Err(Error::connection_error(
                        ErrorCode::ProtocolError,
                        "server sent ENABLE_PUSH=1 to client",
                    ));
                }

                // RFC 9218 Section 2.1: NO_RFC7540_PRIORITIES は接続中に変更できない
                if let Setting::NoRfc7540Priorities(new_value) = setting {
                    if let Some(initial_value) = self.initial_no_rfc7540_priorities {
                        if *new_value != initial_value {
                            return Err(Error::connection_error(
                                ErrorCode::ProtocolError,
                                "NO_RFC7540_PRIORITIES cannot be changed after initial setting",
                            ));
                        }
                    } else {
                        self.initial_no_rfc7540_priorities = Some(*new_value);
                    }
                }

                // RFC 8441 §3: SETTINGS_ENABLE_CONNECT_PROTOCOL を 1 に設定した後に
                // 0 を送信してはならない (MUST NOT)。違反は PROTOCOL_ERROR。
                if let Setting::EnableConnectProtocol(value) = setting {
                    if *value {
                        self.peer_sent_enable_connect_protocol = true;
                    } else if self.peer_sent_enable_connect_protocol {
                        return Err(Error::connection_error(
                            ErrorCode::ProtocolError,
                            "SETTINGS_ENABLE_CONNECT_PROTOCOL cannot be set to 0 after sending 1",
                        ));
                    }
                }

                self.remote_settings.apply(*setting);
            }

            // RFC 9218 Section 2.1: NO_RFC7540_PRIORITIES は最初の SETTINGS フレームで
            // 送らなければならない (MUST)。最初の SETTINGS に含まれなかった場合、
            // デフォルト値 (0 = false) で確定し、以後の変更を拒否する。
            if self.state == ConnectionState::WaitingPreface
                && self.initial_no_rfc7540_priorities.is_none()
            {
                self.initial_no_rfc7540_priorities = Some(false);
            }

            // RFC 7541 Section 4.2: HEADER_TABLE_SIZE が変更された場合、
            // 次のヘッダーブロック送信時に Dynamic Table Size Update をエンコードする。
            // ヘッダーブロック間に複数回変化した場合、最小値と最終値の両方を送出する必要がある。
            let new_header_table_size = self.remote_settings.header_table_size();
            if new_header_table_size != old_header_table_size {
                let new_min = match self.pending_table_size_update {
                    Some((existing_min, _)) => existing_min.min(new_header_table_size),
                    None => new_header_table_size,
                };
                self.pending_table_size_update = Some((new_min, new_header_table_size));
            }

            // SETTINGS_INITIAL_WINDOW_SIZE が変更された場合、既存ストリームを更新
            let new_initial_window_size = self.remote_settings.initial_window_size().get();
            if new_initial_window_size != old_initial_window_size {
                self.update_stream_windows(new_initial_window_size)?;
            }

            // HPACK エンコーダーのテーブルサイズを更新
            self.hpack_encoder
                .set_max_table_size(self.remote_settings.header_table_size() as usize);

            // RFC 9113 Section 4.2: 受信フレームサイズの上限はローカル設定で決まる。
            // remote_settings.max_frame_size は送信フレームの上限として使用する。
            // frame_decoder の max_frame_size はローカル設定で初期化済みなので更新不要。

            // SETTINGS ACK を送信
            self.send_frame(&Frame::Settings(SettingsFrame::ack()))?;

            self.events
                .push_back(Event::SettingsReceived { ack: false });

            if self.state == ConnectionState::WaitingPreface {
                self.state = ConnectionState::Active;
                self.events.push_back(Event::ConnectionPreface);
            }
        }

        Ok(())
    }

    /// SETTINGS_INITIAL_WINDOW_SIZE 変更時に既存ストリームのウィンドウサイズを調整する
    ///
    /// RFC 9113 Section 6.5.2: When the value of SETTINGS_INITIAL_WINDOW_SIZE changes,
    /// a receiver MUST adjust the size of all stream flow-control windows that it
    /// maintains by the difference between the new value and the old value.
    fn update_stream_windows(&mut self, new_initial_window_size: u32) -> Result<()> {
        for stream in self.streams.values_mut() {
            stream
                .flow_control_mut()
                .update_initial_window_size(new_initial_window_size)?;
        }
        Ok(())
    }

    /// PING フレームを処理する
    fn handle_ping(&mut self, frame: PingFrame) -> Result<()> {
        if !frame.ack {
            // PING ACK を送信
            let ack_frame = PingFrame::ack(frame.opaque_data);
            self.send_frame(&Frame::Ping(ack_frame))?;
        }

        self.events.push_back(Event::PingReceived {
            opaque_data: frame.opaque_data,
            ack: frame.ack,
        });

        Ok(())
    }

    /// GOAWAY フレームを処理する
    fn handle_goaway(&mut self, frame: GoawayFrame) -> Result<()> {
        self.state = ConnectionState::GoawayReceived;

        self.events.push_back(Event::GoawayReceived {
            last_stream_id: StreamId::from_wire(frame.last_stream_id.get()),
            error_code: ErrorCode::from_u32(frame.error_code),
            debug_data: frame.debug_data,
        });

        Ok(())
    }

    /// WINDOW_UPDATE フレームを処理する
    fn handle_window_update(&mut self, frame: WindowUpdateFrame) -> Result<()> {
        let sid = frame.stream_id.as_u32();
        let increment_u32 = frame.window_size_increment.as_u32();

        if matches!(frame.stream_id, StreamId::Connection) {
            self.flow_control.recv_window_update(increment_u32)?;
            // 接続レベルのウィンドウが増えたので、全ストリームのキューを処理
            self.flush_all_stream_data()?;
        } else {
            // RFC 9113 §5.1: アイドルストリームへのフレームは接続エラー
            self.check_not_idle_stream(sid, "WINDOW_UPDATE")?;

            if let Some(stream) = self.streams.get_mut(&sid) {
                // RFC 9113 §5.1: Closed 状態のストリームへの
                // WINDOW_UPDATE は無視する
                if stream.state() == StreamState::Closed {
                    return Ok(());
                }
                // RFC 9113 §6.9.1: ストリームレベルのウィンドウオーバーフローは
                // RST_STREAM(FLOW_CONTROL_ERROR) で処理する（接続エラーではない）。
                // WindowIncrement 型が非ゼロを保証するため、ここで発生するエラーは
                // オーバーフローのみ。
                if stream
                    .flow_control_mut()
                    .recv_window_update(increment_u32)
                    .is_err()
                {
                    self.reset_stream(frame.stream_id, ErrorCode::FlowControlError)?;
                    return Ok(());
                }
                // ストリームレベルのウィンドウが増えたので、そのストリームのキューを処理
                self.flush_stream_data(sid)?;
                // WINDOW_UPDATE により送信が完了した場合、ストリームを削除する
                self.try_remove_closed_stream(sid);
            }
            // 暗黙的にクローズ済みストリームへの WINDOW_UPDATE は無視
        }

        self.events.push_back(Event::WindowUpdateReceived {
            stream_id: frame.stream_id,
            increment: increment_u32,
        });

        Ok(())
    }

    /// PRIORITY_UPDATE フレームを処理する (RFC 9218 Section 7.1)
    ///
    /// RFC 9218 Section 7.1 では idle ストリームの PRIORITY_UPDATE 数 + active ストリーム数が
    /// SETTINGS_MAX_CONCURRENT_STREAMS を超えてはならない (MUST) と規定されているが、
    /// 主要ブラウザが PRIORITY_UPDATE をほぼ使用しないため実装しない。
    fn handle_priority_update(&mut self, frame: PriorityUpdateFrame) -> Result<()> {
        // RFC 9218 Section 7.1: サーバーは PRIORITY_UPDATE を送信してはならない (MUST NOT)。
        // クライアントが受信した場合はプロトコルエラー
        if self.role == Role::Client {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "client received PRIORITY_UPDATE frame",
            ));
        }

        // RFC 9218 §7.1: Prioritized Stream ID はクライアント開始ストリーム (奇数) でなければならない
        // stream_id = 0 は NonZeroStreamId により構造的に排除済み
        match frame.prioritized_element_id {
            NonZeroStreamId::Client(_) => {}
            NonZeroStreamId::Server(_) => {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    "PRIORITY_UPDATE for non-client-initiated stream",
                ));
            }
        }

        self.events.push_back(Event::PriorityUpdateReceived {
            stream_id: StreamId::from(frame.prioritized_element_id),
            priority_field_value: frame.priority_field_value,
        });

        Ok(())
    }

    /// ストリームが idle 状態かどうかを判定する
    ///
    /// RFC 9113 Section 5.1: マップに存在しないストリームで、
    /// last_recv_stream_id より大きい（または偶数で未使用）ものは idle。
    fn is_idle_stream(&self, stream_id: u32) -> bool {
        if self.streams.contains_key(&stream_id) {
            return false;
        }
        // サーバープッシュ非サポートのため偶数ストリーム ID は常にアイドル
        if stream_id.is_multiple_of(2) {
            return true;
        }
        // last_recv_stream_id よりも大きいストリーム ID はアイドル
        stream_id > self.last_recv_stream_id
    }

    /// マップ内のストリームが Closed 状態かどうかを判定する
    fn is_stream_closed(&self, stream_id: u32) -> bool {
        self.streams
            .get(&stream_id)
            .is_some_and(|stream| stream.state() == StreamState::Closed)
    }

    /// 非 HEADERS フレームのストリーム ID を検証する (RFC 9113 Section 5.1)
    ///
    /// アイドルストリームへの DATA / RST_STREAM / WINDOW_UPDATE は接続エラー。
    /// サーバープッシュ非サポートのため偶数ストリーム ID も拒否する。
    fn check_not_idle_stream(&self, stream_id: u32, frame_type: &str) -> Result<()> {
        if self.streams.contains_key(&stream_id) {
            return Ok(());
        }
        // サーバープッシュ非サポートのため偶数ストリーム ID は常にアイドル
        if stream_id.is_multiple_of(2) {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                format!("{} on idle stream: {}", frame_type, stream_id),
            ));
        }
        // last_recv_stream_id よりも大きいストリーム ID はアイドル
        if stream_id > self.last_recv_stream_id {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                format!("{} on idle stream: {}", frame_type, stream_id),
            ));
        }
        // last_recv_stream_id 以下でマップにないストリームは暗黙的にクローズ済み
        Ok(())
    }

    /// 同時ストリーム数の上限チェック
    fn check_concurrent_streams_limit(&self, stream_id: u32) -> Result<()> {
        if self.streams.contains_key(&stream_id) {
            return Ok(());
        }

        if let Some(max) = self.local_settings.max_concurrent_streams() {
            let current_open = self
                .streams
                .values()
                .filter(|s| !s.state().is_closed())
                .count();
            if current_open >= max as usize {
                return Err(Error::stream_error(
                    ErrorCode::RefusedStream,
                    "max concurrent streams exceeded",
                ));
            }
        }
        Ok(())
    }

    /// フレームを出力バッファに書き込む
    ///
    /// encode が成功した場合のみ encoder 内部バッファにデータが蓄積される。
    /// 失敗時は encoder の状態が変わらないことを前提とし、output_buffer も不変に保つ。
    fn send_frame(&mut self, frame: &Frame) -> Result<()> {
        self.frame_encoder.encode(frame)?;
        self.output_buffer.extend(self.frame_encoder.buffer());
        self.frame_encoder.clear();
        Ok(())
    }
}

/// RFC 9113 Section 8.2.3: 複数の Cookie ヘッダーフィールドを "; " で連結する
///
/// HPACK 展開後に複数の cookie フィールドが存在する場合、
/// non-HTTP/2 コンテキストへ渡す前に 1 つのフィールドに連結しなければならない (MUST)。
pub(crate) fn concatenate_cookies(headers: Vec<HeaderField>) -> Vec<HeaderField> {
    let cookie_count = headers
        .iter()
        .filter(|h| h.name().eq_ignore_ascii_case(b"cookie"))
        .count();
    if cookie_count <= 1 {
        return headers;
    }

    let mut result = Vec::with_capacity(headers.len() - cookie_count + 1);
    let mut cookie_values: Vec<Vec<u8>> = Vec::with_capacity(cookie_count);
    let mut cookie_sensitive = false;

    for header in headers {
        if header.name().eq_ignore_ascii_case(b"cookie") {
            if header.sensitive() {
                cookie_sensitive = true;
            }
            // 空 cookie は RFC 6265 §4.2.1 (cookie-string = cookie-pair *( ";" SP cookie-pair ))
            // の文法外。連結すると末尾 SP が生まれて field-value 規則を侵すため除外する。
            let value = header.value();
            if !value.is_empty() {
                cookie_values.push(value.to_vec());
            }
        } else {
            result.push(header);
        }
    }

    if cookie_values.is_empty() {
        return result;
    }

    let concatenated = cookie_values.join(&b"; "[..]);
    // 連結後の cookie は HPACK decoder 経路で個別 cookie が既に検証済みの値であり、
    // 区切り文字 "; " は ASCII、空 cookie は事前に除外しているため、両端 SP/HTAB は
    // 発生しない。`from_validated_parts` で構築する。
    result.push(HeaderField::from_validated_parts(
        b"cookie".to_vec(),
        concatenated,
        cookie_sensitive,
    ));

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn concatenate_cookies_returns_input_when_cookie_count_is_zero() {
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
        ];
        let input = headers.clone();
        let output = concatenate_cookies(headers);
        assert_eq!(output.len(), input.len());
        for (a, b) in output.iter().zip(input.iter()) {
            assert_eq!(a.name(), b.name());
            assert_eq!(a.value(), b.value());
        }
    }

    #[test]
    fn concatenate_cookies_returns_input_when_cookie_count_is_one() {
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new("cookie", "a=1").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
        ];
        let input = headers.clone();
        let output = concatenate_cookies(headers);
        assert_eq!(output.len(), input.len());
        for (a, b) in output.iter().zip(input.iter()) {
            assert_eq!(a.name(), b.name());
            assert_eq!(a.value(), b.value());
        }
    }

    #[test]
    fn concatenate_cookies_all_empty_cookies_excluded() {
        // 全 cookie が空の場合、出力に cookie ヘッダーが含まれない
        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new("cookie", "").unwrap(),
            HeaderField::new("cookie", "").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
        ];
        let output = concatenate_cookies(headers);
        assert!(
            !output
                .iter()
                .any(|h| h.name().eq_ignore_ascii_case(b"cookie")),
            "全空 cookie 入力に対し出力に cookie が含まれてはならない"
        );
        // 非 cookie のみ残る
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn concatenate_cookies_no_double_separator() {
        // 空 cookie 混在時に連結結果に "; ;" が含まれないことを確認
        let headers = vec![
            HeaderField::new("cookie", "a=1").unwrap(),
            HeaderField::new("cookie", "").unwrap(),
            HeaderField::new("cookie", "b=2").unwrap(),
        ];
        let output = concatenate_cookies(headers);
        let cookie = output.last().unwrap();
        assert_eq!(cookie.name(), b"cookie");
        assert!(
            !cookie.value().windows(3).any(|w| w == b"; ;"),
            "連結結果に二重区切り '; ;' が含まれてはならない"
        );
        assert_eq!(cookie.value(), b"a=1; b=2");
    }

    #[test]
    fn concatenate_cookies_sensitive_propagation() {
        // sensitive フラグは cookie 全体の OR
        let headers = vec![
            HeaderField::new_with_sensitive("cookie", "a=1", true).unwrap(),
            HeaderField::new_with_sensitive("cookie", "b=2", false).unwrap(),
        ];
        let output = concatenate_cookies(headers);
        assert!(output.last().unwrap().sensitive());

        // 全て false なら結果も false
        let headers = vec![
            HeaderField::new_with_sensitive("cookie", "x=1", false).unwrap(),
            HeaderField::new_with_sensitive("cookie", "y=2", false).unwrap(),
        ];
        let output = concatenate_cookies(headers);
        assert!(!output.last().unwrap().sensitive());
    }

    /// cookie 値の Strategy: 空 (1/4 の確率) または 印字可能 ASCII 1..=32 文字
    /// 0x3B (';') を除外: cookie 値に ';' を含むと連結区切り "; " と合わせて
    /// "; ;" パターンが正当に出現しうるため、二重区切り不在の検証が偽陽性になる
    fn cookie_value_strategy() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            1 => Just(Vec::new()),
            3 => prop::collection::vec(
                (0x21u8..=0x7Eu8).prop_filter("';' を除外する", |b| *b != 0x3B),
                1..=32,
            ),
        ]
    }

    proptest! {
        #[test]
        fn prop_concatenate_cookies(
            cookie_count in 0usize..=8,
            non_cookie_count in 0usize..=4,
            cookie_values in prop::collection::vec(cookie_value_strategy(), 8),
            cookie_sensitives in prop::collection::vec(any::<bool>(), 8),
            non_cookie_values in prop::collection::vec(
                prop::collection::vec(0x21u8..=0x7Eu8, 1..=16), 4
            ),
        ) {
            // 入力を構築する
            let mut input = Vec::new();
            for (i, value) in non_cookie_values.iter().take(non_cookie_count).enumerate() {
                let name = format!("x-header-{i}");
                let v = String::from_utf8_lossy(value).to_string();
                input.push(HeaderField::new(&name, &v).unwrap());
            }
            let cookie_input: Vec<_> = cookie_values[..cookie_count]
                .iter()
                .zip(&cookie_sensitives[..cookie_count])
                .collect();
            for (value, sensitive) in cookie_input {
                let v = String::from_utf8_lossy(value).to_string();
                input.push(
                    HeaderField::new_with_sensitive("cookie", &v, *sensitive).unwrap(),
                );
            }

            let output = concatenate_cookies(input.clone());

            if cookie_count <= 1 {
                // ケース A: 早期 return → 出力 = 入力
                prop_assert_eq!(output.len(), input.len());
                for (a, b) in output.iter().zip(input.iter()) {
                    prop_assert_eq!(a.name(), b.name());
                    prop_assert_eq!(a.value(), b.value());
                }
            } else {
                // 非空 cookie の抽出
                let non_empty_cookies: Vec<_> = cookie_values[..cookie_count]
                    .iter()
                    .filter(|v| !v.is_empty())
                    .collect();

                if non_empty_cookies.is_empty() {
                    // ケース B: 全 cookie 空 → 出力に cookie なし
                    prop_assert!(
                        !output.iter().any(|h| h.name().eq_ignore_ascii_case(b"cookie")),
                        "全空 cookie 入力で出力に cookie が含まれてはならない"
                    );
                    prop_assert_eq!(output.len(), non_cookie_count);
                } else {
                    // ケース C: 1 件以上非空 cookie → 末尾に連結 cookie
                    prop_assert_eq!(
                        output.last().unwrap().name(),
                        b"cookie",
                        "連結 cookie は末尾に配置される"
                    );
                    // 非 cookie 順序保持
                    let non_cookie_output: Vec<_> = output[..output.len() - 1].iter().collect();
                    let non_cookie_input: Vec<_> = input
                        .iter()
                        .filter(|h| !h.name().eq_ignore_ascii_case(b"cookie"))
                        .collect();
                    prop_assert_eq!(non_cookie_output.len(), non_cookie_input.len());
                    for (a, b) in non_cookie_output.iter().zip(non_cookie_input.iter()) {
                        prop_assert_eq!(a.name(), b.name());
                        prop_assert_eq!(a.value(), b.value());
                    }
                    // 二重区切り不在
                    let cookie_value = output.last().unwrap().value();
                    prop_assert!(
                        !cookie_value.windows(3).any(|w| w == b"; ;"),
                        "連結結果に二重区切り '; ;' が含まれてはならない"
                    );
                    // sensitive フラグは全 cookie の OR (空 cookie も含む)
                    let expected_sensitive =
                        cookie_sensitives[..cookie_count].iter().any(|s| *s);
                    prop_assert_eq!(output.last().unwrap().sensitive(), expected_sensitive);
                }
            }
        }
    }

    mod stream_id_exhaustion {
        use super::*;
        use crate::frame::{Frame, FrameEncoder, SettingsFrame};
        use crate::stream_id::STREAM_ID_MAX;

        fn encode_frame(frame: &Frame) -> Vec<u8> {
            let mut encoder = FrameEncoder::new();
            encoder.encode(frame).expect("encode must succeed");
            encoder.buffer().to_vec()
        }

        fn setup_active_client() -> Connection {
            let mut client = Connection::client(Limits::default());
            client.initiate().expect("initiate must succeed");

            let settings_frame = Frame::Settings(SettingsFrame::new());
            let settings_bytes = encode_frame(&settings_frame);
            client.feed(&settings_bytes).expect("feed must succeed");
            client.process().expect("process must succeed");
            while client.poll_event().is_some() {}

            client
        }

        fn test_request_headers() -> Vec<HeaderField> {
            vec![
                HeaderField::new(":method", "GET").expect("valid header"),
                HeaderField::new(":path", "/").expect("valid header"),
                HeaderField::new(":scheme", "https").expect("valid header"),
                HeaderField::new(":authority", "example.com").expect("valid header"),
            ]
        }

        /// next_stream_id == STREAM_ID_MAX で最後のストリームが正常に開始される
        #[test]
        fn last_valid_id() {
            let mut client = setup_active_client();
            client.set_next_stream_id(STREAM_ID_MAX);

            let result = client.start_stream(test_request_headers(), true);
            assert!(
                result.is_ok(),
                "STREAM_ID_MAX でのストリーム開始は成功しなければならない"
            );
        }

        /// next_stream_id == STREAM_ID_MAX + 2 で RefusedStream エラーが返される
        #[test]
        fn past_max() {
            let mut client = setup_active_client();
            client.set_next_stream_id(STREAM_ID_MAX + 2);

            let result = client.start_stream(test_request_headers(), true);
            assert!(
                result.is_err(),
                "枯渇後のストリーム開始はエラーでなければならない"
            );
            if let Err(e) = result {
                assert!(e.is_stream_error());
                assert_eq!(e.error_code(), Some(ErrorCode::RefusedStream));
            }
        }

        /// 境界: STREAM_ID_MAX - 2 → 成功、STREAM_ID_MAX → 成功、
        /// STREAM_ID_MAX + 2 → 失敗の 3 段階
        #[test]
        fn boundary_sequence() {
            let mut client = setup_active_client();
            client.set_next_stream_id(STREAM_ID_MAX - 2);

            let result1 = client.start_stream(test_request_headers(), true);
            assert!(
                result1.is_ok(),
                "STREAM_ID_MAX - 2 でのストリーム開始は成功しなければならない"
            );

            let result2 = client.start_stream(test_request_headers(), true);
            assert!(
                result2.is_ok(),
                "STREAM_ID_MAX でのストリーム開始は成功しなければならない"
            );

            let result3 = client.start_stream(test_request_headers(), true);
            assert!(
                result3.is_err(),
                "STREAM_ID_MAX + 2 でのストリーム開始はエラーでなければならない"
            );
            if let Err(e) = result3 {
                assert!(e.is_stream_error());
                assert_eq!(e.error_code(), Some(ErrorCode::RefusedStream));
            }
        }
    }

    mod bounded_closed_streams {
        use super::BoundedClosedStreams;

        #[test]
        fn oldest_entry_evicted_on_overflow() {
            let mut set = BoundedClosedStreams::new();

            // 上限 (10000) まで挿入する
            for i in 0..BoundedClosedStreams::DEFAULT_MAX_SIZE {
                set.insert(i as u32);
            }

            // 上限まで挿入されたエントリは全て含まれている
            for i in 0..BoundedClosedStreams::DEFAULT_MAX_SIZE {
                assert!(
                    set.contains(&(i as u32)),
                    "上限以内のエントリ {i} は含まれていること"
                );
            }

            // 上限 + 1 のエントリを挿入すると、最も古いエントリ (0) が削除される
            set.insert(BoundedClosedStreams::DEFAULT_MAX_SIZE as u32);

            assert!(
                !set.contains(&0),
                "上限超過により最も古いエントリ 0 は削除されていること"
            );
            assert!(set.contains(&1), "2 番目に古いエントリ 1 は残っていること");
            assert!(
                set.contains(&(BoundedClosedStreams::DEFAULT_MAX_SIZE as u32)),
                "最新のエントリは含まれていること"
            );
        }

        #[test]
        fn oldest_entries_evicted_continuously_past_limit() {
            let mut set = BoundedClosedStreams::new();

            // 上限まで挿入
            for i in 0..BoundedClosedStreams::DEFAULT_MAX_SIZE {
                set.insert(i as u32);
            }

            // 上限を超えてさらに 100 件挿入する
            let extra = 100;
            for i in 0..extra {
                set.insert((BoundedClosedStreams::DEFAULT_MAX_SIZE + i) as u32);
            }

            // 0..extra のエントリは全て削除されている
            for i in 0..extra {
                assert!(
                    !set.contains(&(i as u32)),
                    "上限超過により古いエントリ {i} は削除されていること"
                );
            }

            // extra 以降のエントリは残っている
            assert!(
                set.contains(&(extra as u32)),
                "extra 番目のエントリは残っていること"
            );
        }
    }
}
