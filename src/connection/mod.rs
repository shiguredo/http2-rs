//! HTTP/2 接続管理
//!
//! Sans I/O パターンで HTTP/2 接続を管理する。

use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::{Error, ErrorCode, Result};
use crate::event::Event;
use crate::flow_control::{FlowControl, MAX_WINDOW_SIZE};
use crate::frame::{
    CONNECTION_STREAM_ID, ContinuationFrame, DataFrame, Frame, FrameDecoder, FrameEncoder,
    GoawayFrame, HeadersFrame, PingFrame, PriorityUpdateFrame, RstStreamFrame, SettingsFrame,
    StreamId, WindowUpdateFrame,
};
use crate::hpack::{Decoder as HpackDecoder, Encoder as HpackEncoder, HeaderField};
use crate::limits::Limits;
use crate::settings::Settings;
use crate::stream::{Stream, StreamState};
use crate::validation;

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
    streams: HashMap<StreamId, Stream>,
    /// クローズ済みストリーム ID の集合
    ///
    /// RFC 9113 Section 5.1: マップから削除されたストリームの ID を追跡する。
    /// RST_STREAM 送信後や END_STREAM による正常クローズ後に到着する遅延フレームを
    /// 接続エラーではなく破棄として処理するために使用する。
    closed_streams: HashSet<StreamId>,
    /// 次のストリーム ID
    next_stream_id: StreamId,
    /// 最後に受信したストリーム ID
    last_recv_stream_id: StreamId,
    /// 最後に正常処理が完了したストリーム ID (GOAWAY 用)
    /// RFC 9113 Section 5.4.1: GOAWAY には正常に受信した最後のストリーム ID を載せる
    last_successful_stream_id: StreamId,
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
    header_continuation_stream: Option<StreamId>,
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
}

