//! nghttp2 セッション

use crate::error::{Error, Result, check_nghttp2, check_nghttp2_with_value};
use crate::types::{ErrorCode, Header, Http2Event, StreamId};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::ptr;

/// セッションの役割
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRole {
    /// クライアント
    Client,
    /// サーバー
    Server,
}

/// ストリームごとの送信バッファ
struct StreamSendBuffer {
    /// 送信待ちデータ
    data: VecDeque<u8>,
    /// ストリーム終了フラグ
    end_stream: bool,
}

/// nghttp2 セッション
pub struct Session {
    /// 内部セッションポインタ
    session: *mut nghttp2_sys::nghttp2_session,
    /// 役割
    role: SessionRole,
    /// イベントキュー
    events: VecDeque<Http2Event>,
    /// 出力バッファ
    output: Vec<u8>,
    /// ストリームごとのヘッダー蓄積バッファ
    pending_headers: HashMap<StreamId, Vec<Header>>,
    /// ストリームごとのデータ蓄積バッファ（DATA チャンクを蓄積し、フレーム完了時にイベント発行）
    pending_data: HashMap<StreamId, Vec<u8>>,
    /// ストリームごとの送信バッファ（data provider read callback から読み出される）
    send_buffers: HashMap<StreamId, StreamSendBuffer>,
}

// Session は Send + Sync を実装（内部で適切に同期を取る）
unsafe impl Send for Session {}
unsafe impl Sync for Session {}

impl Session {
    /// 新しいクライアントセッションを作成
    pub fn client() -> Result<Self> {
        Self::new(SessionRole::Client)
    }

    /// 新しいサーバーセッションを作成
    pub fn server() -> Result<Self> {
        Self::new(SessionRole::Server)
    }

    /// 新しいセッションを作成
    fn new(role: SessionRole) -> Result<Self> {
        let mut session: *mut nghttp2_sys::nghttp2_session = ptr::null_mut();

        // コールバック構造体を作成
        let mut callbacks: *mut nghttp2_sys::nghttp2_session_callbacks = ptr::null_mut();
        unsafe {
            check_nghttp2(nghttp2_sys::nghttp2_session_callbacks_new(&mut callbacks))?;

            // コールバックを設定
            nghttp2_sys::nghttp2_session_callbacks_set_on_frame_recv_callback(
                callbacks,
                Some(on_frame_recv_callback),
            );
            nghttp2_sys::nghttp2_session_callbacks_set_on_data_chunk_recv_callback(
                callbacks,
                Some(on_data_chunk_recv_callback),
            );
            nghttp2_sys::nghttp2_session_callbacks_set_on_stream_close_callback(
                callbacks,
                Some(on_stream_close_callback),
            );
            nghttp2_sys::nghttp2_session_callbacks_set_on_header_callback(
                callbacks,
                Some(on_header_callback),
            );
            nghttp2_sys::nghttp2_session_callbacks_set_on_begin_headers_callback(
                callbacks,
                Some(on_begin_headers_callback),
            );

            // セッションを作成
            let result = match role {
                SessionRole::Client => nghttp2_sys::nghttp2_session_client_new(
                    &mut session,
                    callbacks,
                    ptr::null_mut(),
                ),
                SessionRole::Server => nghttp2_sys::nghttp2_session_server_new(
                    &mut session,
                    callbacks,
                    ptr::null_mut(),
                ),
            };

            nghttp2_sys::nghttp2_session_callbacks_del(callbacks);
            check_nghttp2(result)?;
        }

        Ok(Self {
            session,
            role,
            events: VecDeque::new(),
            output: Vec::new(),
            pending_headers: HashMap::new(),
            pending_data: HashMap::new(),
            send_buffers: HashMap::new(),
        })
    }

    /// セッションの役割を取得
    pub fn role(&self) -> SessionRole {
        self.role
    }

    /// ユーザーデータを設定
    pub fn set_user_data(&mut self) {
        unsafe {
            nghttp2_sys::nghttp2_session_set_user_data(
                self.session,
                self as *mut Session as *mut c_void,
            );
        }
    }

    /// 入力データを処理
    pub fn recv(&mut self, data: &[u8]) -> Result<usize> {
        self.set_user_data();
        let result = unsafe {
            nghttp2_sys::nghttp2_session_mem_recv(self.session, data.as_ptr(), data.len())
        };
        check_nghttp2_with_value(result as i32).map(|v| v as usize)
    }

