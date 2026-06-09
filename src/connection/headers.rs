//! HTTP/2 の HEADERS 関連処理
//!
//! HEADERS フレームの送受信、継続処理、ヘッダーリストの検証を行う。

use super::{Connection, ConnectionState, Role, concatenate_cookies};
use crate::error::{Error, ErrorCode, Result};
use crate::event::Event;
use crate::frame::{ContinuationFrame, Frame, HeadersFrame, NonZeroStreamId, StreamId};
use crate::hpack::HeaderField;
use crate::stream::{Stream, StreamState};
use crate::validation;

impl Connection {
    /// レスポンスヘッダーを送信する (サーバー用)
    ///
    /// 既存のストリームにレスポンスヘッダーを送信する。
    pub fn send_response(
        &mut self,
        stream_id: StreamId,
        headers: Vec<HeaderField>,
        end_stream: bool,
    ) -> Result<()> {
        let sid = stream_id.as_u32();

        // RFC 9113 Section 8.5: CONNECT 確立済みストリームでは HEADERS を送信できない
        if let Some(stream) = self.streams.get(&sid)
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
            && let Some(stream) = self.streams.get(&sid)
            && stream.final_response_sent()
        {
            return Err(Error::stream_error(
                ErrorCode::ProtocolError,
                "final response already sent on this stream",
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

        let is_closed = {
            let stream = self
                .streams
                .get_mut(&sid)
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

        let nz_stream_id = stream_id
            .non_zero()
            .expect("response stream ID is always non-zero");
        self.send_header_block(nz_stream_id, encoded_headers, end_stream)?;

        // 送信側で end_stream によりストリームが closed になった場合
        if is_closed {
            self.events.push_back(Event::StreamClosed { stream_id });
            self.closed_streams.insert(sid);
            self.streams.remove(&sid);
        }

        Ok(())
    }

    /// トレーラーヘッダーを送信する
    ///
    /// RFC 9113 Section 8.1: トレーラーは END_STREAM 付きの HEADERS フレームで送信する。
    /// 疑似ヘッダーを含めてはならない。
    ///
    /// サーバーの場合は最終レスポンス送信後にのみ送信可能。
    /// クライアントの場合はリクエストヘッダー送信後に使用する。
    pub fn send_trailers(&mut self, stream_id: StreamId, headers: Vec<HeaderField>) -> Result<()> {
        let sid = stream_id.as_u32();

        // RFC 9113 Section 8.5: CONNECT 確立済みストリームでは HEADERS を送信できない
        if let Some(stream) = self.streams.get(&sid)
            && stream.connect_established()
        {
            return Err(Error::stream_error(
                ErrorCode::ProtocolError,
                "HEADERS not allowed on established CONNECT tunnel",
            ));
        }

        // RFC 9113 Section 8.1: サーバーのトレーラーは最終レスポンス送信後にのみ送信可能
        // クライアントのトレーラーはリクエスト送信後に利用可能（状態機械が検証する）
        if self.role == Role::Server
            && let Some(stream) = self.streams.get(&sid)
            && !stream.final_response_sent()
        {
            return Err(Error::stream_error(
                ErrorCode::ProtocolError,
                "cannot send trailers before final response",
            ));
        }

        // RFC 9113 Section 8.1: トレーラー検証 (疑似ヘッダー禁止、接続固有ヘッダー禁止)
        validation::validate_trailers(&headers)?;

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

        // RFC 9113 Section 8.1: トレーラーは常に END_STREAM を伴う
        let end_stream = true;

        let is_closed = {
            let stream = self
                .streams
                .get_mut(&sid)
                .ok_or_else(|| Error::stream_error(ErrorCode::StreamClosed, "stream not found"))?;

            stream.state_machine_mut().send_headers(end_stream)?;

            stream.state() == StreamState::Closed
        };

        // HEADERS フレームを送信 (必要に応じて CONTINUATION に分割)
        let mut encoded_headers = Vec::new();
        self.hpack_encoder.encode(&mut encoded_headers, &headers);

        let nz_stream_id = stream_id
            .non_zero()
            .expect("trailer stream ID is always non-zero");
        self.send_header_block(nz_stream_id, encoded_headers, end_stream)?;

        // 送信側で end_stream によりストリームが closed になった場合
        if is_closed {
            self.events.push_back(Event::StreamClosed { stream_id });
            self.closed_streams.insert(sid);
            self.streams.remove(&sid);
        }

        Ok(())
    }

    /// HEADERS フレームを処理する
    pub(super) fn handle_headers(&mut self, frame: HeadersFrame) -> Result<()> {
        // RFC 9113 §5.1.1: サーバープッシュ非サポートのため偶数ストリーム ID は拒否
        // stream_id = 0 は NonZeroStreamId により構造的に排除済み
        match frame.stream_id {
            NonZeroStreamId::Client(_) => {}
            NonZeroStreamId::Server(_) => {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    format!(
                        "server-initiated stream not supported: {}",
                        frame.stream_id.as_u32()
                    ),
                ));
            }
        }

        let sid = frame.stream_id.as_u32();

        // RFC 9113 Section 6.8: GOAWAY 送信後の新規ストリームチェック
        if matches!(self.state, ConnectionState::GoawaySent)
            && sid > self.last_recv_stream_id
            && !self.streams.contains_key(&sid)
        {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "new stream after GOAWAY sent",
            ));
        }

        // RFC 9113 Section 5.1: クローズ済みストリームへの遅延 HEADERS は
        // HPACK 状態を更新してから破棄する必要がある (MUST)。
        // closed_streams で追跡しているため、未開設ストリームの非単調 ID とは区別できる。
        let is_previously_closed = self.closed_streams.contains(&sid);

        if !is_previously_closed {
            // RFC 9113 Section 5.1.1: 新規ストリームの単調増加チェック
            if !self.streams.contains_key(&sid) && sid <= self.last_recv_stream_id {
                return Err(Error::connection_error(
                    ErrorCode::ProtocolError,
                    format!(
                        "stream ID {} not greater than last received {}",
                        sid, self.last_recv_stream_id
                    ),
                ));
            }

            // RFC 9113 Section 8.5: CONNECT 確立済みストリームでは HEADERS を拒否する
            // Extended CONNECT (:protocol 付き) は通常のストリームとして動作するため対象外
            if let Some(stream) = self.streams.get(&sid)
                && stream.connect_established()
            {
                return Err(Error::stream_error(
                    ErrorCode::ProtocolError,
                    "HEADERS not allowed on established CONNECT tunnel",
                ));
            }

            // 同時ストリーム数の上限チェック
            if !self.streams.contains_key(&sid) {
                self.check_concurrent_streams_limit(sid)?;
            }
        }

        if sid > self.last_recv_stream_id {
            self.last_recv_stream_id = sid;
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
            if is_previously_closed || self.is_stream_closed(sid) {
                return Ok(());
            }

            self.process_headers(StreamId::from(frame.stream_id), headers, frame.end_stream)?;

            // RFC 9113 Section 5.4.1: ヘッダーの HPACK デコードと検証が
            // 両方成功した場合のみ GOAWAY 用の last_successful_stream_id を更新する
            if sid > self.last_successful_stream_id {
                self.last_successful_stream_id = sid;
            }
        } else {
            // ヘッダーブロック継続
            // RFC 9113 Section 6.2: END_STREAM フラグは最初の HEADERS フレームで決まる
            self.header_continuation_stream = Some(sid);
            self.header_block_fragment = frame.header_block_fragment;
            self.header_end_stream = frame.end_stream;
            self.check_header_block_fragment_size()?;
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
        let sid = stream_id.as_u32();

        // RFC 9113 Section 6.5.2: 受信ヘッダーリストサイズの上限 (SETTINGS_MAX_HEADER_LIST_SIZE)
        // は HPACK デコーダがデコード中に逐次検査し、超過時に COMPRESSION_ERROR の接続エラーに
        // する (src/hpack/decoder.rs)。ここではデコード済みヘッダーが上限以下であることが保証される。

        // RFC 9113 Section 8.1: トレーラーは疑似ヘッダーを含まない
        // 疑似ヘッダーの有無でトレーラーかどうかを判定
        let has_pseudo_header = headers.iter().any(|h| h.name().starts_with(b":"));

        // RFC 9113 Section 8.1 / 8.3.1: 初回 HEADERS には疑似ヘッダーが必須
        if !has_pseudo_header {
            let is_initial = !self
                .streams
                .get(&sid)
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
            && let Some(stream) = self.streams.get(&sid)
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
                    if has_protocol && !self.local_settings.enable_connect_protocol() {
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
                        let stream = self.streams.entry(sid).or_insert_with(|| {
                            Stream::new(
                                stream_id,
                                self.remote_settings.initial_window_size().get(),
                                self.local_settings.initial_window_size().get(),
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
                    if is_2xx && let Some(stream) = self.streams.get_mut(&sid) {
                        let is_connect = stream.request_method().is_some_and(|m| m == b"CONNECT");
                        if is_connect && !stream.has_protocol() {
                            stream.set_connect_established(true);
                        }
                    }

                    // RFC 9110 Section 6.4.1: 204/304 レスポンスおよび HEAD リクエストへの
                    // レスポンスはコンテンツを持たない (RFC 9113 Section 8.1.1)。
                    // 空 DATA は許容されるが、内容を持つ DATA は malformed として扱う。
                    let is_no_content = status == Some(b"204") || status == Some(b"304");
                    if let Some(stream) = self.streams.get_mut(&sid) {
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

        // RFC 9113 Section 6.5.2 / Section 6.9.2: 受信したストリームの場合、
        // 送信ウィンドウはリモートの initial_window_size、
        // 受信ウィンドウはローカルの initial_window_size で初期化する
        let is_closed = {
            let stream = self.streams.entry(sid).or_insert_with(|| {
                Stream::new(
                    stream_id,
                    self.remote_settings.initial_window_size().get(),
                    self.local_settings.initial_window_size().get(),
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
            self.closed_streams.insert(sid);
            self.streams.remove(&sid);
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

    /// CONTINUATION フレームを処理する
    pub(super) fn handle_continuation(&mut self, frame: ContinuationFrame) -> Result<()> {
        // RFC 9113 Section 6.10: CONTINUATION は先行する HEADERS の後にのみ許可
        let expected_stream_id = self.header_continuation_stream.ok_or_else(|| {
            Error::connection_error(
                ErrorCode::ProtocolError,
                "CONTINUATION frame received without preceding HEADERS",
            )
        })?;

        if frame.stream_id.as_u32() != expected_stream_id {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "CONTINUATION frame stream ID mismatch",
            ));
        }

        self.header_block_fragment
            .extend_from_slice(&frame.header_block_fragment);
        self.check_header_block_fragment_size()?;

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
            self.process_headers(StreamId::from_wire(expected_stream_id), headers, end_stream)?;

            // RFC 9113 Section 5.4.1: ヘッダーの HPACK デコードと検証が
            // 両方成功した場合のみ GOAWAY 用の last_successful_stream_id を更新する
            if expected_stream_id > self.last_successful_stream_id {
                self.last_successful_stream_id = expected_stream_id;
            }
        }

        Ok(())
    }

    /// ヘッダーブロックを送信する (HEADERS + CONTINUATION)
    ///
    /// エンコード済みヘッダーを MAX_FRAME_SIZE に従って分割し、
    /// 必要に応じて CONTINUATION フレームを使用する。
    pub(super) fn send_header_block(
        &mut self,
        stream_id: NonZeroStreamId,
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

        let max_frame_size = self.remote_settings.max_frame_size().get() as usize;

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
    pub(super) fn calculate_header_list_size(headers: &[HeaderField]) -> usize {
        headers.iter().map(HeaderField::size).sum()
    }

    /// 累積中のヘッダーブロックフラグメント (HEADERS + CONTINUATION) のサイズ上限を検査する
    ///
    /// RFC 9113 Section 6.10: CONTINUATION フレームの個数には上限がなく、`max_frame_size` は
    /// フレーム単体のサイズしか制限しない。そのため累積フラグメントを無制限に成長させて
    /// メモリを枯渇させる攻撃 (CVE-2016-8740 系) が成立する。
    ///
    /// 圧縮後のフラグメントサイズは概ねデコード後のヘッダーリストサイズと同オーダーに収まる
    /// (Dynamic Table Size Update のみ例外的に出力に寄与しない) ため、上限には
    /// `SETTINGS_MAX_HEADER_LIST_SIZE` (ローカル設定) を保守的に流用する。`None` (無制限) の場合は
    /// 検査しない。
    ///
    /// RFC 9113 Section 4.3: field block を展開せずに打ち切るため、接続エラーは
    /// COMPRESSION_ERROR にする (MUST)。
    /// `SETTINGS_MAX_HEADER_LIST_SIZE` が未設定の場合の絶対的な上限 (64MB)
    const MAX_HEADER_BLOCK_FRAGMENT_SIZE: usize = 64 * 1024 * 1024;

    fn check_header_block_fragment_size(&self) -> Result<()> {
        let effective_max = self
            .local_settings
            .max_header_list_size()
            .map(|s| s as usize)
            .unwrap_or(Self::MAX_HEADER_BLOCK_FRAGMENT_SIZE);

        if self.header_block_fragment.len() > effective_max {
            return Err(Error::connection_error(
                ErrorCode::CompressionError,
                "accumulated header block fragment exceeds size limit",
            ));
        }
        Ok(())
    }
}