impl Connection {
    /// 新しい接続を生成する
    #[must_use]
    pub fn new(role: Role, limits: Limits) -> Self {
        let mut local_settings = Settings::new();
        local_settings.header_table_size = limits.header_table_size;
        if let Some(max) = limits.max_concurrent_streams {
            local_settings.max_concurrent_streams = Some(max);
        }
        local_settings.initial_window_size = limits.initial_window_size;
        local_settings.max_frame_size = limits.max_frame_size;
        local_settings.max_header_list_size = limits.max_header_list_size;
        local_settings.enable_connect_protocol = limits.enable_connect_protocol;
        local_settings.no_rfc7540_priorities = limits.no_rfc7540_priorities;
        local_settings.wt_initial = limits.wt_initial.clone();

        let next_stream_id = match role {
            Role::Client => 1,
            Role::Server => 2,
        };

        Self {
            role,
            state: ConnectionState::WaitingPreface,
            local_settings,
            remote_settings: Settings::new(),
            flow_control: FlowControl::new(limits.connection_window_size),
            streams: HashMap::new(),
            closed_streams: HashSet::new(),
            next_stream_id,
            last_recv_stream_id: 0,
            last_successful_stream_id: 0,
            hpack_encoder: HpackEncoder::new(limits.header_table_size as usize),
            hpack_decoder: HpackDecoder::new(limits.header_table_size as usize),
            frame_decoder: FrameDecoder::new(limits.max_frame_size),
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
            pending_table_size_update: None,
            preface_buffer: Vec::new(),
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
    /// 送信済みの SETTINGS に対応する設定。WebTransport 初期設定 (`wt_initial`) を
    /// 含む拡張 SETTINGS もここから参照できる。
    #[must_use]
    pub const fn local_settings(&self) -> &Settings {
        &self.local_settings
    }

    /// リモート設定を取得する
    ///
    /// ピアから受信して ACK した SETTINGS に対応する設定。
    /// WebTransport セッションを張る前に `enable_connect_protocol` や
    /// `wt_initial` を確認する用途で使用する。
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

    /// 接続プリフェイスを送信する（クライアント）
    ///
    /// RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 0 以外に設定できない。
    /// そのため、サーバーの場合は ENABLE_PUSH を送信しない。
    pub fn initiate(&mut self) -> Result<()> {
        use crate::settings::SettingId;

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
            if self.role == Role::Server && setting.id == SettingId::EnablePush.as_u16() {
                continue;
            }
            settings_frame.add_setting(setting);
        }
        self.send_frame(&Frame::Settings(settings_frame))?;
        self.pending_settings_count += 1;
        self.preface_sent = true;

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
        use crate::settings::SettingId;

        let mut settings_frame = SettingsFrame::new();
        for setting in self.local_settings.to_settings_list() {
            // RFC 9113 Section 8.4: サーバーは ENABLE_PUSH を 1 に設定できない
            if self.role == Role::Server && setting.id == SettingId::EnablePush.as_u16() {
                continue;
            }
            settings_frame.add_setting(setting);
        }
        self.send_frame(&Frame::Settings(settings_frame))?;
        self.pending_settings_count += 1;

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
                        self.reset_stream(stream_id, error_code)?;
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
        if let Some(max) = self.remote_settings.max_concurrent_streams {
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
        if has_protocol && !self.remote_settings.enable_connect_protocol {
            return Err(Error::protocol_error(
                "cannot send :protocol without peer's SETTINGS_ENABLE_CONNECT_PROTOCOL=1",
            ));
        }

        // RFC 9113 Section 10.5.1: 送信ヘッダーリストサイズの上限チェック
        if let Some(max_size) = self.remote_settings.max_header_list_size {
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

        let stream_id = self.next_stream_id;
        self.next_stream_id += 2;

        // RFC 9113 Section 5.2: 送信ウィンドウはリモートの initial_window_size、
        // 受信ウィンドウはローカルの initial_window_size で初期化する
        let mut stream = Stream::new(
            stream_id,
            self.remote_settings.initial_window_size,
            self.local_settings.initial_window_size,
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

        self.streams.insert(stream_id, stream);

        // HEADERS フレームを送信 (必要に応じて CONTINUATION に分割)
        let mut encoded_headers = Vec::new();
        self.hpack_encoder.encode(&mut encoded_headers, &headers);

        self.send_header_block(stream_id, encoded_headers, end_stream)?;

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
        // ストリームの存在確認と状態遷移
        {
            let stream = self
                .streams
                .get_mut(&stream_id)
                .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

            // 状態チェック（end_stream=false でも送信可能な状態か検証する）
            stream.state_machine_mut().send_data(end_stream)?;
        }

        // データをキューに追加
        self.queue_data(stream_id, data, end_stream)?;

        // キューから送信可能な分を送信
        self.flush_stream_data(stream_id)?;

        // RFC 9113 Section 5.1: END_STREAM 付きフレームを実際に送信完了した場合のみ
        // ストリームを closed として削除する
        self.try_remove_closed_stream(stream_id);

        Ok(())
    }

    /// ストリームのデータをキューに追加する
    fn queue_data(&mut self, stream_id: StreamId, data: Vec<u8>, end_stream: bool) -> Result<()> {
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
    fn flush_stream_data(&mut self, stream_id: StreamId) -> Result<()> {
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
                let max_frame = self.remote_settings.max_frame_size as usize;

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

                // end_stream を送信したらフラグをクリア
                if end_stream {
                    stream.set_pending_end_stream(false);
                }
            }

            // DATA フレームを送信
            let data_frame = DataFrame::new(stream_id, data).with_end_stream(end_stream);
            self.send_frame(&Frame::Data(data_frame))?;

            if end_stream || remaining_after == 0 {
                break;
            }
        }

        // RFC 9113 Section 6.9.1: 空 DATA + END_STREAM を送信する
        // ループから break で抜けた場合（buffer_len == 0 && pending_end_stream）
        if let Some(stream) = self.streams.get_mut(&stream_id)
            && stream.pending_end_stream()
            && stream.send_buffer().is_empty()
        {
            stream.set_pending_end_stream(false);
            let data_frame = DataFrame::new(stream_id, vec![]).with_end_stream(true);
            self.send_frame(&Frame::Data(data_frame))?;
        }

        Ok(())
    }

    /// 全ストリームのキューからデータを送信する
    fn flush_all_stream_data(&mut self) -> Result<()> {
        // 送信待ちデータまたは送信待ち END_STREAM があるストリームを収集
        let stream_ids: Vec<StreamId> = self
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
    fn try_remove_closed_stream(&mut self, stream_id: StreamId) {
        let should_remove = self.streams.get(&stream_id).is_some_and(|stream| {
            stream.state() == StreamState::Closed
                && stream.send_buffer().is_empty()
                && !stream.pending_end_stream()
        });
        if should_remove {
            self.events.push_back(Event::StreamClosed { stream_id });
            self.closed_streams.insert(stream_id);
            self.streams.remove(&stream_id);
        }
    }

    /// ストリームをリセットする
    pub fn reset_stream(&mut self, stream_id: StreamId, error_code: ErrorCode) -> Result<()> {
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.state_machine_mut().send_rst_stream();
        }

        let rst_frame = RstStreamFrame::new(stream_id, error_code.as_u32());
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
        let goaway_frame = GoawayFrame::new(self.last_successful_stream_id, error_code.as_u32())
            .with_debug_data(debug_data);
        self.send_frame(&Frame::Goaway(goaway_frame))?;
        self.state = ConnectionState::GoawaySent;
        Ok(())
    }

    /// WINDOW_UPDATE を送信する
    pub fn send_window_update(&mut self, stream_id: StreamId, increment: u32) -> Result<()> {
        // RFC 9113 Section 6.9: increment は 1 以上 2^31-1 以下でなければならない
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
        if stream_id == CONNECTION_STREAM_ID {
            self.flow_control.add_recv_window(increment)?;
        } else if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.flow_control_mut().add_recv_window(increment)?;
        }

        let window_update_frame = WindowUpdateFrame::new(stream_id, increment);
        self.send_frame(&Frame::WindowUpdate(window_update_frame))?;

        Ok(())
    }

    /// レスポンスヘッダーを送信する (サーバー用)
    ///
    /// 既存のストリームにレスポンスヘッダーを送信する。
    pub fn send_response(
        &mut self,
        stream_id: StreamId,
        headers: Vec<HeaderField>,
        end_stream: bool,
    ) -> Result<()> {
        // RFC 9113 Section 8.5: CONNECT 確立済みストリームでは HEADERS を送信できない
        if let Some(stream) = self.streams.get(&stream_id)
            && stream.connect_established()
        {
            return Err(Error::stream_error(
                ErrorCode::ProtocolError,
                "HEADERS not allowed on established CONNECT tunnel",
            ));
        }

        // RFC 9113 Section 8.3.2: 送信前にレスポンスヘッダーの妥当性を検証する
        validation::validate_response_headers(&headers)?;

        let is_informational = headers
            .iter()
            .find(|h| h.name() == validation::pseudo_headers::STATUS)
            .is_some_and(|h| h.value().len() == 3 && h.value()[0] == b'1');

        // RFC 9113 Section 8.1: 1xx 情報レスポンスに END_STREAM を付けてはならない
        // END_STREAM 付きの情報レスポンスは malformed である (Section 8.1.1)
        if end_stream && is_informational {
            return Err(Error::stream_error(
                ErrorCode::ProtocolError,
                "informational response (1xx) with END_STREAM is malformed",
            ));
        }

        // RFC 9113 Section 8.1: 最終レスポンス (非 1xx) は 1 回のみ送信可能
        // 最終レスポンス送信後の追加 HEADERS (END_STREAM なし) は malformed
        if !is_informational
            && let Some(stream) = self.streams.get(&stream_id)
            && stream.final_response_sent()
        {
            return Err(Error::stream_error(
                ErrorCode::ProtocolError,
                "final response already sent on this stream",
            ));
        }

        // RFC 9113 Section 10.5.1: 送信ヘッダーリストサイズの上限チェック
        if let Some(max_size) = self.remote_settings.max_header_list_size {
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

        let is_closed = {
            let stream = self
                .streams
                .get_mut(&stream_id)
                .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

            stream.state_machine_mut().send_headers(end_stream)?;

            // RFC 9113 Section 8.1: 最終レスポンス送信済みフラグを設定する
            if !is_informational {
                stream.set_final_response_sent(true);
            }

            // RFC 9113 Section 8.5: サーバーが通常 CONNECT に 2xx を返す場合、
            // CONNECT トンネル確立済みフラグを設定する
            let status = headers
                .iter()
                .find(|h| h.name() == validation::pseudo_headers::STATUS)
                .map(HeaderField::value);
            let is_2xx = status.is_some_and(|s| s.len() == 3 && s[0] == b'2');
            if is_2xx {
                let is_connect = stream.request_method().is_some_and(|m| m == b"CONNECT");
                if is_connect && !stream.has_protocol() {
                    stream.set_connect_established(true);
                }
            }

            end_stream && stream.state() == StreamState::Closed
        };

        // HEADERS フレームを送信 (必要に応じて CONTINUATION に分割)
        let mut encoded_headers = Vec::new();
        self.hpack_encoder.encode(&mut encoded_headers, &headers);

        self.send_header_block(stream_id, encoded_headers, end_stream)?;

        // 送信側で end_stream によりストリームが closed になった場合
        if is_closed {
            self.events.push_back(Event::StreamClosed { stream_id });
            self.closed_streams.insert(stream_id);
            self.streams.remove(&stream_id);
        }

        Ok(())
    }

    /// トレーラーヘッダーを送信する
    ///
    /// RFC 9113 Section 8.1: トレーラーは END_STREAM 付きの HEADERS フレームで送信する。
    /// 疑似ヘッダーを含めてはならない。
    pub fn send_trailers(&mut self, stream_id: StreamId, headers: Vec<HeaderField>) -> Result<()> {
        // RFC 9113 Section 8.5: CONNECT 確立済みストリームでは HEADERS を送信できない
        // RFC 9113 Section 8.1: トレーラーは最終レスポンス送信後にのみ送信可能
        if let Some(stream) = self.streams.get(&stream_id) {
            if stream.connect_established() {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    "HEADERS not allowed on established CONNECT tunnel",
                ));
            }
            if !stream.final_response_sent() {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    "cannot send trailers before final response",
                ));
            }
        }