    /// 出力データを生成
    pub fn send(&mut self) -> Result<Vec<u8>> {
        self.output.clear();

        loop {
            let mut data_ptr: *const u8 = ptr::null();
            let len = unsafe { nghttp2_sys::nghttp2_session_mem_send(self.session, &mut data_ptr) };

            if len < 0 {
                return Err(Error::from_nghttp2(len as i32));
            }
            if len == 0 {
                break;
            }

            unsafe {
                let slice = std::slice::from_raw_parts(data_ptr, len as usize);
                self.output.extend_from_slice(slice);
            }
        }

        Ok(std::mem::take(&mut self.output))
    }

    /// イベントを取得
    pub fn poll_event(&mut self) -> Option<Http2Event> {
        self.events.pop_front()
    }

    /// SETTINGS フレームを送信
    pub fn submit_settings(&mut self, settings: &[(u16, u32)]) -> Result<()> {
        let iv: Vec<nghttp2_sys::nghttp2_settings_entry> = settings
            .iter()
            .map(|(id, value)| nghttp2_sys::nghttp2_settings_entry {
                settings_id: *id as i32,
                value: *value,
            })
            .collect();

        unsafe {
            check_nghttp2(nghttp2_sys::nghttp2_submit_settings(
                self.session,
                nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_NONE as u8,
                iv.as_ptr(),
                iv.len(),
            ))
        }
    }

    /// リクエストを送信（クライアント）
    pub fn submit_request(
        &mut self,
        headers: &[Header],
        data: Option<&[u8]>,
        end_stream: bool,
    ) -> Result<StreamId> {
        let nva = headers_to_nva(headers);

        let stream_id = if data.is_none() && end_stream {
            // データなし + end_stream: data_provider を NULL にして END_STREAM 付き HEADERS のみ送信
            unsafe {
                nghttp2_sys::nghttp2_submit_request2(
                    self.session,
                    ptr::null(),
                    nva.as_ptr(),
                    nva.len(),
                    ptr::null(),
                    ptr::null_mut(),
                )
            }
        } else {
            // データあり、またはデータなし + end_stream=false (後で submit_data でデータ追加)
            let data_prd = build_data_provider2();
            let stream_id = unsafe {
                nghttp2_sys::nghttp2_submit_request2(
                    self.session,
                    ptr::null(),
                    nva.as_ptr(),
                    nva.len(),
                    &data_prd,
                    ptr::null_mut(),
                )
            };

            if stream_id > 0 {
                let mut send_buf = StreamSendBuffer {
                    data: VecDeque::new(),
                    end_stream: false,
                };

                if let Some(d) = data {
                    send_buf.data.extend(d);
                    send_buf.end_stream = end_stream;
                }
                // data が None + end_stream=false の場合は空バッファで deferred 状態

                self.send_buffers.insert(stream_id, send_buf);
            }

            stream_id
        };

        if stream_id < 0 {
            return Err(Error::from_nghttp2(stream_id));
        }

        Ok(stream_id)
    }

    /// レスポンスを送信（サーバー）
    pub fn submit_response(
        &mut self,
        stream_id: StreamId,
        headers: &[Header],
        end_stream: bool,
    ) -> Result<()> {
        let nva = headers_to_nva(headers);

        if end_stream {
            // end_stream=true: data_provider を NULL にして END_STREAM 付き HEADERS のみ送信
            unsafe {
                check_nghttp2(nghttp2_sys::nghttp2_submit_response2(
                    self.session,
                    stream_id,
                    nva.as_ptr(),
                    nva.len(),
                    ptr::null(),
                ))?;
            }
        } else {
            // end_stream=false: data_provider2 を構築 (後で submit_data でデータ追加)
            let data_prd = build_data_provider2();
            unsafe {
                check_nghttp2(nghttp2_sys::nghttp2_submit_response2(
                    self.session,
                    stream_id,
                    nva.as_ptr(),
                    nva.len(),
                    &data_prd,
                ))?;
            }

            self.send_buffers.insert(
                stream_id,
                StreamSendBuffer {
                    data: VecDeque::new(),
                    end_stream: false,
                },
            );
        }

        Ok(())
    }

    /// DATA を送信
    ///
    /// send_buffers にデータを追加し、deferred 状態を解除する。
    /// 実際のデータ送信は次回の `send()` 呼び出し時に read callback 経由で行われる。
    pub fn submit_data(
        &mut self,
        stream_id: StreamId,
        data: &[u8],
        end_stream: bool,
    ) -> Result<()> {
        let send_buf = self
            .send_buffers
            .entry(stream_id)
            .or_insert_with(|| StreamSendBuffer {
                data: VecDeque::new(),
                end_stream: false,
            });

        send_buf.data.extend(data);
        if end_stream {
            send_buf.end_stream = true;
        }

        // deferred 状態を解除してデータ送信を再開
        unsafe {
            check_nghttp2(nghttp2_sys::nghttp2_session_resume_data(
                self.session,
                stream_id,
            ))?;
        }

        Ok(())
    }

    /// RST_STREAM を送信
    pub fn submit_rst_stream(&mut self, stream_id: StreamId, error_code: ErrorCode) -> Result<()> {
        unsafe {
            check_nghttp2(nghttp2_sys::nghttp2_submit_rst_stream(
                self.session,
                nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_NONE as u8,
                stream_id,
                error_code.as_u32(),
            ))
        }
    }

    /// GOAWAY を送信
    pub fn submit_goaway(
        &mut self,
        last_stream_id: StreamId,
        error_code: ErrorCode,
        debug_data: &[u8],
    ) -> Result<()> {
        unsafe {
            check_nghttp2(nghttp2_sys::nghttp2_submit_goaway(
                self.session,
                nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_NONE as u8,
                last_stream_id,
                error_code.as_u32(),
                debug_data.as_ptr(),
                debug_data.len(),
            ))
        }
    }

    /// PING を送信
    pub fn submit_ping(&mut self, opaque_data: &[u8; 8]) -> Result<()> {
        unsafe {
            check_nghttp2(nghttp2_sys::nghttp2_submit_ping(
                self.session,
                nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_NONE as u8,
                opaque_data.as_ptr(),
            ))
        }
    }

    /// WINDOW_UPDATE を送信
    pub fn submit_window_update(&mut self, stream_id: StreamId, increment: i32) -> Result<()> {
        unsafe {
            check_nghttp2(nghttp2_sys::nghttp2_submit_window_update(
                self.session,
                nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_NONE as u8,
                stream_id,
                increment,
            ))
        }
    }

    /// セッションが送信すべきデータを持っているか
    pub fn want_write(&self) -> bool {
        unsafe { nghttp2_sys::nghttp2_session_want_write(self.session) != 0 }
    }

    /// セッションが受信すべきデータを待っているか
    pub fn want_read(&self) -> bool {
        unsafe { nghttp2_sys::nghttp2_session_want_read(self.session) != 0 }
    }

    /// イベントを追加（コールバックから呼ばれる）
    fn push_event(&mut self, event: Http2Event) {
        self.events.push_back(event);
    }

    /// ヘッダーを蓄積（コールバックから呼ばれる）
    fn push_header(&mut self, stream_id: StreamId, header: Header) {
        self.pending_headers
            .entry(stream_id)
            .or_default()
            .push(header);
    }

    /// 蓄積したヘッダーを取得（コールバックから呼ばれる）
    fn take_headers(&mut self, stream_id: StreamId) -> Vec<Header> {
        self.pending_headers.remove(&stream_id).unwrap_or_default()
    }

    /// データを蓄積（コールバックから呼ばれる）
    fn push_data(&mut self, stream_id: StreamId, data: &[u8]) {
        self.pending_data
            .entry(stream_id)
            .or_default()
            .extend_from_slice(data);
    }

    /// 蓄積したデータを取得（コールバックから呼ばれる）
    fn take_data(&mut self, stream_id: StreamId) -> Vec<u8> {
        self.pending_data.remove(&stream_id).unwrap_or_default()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            nghttp2_sys::nghttp2_session_del(self.session);
        }
    }
}

/// Header を nghttp2_nv に変換
fn headers_to_nva(headers: &[Header]) -> Vec<nghttp2_sys::nghttp2_nv> {
    headers
        .iter()
        .map(|h| {
            let flags = if h.sensitive {
                nghttp2_sys::nghttp2_nv_flag_NGHTTP2_NV_FLAG_NO_INDEX as u8
            } else {
                nghttp2_sys::nghttp2_nv_flag_NGHTTP2_NV_FLAG_NONE as u8
            };
            nghttp2_sys::nghttp2_nv {
                name: h.name.as_ptr() as *mut u8,
                value: h.value.as_ptr() as *mut u8,
                namelen: h.name.len(),
                valuelen: h.value.len(),
                flags,
            }
        })
        .collect()
}