        // RFC 9113 Section 8.1: トレーラー検証 (疑似ヘッダー禁止、接続固有ヘッダー禁止)
        validation::validate_trailers(&headers)?;

        // RFC 9113 Section 10.5.1: 送信ヘッダーリストサイズの上限チェック
        if let Some(max_size) = self.remote_settings.max_header_list_size {
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

        // RFC 9113 Section 8.1: トレーラーは常に END_STREAM を伴う
        let end_stream = true;

        let is_closed = {
            let stream = self
                .streams
                .get_mut(&stream_id)
                .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

            stream.state_machine_mut().send_headers(end_stream)?;

            stream.state() == StreamState::Closed
        };

        // HEADERS フレームを送信 (必要に応じて CONTINUATION に分割)
        let mut encoded_headers = Vec::new();
        self.hpack_encoder.encode(&mut encoded_headers, &headers);

        self.send_header_block(stream_id, encoded_headers, end_stream)?;

        // 送信側で end_stream によりストリームが closed になった場合
        if is_closed {
            self.events.push_back(Event::StreamClosed { stream_id });
            self.closed_streams.insert(stream_id);
            self.streams.remove(&stream_id);
        }

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
            if !matches!(&frame, Frame::Settings(sf) if !sf.ack) {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    "first frame must be SETTINGS",
                ));
            }
        }

        // ヘッダーブロック継続中は CONTINUATION のみ許可
        if let Some(expected_stream_id) = self.header_continuation_stream {
            match &frame {
                Frame::Continuation(cf) if cf.stream_id == expected_stream_id => {
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
                if header.stream_id != CONNECTION_STREAM_ID
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
        // RFC 9113 Section 5.1: アイドルストリームへのフレームは接続エラー
        self.check_not_idle_stream(frame.stream_id, "DATA")?;

        // RFC 9113 Section 6.1: フロー制御はペイロード全体に適用
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
        if self.is_stream_closed(frame.stream_id) || !self.streams.contains_key(&frame.stream_id) {
            return Ok(());
        }

        let is_closed = {
            let stream = self
                .streams
                .get_mut(&frame.stream_id)
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

        self.events.push_back(Event::DataReceived {
            stream_id: frame.stream_id,
            data: frame.data,
            end_stream: frame.end_stream,
        });

        if is_closed {
            self.events.push_back(Event::StreamClosed {
                stream_id: frame.stream_id,
            });
            self.closed_streams.insert(frame.stream_id);
            self.streams.remove(&frame.stream_id);
        }

        Ok(())
    }

    /// HEADERS フレームを処理する
    fn handle_headers(&mut self, frame: HeadersFrame) -> Result<()> {
        // RFC 9113 Section 5.1.1: ストリーム ID の偶奇チェック
        self.validate_stream_id_parity(frame.stream_id)?;

        // RFC 9113 Section 6.8: GOAWAY 送信後の新規ストリームチェック
        if matches!(self.state, ConnectionState::GoawaySent)
            && frame.stream_id > self.last_recv_stream_id
            && !self.streams.contains_key(&frame.stream_id)
        {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "new stream after GOAWAY sent",
            ));
        }

        // RFC 9113 Section 5.1: クローズ済みストリームへの遅延 HEADERS は
        // HPACK 状態を更新してから破棄する必要がある (MUST)。
        // closed_streams で追跡しているため、未開設ストリームの非単調 ID とは区別できる。
        let is_previously_closed = self.closed_streams.contains(&frame.stream_id);

        if !is_previously_closed {
            // RFC 9113 Section 5.1.1: 新規ストリームの単調増加チェック
            if !self.streams.contains_key(&frame.stream_id)
                && frame.stream_id <= self.last_recv_stream_id
            {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    format!(
                        "stream ID {} not greater than last received {}",
                        frame.stream_id, self.last_recv_stream_id
                    ),
                ));
            }

            // RFC 9113 Section 8.5: CONNECT 確立済みストリームでは HEADERS を拒否する
            // Extended CONNECT (:protocol 付き) は通常のストリームとして動作するため対象外
            if let Some(stream) = self.streams.get(&frame.stream_id)
                && stream.connect_established()
            {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    "HEADERS not allowed on established CONNECT tunnel",
                ));
            }

            // 同時ストリーム数の上限チェック
            if !self.streams.contains_key(&frame.stream_id) {
                self.check_concurrent_streams_limit(frame.stream_id)?;
            }
        }

        if frame.stream_id > self.last_recv_stream_id {
            self.last_recv_stream_id = frame.stream_id;
        }

        if frame.end_headers {
            // 完全なヘッダーブロック: HPACK 状態を必ず更新する
            let headers = self
                .hpack_decoder
                .decode(&frame.header_block_fragment)
                .map_err(|e| {
                    Error::connection_error(
                        ErrorCode::CompressionError,
                        format!("HPACK decode error: {}", e.reason),
                    )
                })?;

            // RFC 9113 Section 5.1: Closed 状態のストリームへの HEADERS は
            // HPACK 状態を更新した上で破棄する (マップから削除済みの場合を含む)
            if is_previously_closed || self.is_stream_closed(frame.stream_id) {
                return Ok(());
            }

            self.process_headers(frame.stream_id, headers, frame.end_stream)?;

            // RFC 9113 Section 5.4.1: ヘッダーの HPACK デコードと検証が
            // 両方成功した場合のみ GOAWAY 用の last_successful_stream_id を更新する
            if frame.stream_id > self.last_successful_stream_id {
                self.last_successful_stream_id = frame.stream_id;
            }
        } else {
            // ヘッダーブロック継続
            // RFC 9113 Section 6.2: END_STREAM フラグは最初の HEADERS フレームで決まる
            self.header_continuation_stream = Some(frame.stream_id);
            self.header_block_fragment = frame.header_block_fragment;
            self.header_end_stream = frame.end_stream;
        }

        Ok(())
    }

    /// ヘッダーを処理する
    fn process_headers(
        &mut self,
        stream_id: StreamId,
        headers: Vec<HeaderField>,
        end_stream: bool,
    ) -> Result<()> {
        // RFC 9113 Section 10.5.1: ヘッダーリストサイズの上限チェック
        if let Some(max_size) = self.local_settings.max_header_list_size {
            let header_list_size = Self::calculate_header_list_size(&headers);
            if header_list_size > max_size as usize {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    format!(
                        "header list size {} exceeds SETTINGS_MAX_HEADER_LIST_SIZE {}",
                        header_list_size, max_size
                    ),
                ));
            }
        }

        // RFC 9113 Section 8.1: トレーラーは疑似ヘッダーを含まない
        // 疑似ヘッダーの有無でトレーラーかどうかを判定
        let has_pseudo_header = headers.iter().any(|h| h.name().starts_with(b":"));

        // RFC 9113 Section 8.1 / 8.3.1: 初回 HEADERS には疑似ヘッダーが必須
        if !has_pseudo_header {
            let is_initial = !self
                .streams
                .get(&stream_id)
                .is_some_and(|s| s.initial_headers_received());
            if is_initial {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    "initial HEADERS must contain pseudo-headers",
                ));
            }
        }

        // RFC 9113 Section 8.1: 初回ヘッダー受信後の疑似ヘッダー検証
        // サーバー: リクエストヘッダーは 1 回のみ許可
        // クライアント: 最終レスポンス後の疑似ヘッダーは不正
        if has_pseudo_header
            && let Some(stream) = self.streams.get(&stream_id)
            && stream.initial_headers_received()
        {
            return Err(Error::stream_error(
                ErrorCode::ProtocolError,
                "pseudo-headers in non-initial HEADERS",
            ));
        }

        let is_trailer = !has_pseudo_header;

        if is_trailer {
            // RFC 9113 Section 8.1: トレーラー検証
            validation::validate_trailers(&headers)?;
            if !end_stream {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    "trailers must be sent with END_STREAM",
                ));
            }
        } else {
            // ヘッダー検証
            // サーバーはリクエストを受信、クライアントはレスポンスを受信
            match self.role {
                Role::Server => {
                    validation::validate_request_headers(&headers)?;

                    // RFC 8441: Extended CONNECT のネゴシエーションチェック
                    // :protocol を含むリクエストは ENABLE_CONNECT_PROTOCOL=1 を
                    // 送信済みの場合のみ許可
                    let protocol_value = headers
                        .iter()
                        .find(|h| h.name() == validation::pseudo_headers::PROTOCOL)
                        .map(|h| h.value().to_vec());
                    let has_protocol = protocol_value.is_some();
                    if has_protocol && !self.local_settings.enable_connect_protocol {
                        return Err(Error::stream_error(
                            ErrorCode::ProtocolError,
                            "received :protocol without ENABLE_CONNECT_PROTOCOL",
                        ));
                    }

                    // リクエストメソッドと :protocol を記録する
                    // CONNECT フレーム制限 (修正 1) と Content-Length 例外 (修正 2) に使用
                    let method = headers
                        .iter()
                        .find(|h| h.name() == validation::pseudo_headers::METHOD)
                        .map(|h| h.value().to_vec());
                    if let Some(method) = method {
                        let stream = self.streams.entry(stream_id).or_insert_with(|| {
                            Stream::new(
                                stream_id,
                                self.remote_settings.initial_window_size,
                                self.local_settings.initial_window_size,
                            )
                        });
                        stream.set_request_method(method);
                        stream.set_has_protocol(has_protocol);
                        if let Some(proto) = protocol_value {
                            stream.set_protocol(proto);
                        }
                    }
                }
                Role::Client => {
                    validation::validate_response_headers(&headers)?;

                    // クライアント側: レスポンスステータスの判定
                    let status = headers
                        .iter()
                        .find(|h| h.name() == validation::pseudo_headers::STATUS)
                        .map(HeaderField::value);

                    // 通常 CONNECT の 2xx レスポンスで CONNECT 確立
                    let is_2xx = status.is_some_and(|s| s.len() == 3 && s[0] == b'2');
                    if is_2xx && let Some(stream) = self.streams.get_mut(&stream_id) {
                        let is_connect = stream.request_method().is_some_and(|m| m == b"CONNECT");
                        if is_connect && !stream.has_protocol() {
                            stream.set_connect_established(true);
                        }
                    }

                    // RFC 9110 Section 6.4.1: 204/304 レスポンスおよび HEAD リクエストへの
                    // レスポンスはコンテンツを持たない (RFC 9113 Section 8.1.1)。
                    // 空 DATA は許容されるが、内容を持つ DATA は malformed として扱う。
                    let is_no_content = status == Some(b"204") || status == Some(b"304");
                    if let Some(stream) = self.streams.get_mut(&stream_id) {
                        let is_head = stream.request_method().is_some_and(|m| m == b"HEAD");
                        if is_no_content || is_head {
                            stream.set_no_content(true);
                        }
                    }
                }
            }
        }

        // RFC 9113 Section 8.1.1: Content-Length ヘッダーを抽出 (トレーラーでは不要)
        let content_length = if is_trailer {
            None
        } else {
            Self::extract_content_length(&headers)?
        };

        // RFC 9113 Section 5.2: 受信したストリームの場合、
        // 送信ウィンドウはリモートの initial_window_size、
        // 受信ウィンドウはローカルの initial_window_size で初期化する
        let is_closed = {
            let stream = self.streams.entry(stream_id).or_insert_with(|| {
                Stream::new(
                    stream_id,
                    self.remote_settings.initial_window_size,
                    self.local_settings.initial_window_size,
                )
            });

            stream.state_machine_mut().recv_headers(end_stream)?;

            // RFC 9113 Section 8.2.3: 複数の Cookie ヘッダーを連結する
            let headers = concatenate_cookies(headers);

            if is_trailer {
                // トレーラーの場合はイベントを TrailersReceived に
                self.events.push_back(Event::TrailersReceived {
                    stream_id,
                    trailers: headers,
                });
            } else {
                stream.set_headers(headers.clone());
                stream.set_expected_content_length(content_length);

                // RFC 9113 Section 8.1: 初回ヘッダー受信フラグを設定する
                if !stream.initial_headers_received() {
                    match self.role {
                        Role::Server => {
                            // サーバーはリクエストヘッダーを 1 回のみ受信する
                            stream.set_initial_headers_received(true);
                        }
                        Role::Client => {
                            // 1xx 情報レスポンスは最終レスポンスではないため、
                            // フラグを設定しない
                            let is_informational = headers
                                .iter()
                                .find(|h| h.name() == validation::pseudo_headers::STATUS)
                                .is_some_and(|h| h.value().len() == 3 && h.value()[0] == b'1');
                            // RFC 9113 Section 8.1: END_STREAM 付きの情報レスポンス (1xx) は
                            // malformed である (Section 8.1.1)
                            if is_informational && end_stream {
                                return Err(Error::stream_error(
                                    ErrorCode::ProtocolError,
                                    "informational response (1xx) with END_STREAM is malformed",
                                ));
                            }
                            if !is_informational {
                                stream.set_initial_headers_received(true);
                            }
                        }
                    }
                }

                // RFC 9113 Section 8.1.1: END_STREAM の場合、Content-Length は 0 でなければならない
                // ただし、コンテンツを持たないレスポンス (204/304/HEAD) は例外
                if end_stream && content_length.is_some_and(|len| len != 0) {
                    let skip_check = match self.role {
                        Role::Client => {
                            // 204/304 レスポンスはコンテンツを持たない
                            let status = headers
                                .iter()
                                .find(|h| h.name() == validation::pseudo_headers::STATUS)
                                .map(HeaderField::value);
                            let is_no_content = status == Some(b"204") || status == Some(b"304");
                            // HEAD レスポンスはコンテンツを持たない
                            let is_head = stream.request_method().is_some_and(|m| m == b"HEAD");
                            is_no_content || is_head
                        }
                        Role::Server => false,
                    };
                    if !skip_check {
                        return Err(Error::stream_error(
                            ErrorCode::ProtocolError,
                            "content-length mismatch: expected 0 with END_STREAM",
                        ));
                    }
                }

                let protocol = stream.protocol().map(|p| p.to_vec());
                self.events.push_back(Event::HeadersReceived {
                    stream_id,
                    headers,
                    end_stream,
                    protocol,
                });
            }

            end_stream && stream.state() == StreamState::Closed
        };

        if is_closed {
            self.events.push_back(Event::StreamClosed { stream_id });
            self.closed_streams.insert(stream_id);
            self.streams.remove(&stream_id);
        }

        Ok(())
    }

    /// Content-Length ヘッダーを抽出する
    ///
    /// RFC 9110 Section 8.6: 複数の Content-Length が異なる値を持つ場合は malformed。
    fn extract_content_length(headers: &[HeaderField]) -> Result<Option<u64>> {
        let mut content_length: Option<u64> = None;
        for header in headers {
            if header.name() == b"content-length" {
                let value_str = std::str::from_utf8(header.value()).map_err(|_| {
                    Error::stream_error(ErrorCode::ProtocolError, "invalid content-length encoding")
                })?;
                let len: u64 = value_str.parse().map_err(|_| {
                    Error::stream_error(ErrorCode::ProtocolError, "invalid content-length value")
                })?;
                if let Some(existing) = content_length {
                    if existing != len {
                        return Err(Error::stream_error(
                            ErrorCode::ProtocolError,
                            format!(
                                "conflicting content-length values: {} and {}",
                                existing, len
                            ),
                        ));
                    }
                } else {
                    content_length = Some(len);
                }
            }
        }
        Ok(content_length)
    }

    /// RST_STREAM フレームを処理する
    fn handle_rst_stream(&mut self, frame: RstStreamFrame) -> Result<()> {
        // RFC 9113 Section 5.1: アイドルストリームへのフレームは接続エラー
        self.check_not_idle_stream(frame.stream_id, "RST_STREAM")?;

        match self.streams.get_mut(&frame.stream_id) {
            Some(stream) => {
                stream.state_machine_mut().recv_rst_stream();
                self.events.push_back(Event::StreamReset {
                    stream_id: frame.stream_id,
                    error_code: ErrorCode::from_u32(frame.error_code),
                });
                self.closed_streams.insert(frame.stream_id);
                self.streams.remove(&frame.stream_id);
            }
            None => {
                // 暗黙的にクローズ済みストリームへの RST_STREAM は無視
            }
        }
        Ok(())
    }

    /// SETTINGS フレームを処理する
    fn handle_settings(&mut self, frame: SettingsFrame) -> Result<()> {
        if frame.ack {
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
            let old_initial_window_size = self.remote_settings.initial_window_size;

            // HEADER_TABLE_SIZE の変更を追跡
            let old_header_table_size = self.remote_settings.header_table_size;

            for setting in &frame.settings {
                // RFC 9113 Section 8.4: サーバーはクライアントに ENABLE_PUSH=1 を送信できない
                if self.role == Role::Client
                    && setting.id == crate::settings::SettingId::EnablePush.as_u16()
                    && setting.value == 1
                {
                    return Err(Error::connection_error(
                        ErrorCode::ProtocolError,
                        "server sent ENABLE_PUSH=1 to client",
                    ));
                }

                // RFC 9218 Section 2.1: NO_RFC7540_PRIORITIES は接続中に変更できない
                if setting.id == crate::settings::SettingId::NoRfc7540Priorities.as_u16() {
                    let new_value = setting.value == 1;
                    if let Some(initial_value) = self.initial_no_rfc7540_priorities {
                        if new_value != initial_value {
                            return Err(Error::connection_error(
                                ErrorCode::ProtocolError,
                                "NO_RFC7540_PRIORITIES cannot be changed after initial setting",
                            ));
                        }
                    } else {
                        self.initial_no_rfc7540_priorities = Some(new_value);
                    }
                }

                self.remote_settings.apply(*setting).map_err(|e| {
                    // RFC 9113 §6.5.2: 各 SETTINGS パラメータ違反のエラーコード
                    #[allow(unreachable_patterns)]
                    let error_code = match e {
                        crate::settings::SettingError::InitialWindowSizeOutOfRange { .. } => {
                            ErrorCode::FlowControlError
                        }
                        crate::settings::SettingError::EnablePushNotBoolean { .. }
                        | crate::settings::SettingError::MaxFrameSizeOutOfRange { .. }
                        | crate::settings::SettingError::EnableConnectProtocolNotBoolean {
                            ..
                        }
                        | crate::settings::SettingError::NoRfc7540PrioritiesNotBoolean { .. } => {
                            ErrorCode::ProtocolError
                        }
                        _ => ErrorCode::ProtocolError,
                    };
                    Error::connection_error(error_code, e.to_string())
                })?;
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
            let new_header_table_size = self.remote_settings.header_table_size;
            if new_header_table_size != old_header_table_size {
                let new_min = match self.pending_table_size_update {
                    Some((existing_min, _)) => existing_min.min(new_header_table_size),
                    None => new_header_table_size,
                };
                self.pending_table_size_update = Some((new_min, new_header_table_size));
            }

            // SETTINGS_INITIAL_WINDOW_SIZE が変更された場合、既存ストリームを更新
            let new_initial_window_size = self.remote_settings.initial_window_size;
            if new_initial_window_size != old_initial_window_size {
                self.update_stream_windows(new_initial_window_size)?;
            }

            // HPACK エンコーダーのテーブルサイズを更新
            self.hpack_encoder
                .set_max_table_size(self.remote_settings.header_table_size as usize);

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
            last_stream_id: frame.last_stream_id,
            error_code: ErrorCode::from_u32(frame.error_code),
            debug_data: frame.debug_data,
        });

        Ok(())
    }

    /// WINDOW_UPDATE フレームを処理する
    fn handle_window_update(&mut self, frame: WindowUpdateFrame) -> Result<()> {
        if frame.stream_id == CONNECTION_STREAM_ID {
            self.flow_control
                .recv_window_update(frame.window_size_increment)?;
            // 接続レベルのウィンドウが増えたので、全ストリームのキューを処理
            self.flush_all_stream_data()?;
        } else {
            // RFC 9113 Section 5.1: アイドルストリームへのフレームは接続エラー
            self.check_not_idle_stream(frame.stream_id, "WINDOW_UPDATE")?;

            if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
                // RFC 9113 Section 5.1: Closed 状態のストリームへの
                // WINDOW_UPDATE は無視する
                if stream.state() == StreamState::Closed {
                    return Ok(());
                }
                // RFC 9113 Section 6.9.1: ストリームレベルのウィンドウオーバーフローは
                // RST_STREAM(FLOW_CONTROL_ERROR) で処理する（接続エラーではない）
                if let Err(e) = stream
                    .flow_control_mut()
                    .recv_window_update(frame.window_size_increment)
                {
                    if e.is_connection_error() {
                        self.reset_stream(frame.stream_id, ErrorCode::FlowControlError)?;
                        return Ok(());
                    }
                    return Err(e);
                }
                // ストリームレベルのウィンドウが増えたので、そのストリームのキューを処理
                self.flush_stream_data(frame.stream_id)?;
                // WINDOW_UPDATE により送信が完了した場合、ストリームを削除する
                self.try_remove_closed_stream(frame.stream_id);
            }
            // 暗黙的にクローズ済みストリームへの WINDOW_UPDATE は無視
        }

        self.events.push_back(Event::WindowUpdateReceived {
            stream_id: frame.stream_id,
            increment: frame.window_size_increment,
        });

        Ok(())
    }

    /// CONTINUATION フレームを処理する
    fn handle_continuation(&mut self, frame: crate::frame::ContinuationFrame) -> Result<()> {
        // RFC 9113 Section 6.10: CONTINUATION は先行する HEADERS の後にのみ許可
        let expected_stream_id = self.header_continuation_stream.ok_or_else(|| {
            Error::connection_error(
                ErrorCode::ProtocolError,
                "CONTINUATION frame received without preceding HEADERS",
            )
        })?;

        if frame.stream_id != expected_stream_id {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "CONTINUATION frame stream ID mismatch",
            ));
        }

        self.header_block_fragment
            .extend_from_slice(&frame.header_block_fragment);

        if frame.end_headers {
            self.header_continuation_stream = None;
            let headers = self
                .hpack_decoder
                .decode(&self.header_block_fragment)
                .map_err(|e| {
                    Error::connection_error(
                        ErrorCode::CompressionError,
                        format!("HPACK decode error: {}", e.reason),
                    )
                })?;
            self.header_block_fragment.clear();

            // RFC 9113 Section 5.1: Closed 状態のストリームへの HEADERS は
            // HPACK 状態を更新した上で破棄する (マップから削除済みの場合を含む)
            let is_previously_closed = self.closed_streams.contains(&expected_stream_id);
            if is_previously_closed || self.is_stream_closed(expected_stream_id) {
                self.header_end_stream = false;
                return Ok(());
            }

            // RFC 9113 Section 6.2: END_STREAM は最初の HEADERS フレームで決まる
            let end_stream = self.header_end_stream;
            self.header_end_stream = false;
            self.process_headers(expected_stream_id, headers, end_stream)?;

            // RFC 9113 Section 5.4.1: ヘッダーの HPACK デコードと検証が
            // 両方成功した場合のみ GOAWAY 用の last_successful_stream_id を更新する
            if expected_stream_id > self.last_successful_stream_id {
                self.last_successful_stream_id = expected_stream_id;
            }
        }

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

        // RFC 9218 Section 7.1: Prioritized Stream ID はクライアント開始ストリーム (奇数) でなければならない
        // クライアント開始ストリームは奇数
        if frame.prioritized_element_id.is_multiple_of(2) {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "PRIORITY_UPDATE for non-client-initiated stream",
            ));
        }

        self.events.push_back(Event::PriorityUpdateReceived {
            stream_id: frame.prioritized_element_id,
            priority_field_value: frame.priority_field_value,
        });

        Ok(())
    }

    /// ストリーム ID の偶奇チェック (RFC 9113 Section 5.1.1)
    ///
    /// HEADERS フレームで使用する。HPACK デコード前に呼び出しても安全な検証のみ行う。
    /// 偶奇違反は接続エラーで接続を閉じるため、HPACK 状態は不要。
    fn validate_stream_id_parity(&self, stream_id: StreamId) -> Result<()> {
        let is_client_initiated = stream_id % 2 == 1;
        match self.role {
            Role::Server => {
                if !is_client_initiated {
                    return Err(Error::connection_error(
                        ErrorCode::ProtocolError,
                        format!("server received even stream ID: {}", stream_id),
                    ));
                }
            }
            Role::Client => {
                if !is_client_initiated {
                    return Err(Error::connection_error(
                        ErrorCode::ProtocolError,
                        format!("client received server-initiated stream: {}", stream_id),
                    ));
                }
            }
        }
        Ok(())
    }

    /// ストリームが idle 状態かどうかを判定する
    ///
    /// RFC 9113 Section 5.1: マップに存在しないストリームで、
    /// last_recv_stream_id より大きい（または偶数で未使用）ものは idle。
    fn is_idle_stream(&self, stream_id: StreamId) -> bool {
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
    fn is_stream_closed(&self, stream_id: StreamId) -> bool {
        self.streams
            .get(&stream_id)
            .is_some_and(|stream| stream.state() == StreamState::Closed)
    }

    /// 非 HEADERS フレームのストリーム ID を検証する (RFC 9113 Section 5.1)
    ///
    /// アイドルストリームへの DATA / RST_STREAM / WINDOW_UPDATE は接続エラー。
    /// サーバープッシュ非サポートのため偶数ストリーム ID も拒否する。
    fn check_not_idle_stream(&self, stream_id: StreamId, frame_type: &str) -> Result<()> {
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
    fn check_concurrent_streams_limit(&self, stream_id: StreamId) -> Result<()> {
        if self.streams.contains_key(&stream_id) {
            return Ok(());
        }

        if let Some(max) = self.local_settings.max_concurrent_streams {
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

    /// ヘッダーブロックを送信する (HEADERS + CONTINUATION)
    ///
    /// エンコード済みヘッダーを MAX_FRAME_SIZE に従って分割し、
    /// 必要に応じて CONTINUATION フレームを使用する。
    fn send_header_block(
        &mut self,
        stream_id: StreamId,
        encoded_headers: Vec<u8>,
        end_stream: bool,
    ) -> Result<()> {
        // RFC 7541 Section 4.2: SETTINGS_HEADER_TABLE_SIZE 変更を受信した場合、
        // 次のヘッダーブロックの先頭に Dynamic Table Size Update をエンコードする。
        // ヘッダーブロック間に複数回変化した場合、最小値と最終値の両方を送出する。
        let encoded_headers =
            if let Some((min_size, final_size)) = self.pending_table_size_update.take() {
                let mut header_block = Vec::new();
                // 最小値と最終値が異なる場合、最小値を先に送出する
                if min_size != final_size {
                    self.hpack_encoder
                        .encode_size_update(&mut header_block, min_size as usize);
                }
                // 最終値を送出する (最小値 == 最終値の場合は 1 回のみ)
                self.hpack_encoder
                    .encode_size_update(&mut header_block, final_size as usize);
                header_block.extend_from_slice(&encoded_headers);
                header_block
            } else {
                encoded_headers
            };

        let max_frame_size = self.remote_settings.max_frame_size as usize;

        if encoded_headers.len() <= max_frame_size {
            // 単一の HEADERS フレームで送信可能
            let headers_frame = HeadersFrame::new(stream_id, encoded_headers)
                .with_end_stream(end_stream)
                .with_end_headers(true);
            self.send_frame(&Frame::Headers(headers_frame))?;
        } else {
            // HEADERS + CONTINUATION に分割
            let mut remaining = &encoded_headers[..];

            // 最初の HEADERS フレーム
            let first_chunk = &remaining[..max_frame_size];
            remaining = &remaining[max_frame_size..];
            let headers_frame = HeadersFrame::new(stream_id, first_chunk.to_vec())
                .with_end_stream(end_stream)
                .with_end_headers(false);
            self.send_frame(&Frame::Headers(headers_frame))?;

            // CONTINUATION フレーム
            while !remaining.is_empty() {
                let chunk_size = remaining.len().min(max_frame_size);
                let chunk = &remaining[..chunk_size];
                remaining = &remaining[chunk_size..];
                let is_last = remaining.is_empty();
                let continuation_frame =
                    ContinuationFrame::new(stream_id, chunk.to_vec()).with_end_headers(is_last);
                self.send_frame(&Frame::Continuation(continuation_frame))?;
            }
        }
        Ok(())
    }

    /// ヘッダーリストのサイズを計算する (RFC 9113 Section 6.5.2)
    ///
    /// 各ヘッダーフィールドのサイズは名前と値のオクテット長に 32 を加えたもの。
    fn calculate_header_list_size(headers: &[HeaderField]) -> usize {
        headers.iter().map(HeaderField::size).sum()
    }

    /// フレームを出力バッファに書き込む
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
fn concatenate_cookies(headers: Vec<HeaderField>) -> Vec<HeaderField> {
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