/// ユーザーデータから Session を取得
unsafe fn get_session<'a>(user_data: *mut c_void) -> Option<&'a mut Session> {
    if user_data.is_null() {
        None
    } else {
        // SAFETY: user_data は Session へのポインタとして設定されている
        unsafe { Some(&mut *(user_data as *mut Session)) }
    }
}

/// data provider2 を構築
fn build_data_provider2() -> nghttp2_sys::nghttp2_data_provider2 {
    nghttp2_sys::nghttp2_data_provider2 {
        source: nghttp2_sys::nghttp2_data_source {
            ptr: ptr::null_mut(),
        },
        read_callback: Some(data_source_read_callback),
    }
}

/// data provider read callback (nghttp2_data_source_read_callback2)
///
/// nghttp2 が DATA フレームを送信する際に呼び出される。
/// send_buffers からストリーム ID に対応するデータを読み出し buf にコピーする。
extern "C" fn data_source_read_callback(
    _session: *mut nghttp2_sys::nghttp2_session,
    stream_id: i32,
    buf: *mut u8,
    length: usize,
    data_flags: *mut u32,
    _source: *mut nghttp2_sys::nghttp2_data_source,
    user_data: *mut c_void,
) -> isize {
    unsafe {
        let Some(session) = get_session(user_data) else {
            return nghttp2_sys::nghttp2_error_NGHTTP2_ERR_CALLBACK_FAILURE as isize;
        };

        let Some(send_buf) = session.send_buffers.get_mut(&stream_id) else {
            // バッファがない場合は deferred (後で submit_data で追加される)
            return nghttp2_sys::nghttp2_error_NGHTTP2_ERR_DEFERRED as isize;
        };

        if send_buf.data.is_empty() {
            if send_buf.end_stream {
                // データなし + end_stream: EOF を設定
                *data_flags |= nghttp2_sys::nghttp2_data_flag_NGHTTP2_DATA_FLAG_EOF;
                return 0;
            }
            // データなし + end_stream でない: deferred (追加データ待ち)
            return nghttp2_sys::nghttp2_error_NGHTTP2_ERR_DEFERRED as isize;
        }

        // バッファからデータを読み出し
        let copy_len = length.min(send_buf.data.len());
        let buf_slice = std::slice::from_raw_parts_mut(buf, copy_len);
        for (i, byte) in send_buf.data.drain(..copy_len).enumerate() {
            buf_slice[i] = byte;
        }

        // 残りデータがなく end_stream の場合は EOF
        if send_buf.data.is_empty() && send_buf.end_stream {
            *data_flags |= nghttp2_sys::nghttp2_data_flag_NGHTTP2_DATA_FLAG_EOF;
        }

        copy_len as isize
    }
}

// ============================================================================
// コールバック関数
// ============================================================================

/// ヘッダー受信開始コールバック
extern "C" fn on_begin_headers_callback(
    _session: *mut nghttp2_sys::nghttp2_session,
    frame: *const nghttp2_sys::nghttp2_frame,
    _user_data: *mut c_void,
) -> libc::c_int {
    let _ = frame;
    // ヘッダー受信開始時の処理（必要に応じて実装）
    0
}

/// ヘッダー受信コールバック
extern "C" fn on_header_callback(
    _session: *mut nghttp2_sys::nghttp2_session,
    frame: *const nghttp2_sys::nghttp2_frame,
    name: *const u8,
    namelen: usize,
    value: *const u8,
    valuelen: usize,
    _flags: u8,
    user_data: *mut c_void,
) -> libc::c_int {
    unsafe {
        if let Some(session) = get_session(user_data) {
            let frame = &*frame;
            let stream_id = frame.hd.stream_id;

            let name_slice = std::slice::from_raw_parts(name, namelen);
            let value_slice = std::slice::from_raw_parts(value, valuelen);

            let header = Header::new(name_slice.to_vec(), value_slice.to_vec());

            // ヘッダーをストリームごとに蓄積
            session.push_header(stream_id, header);
        }
    }
    0
}

/// フレーム受信コールバック
extern "C" fn on_frame_recv_callback(
    _session: *mut nghttp2_sys::nghttp2_session,
    frame: *const nghttp2_sys::nghttp2_frame,
    user_data: *mut c_void,
) -> libc::c_int {
    unsafe {
        if let Some(session) = get_session(user_data) {
            let frame = &*frame;
            let frame_type = frame.hd.type_ as u32;

            match frame_type {
                nghttp2_sys::nghttp2_frame_type_NGHTTP2_SETTINGS => {
                    let ack =
                        (frame.hd.flags & nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_ACK as u8) != 0;
                    session.push_event(Http2Event::SettingsReceived { ack });
                }
                nghttp2_sys::nghttp2_frame_type_NGHTTP2_PING => {
                    let ack =
                        (frame.hd.flags & nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_ACK as u8) != 0;
                    let mut opaque_data = [0u8; 8];
                    opaque_data.copy_from_slice(&frame.ping.opaque_data);
                    session.push_event(Http2Event::PingReceived { opaque_data, ack });
                }
                nghttp2_sys::nghttp2_frame_type_NGHTTP2_GOAWAY => {
                    let goaway = &frame.goaway;
                    let debug_data = if goaway.opaque_data.is_null() || goaway.opaque_data_len == 0
                    {
                        Vec::new()
                    } else {
                        std::slice::from_raw_parts(goaway.opaque_data, goaway.opaque_data_len)
                            .to_vec()
                    };
                    session.push_event(Http2Event::GoawayReceived {
                        last_stream_id: goaway.last_stream_id,
                        error_code: ErrorCode::from_u32(goaway.error_code),
                        debug_data,
                    });
                }
                nghttp2_sys::nghttp2_frame_type_NGHTTP2_WINDOW_UPDATE => {
                    session.push_event(Http2Event::WindowUpdateReceived {
                        stream_id: frame.hd.stream_id,
                        increment: frame.window_update.window_size_increment as u32,
                    });
                }
                nghttp2_sys::nghttp2_frame_type_NGHTTP2_HEADERS => {
                    let stream_id = frame.hd.stream_id;
                    let end_stream = (frame.hd.flags
                        & nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_END_STREAM as u8)
                        != 0;
                    // 蓄積したヘッダーを取り出して HeadersReceived イベントとして発行
                    let headers = session.take_headers(stream_id);
                    session.push_event(Http2Event::HeadersReceived {
                        stream_id,
                        headers,
                        end_stream,
                    });
                }
                nghttp2_sys::nghttp2_frame_type_NGHTTP2_DATA => {
                    let stream_id = frame.hd.stream_id;
                    let end_stream = (frame.hd.flags
                        & nghttp2_sys::nghttp2_flag_NGHTTP2_FLAG_END_STREAM as u8)
                        != 0;
                    // 蓄積したデータを取り出して DataReceived イベントとして発行
                    let data = session.take_data(stream_id);
                    if !data.is_empty() || end_stream {
                        session.push_event(Http2Event::DataReceived {
                            stream_id,
                            data,
                            end_stream,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    0
}

/// データチャンク受信コールバック
///
/// DATA フレームのペイロードを受信するたびに呼ばれる。
/// データを蓄積し、フレーム受信コールバックで END_STREAM を確認してからイベントを発行する。
extern "C" fn on_data_chunk_recv_callback(
    _session: *mut nghttp2_sys::nghttp2_session,
    _flags: u8,
    stream_id: i32,
    data: *const u8,
    len: usize,
    user_data: *mut c_void,
) -> libc::c_int {
    unsafe {
        if let Some(session) = get_session(user_data) {
            let data_slice = std::slice::from_raw_parts(data, len);
            // データを蓄積（フレーム完了時に on_frame_recv_callback でイベント発行）
            session.push_data(stream_id, data_slice);
        }
    }
    0
}

/// ストリームクローズコールバック
extern "C" fn on_stream_close_callback(
    _session: *mut nghttp2_sys::nghttp2_session,
    stream_id: i32,
    error_code: u32,
    user_data: *mut c_void,
) -> libc::c_int {
    unsafe {
        if let Some(session) = get_session(user_data) {
            // 送信バッファをクリーンアップ
            session.send_buffers.remove(&stream_id);

            session.push_event(Http2Event::StreamClosed {
                stream_id,
                error_code: ErrorCode::from_u32(error_code),
            });
        }
    }
    0
}
