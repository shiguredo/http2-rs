//! HTTP/2 ストリーム管理
//!
//! ストリームの状態、フロー制御、バッファを管理する。

pub mod buffer;
pub mod state;

pub use buffer::{RecvBuffer, SendBuffer};
pub use state::{StateMachine, StreamState};

use crate::flow_control::FlowControl;
use crate::frame::StreamId;
use crate::hpack::HeaderField;

/// ストリーム
#[derive(Debug)]
pub struct Stream {
    /// ストリーム ID
    id: StreamId,
    /// 状態機械
    state: StateMachine,
    /// フロー制御
    flow_control: FlowControl,
    /// 受信したヘッダー
    headers: Vec<HeaderField>,
    /// 受信データバッファ
    recv_buffer: RecvBuffer,
    /// 送信データバッファ
    send_buffer: SendBuffer,
    /// 送信待ちの END_STREAM フラグ
    pending_end_stream: bool,
    /// 初回ヘッダー受信済みフラグ (RFC 9113 Section 8.1)
    ///
    /// 最終ヘッダー（サーバーではリクエスト、クライアントでは非 1xx レスポンス）を
    /// 受信した場合に true になる。以降の疑似ヘッダーを含む HEADERS を拒否するために使用する。
    initial_headers_received: bool,
    /// 期待される Content-Length (RFC 9113 Section 8.1.1)
    ///
    /// Content-Length ヘッダーが存在する場合、その値を保持する。
    expected_content_length: Option<u64>,
    /// 受信した Content-Length (累積)
    received_content_length: u64,
    /// CONNECT トンネル確立済みフラグ (RFC 9113 Section 8.5)
    ///
    /// 通常の CONNECT (:protocol なし) で 2xx レスポンスが送受信された場合に true になる。
    /// 確立後は DATA/RST_STREAM/WINDOW_UPDATE のみ許可され、HEADERS は拒否される。
    connect_established: bool,
    /// リクエストメソッド
    ///
    /// Content-Length チェック (204/304/HEAD の例外) と CONNECT 判定に使用する。
    request_method: Option<Vec<u8>>,
    /// Extended CONNECT (:protocol 付き) かどうか
    ///
    /// Extended CONNECT は通常の HTTP/2 ストリームとして動作するため、
    /// CONNECT トンネルのフレーム種別制限の対象外。
    has_protocol: bool,
    /// コンテンツを持たないレスポンス (RFC 9110 Section 6.4.1)
    ///
    /// 204/304 レスポンスおよび HEAD リクエストへのレスポンスは
    /// コンテンツを持たないため、DATA フレームを受信してはならない。
    no_content: bool,
}

impl Stream {
    /// 新しいストリームを生成する
    ///
    /// RFC 9113 Section 5.2: 送信ウィンドウはリモートの initial_window_size、
    /// 受信ウィンドウはローカルの initial_window_size で初期化する。
    #[must_use]
    pub fn new(id: StreamId, send_initial_window_size: u32, recv_initial_window_size: u32) -> Self {
        Self {
            id,
            state: StateMachine::new(),
            flow_control: FlowControl::with_separate_windows(
                send_initial_window_size,
                recv_initial_window_size,
            ),
            headers: Vec::new(),
            recv_buffer: RecvBuffer::new(recv_initial_window_size as usize),
            send_buffer: SendBuffer::new(send_initial_window_size as usize),
            pending_end_stream: false,
            initial_headers_received: false,
            expected_content_length: None,
            received_content_length: 0,
            connect_established: false,
            request_method: None,
            has_protocol: false,
            no_content: false,
        }
    }

    /// ストリーム ID を取得する
    #[must_use]
    pub const fn id(&self) -> StreamId {
        self.id
    }

    /// 状態を取得する
    #[must_use]
    pub const fn state(&self) -> StreamState {
        self.state.state()
    }

    /// 状態機械への参照を取得する
    #[must_use]
    pub const fn state_machine(&self) -> &StateMachine {
        &self.state
    }

    /// 状態機械への可変参照を取得する
    pub fn state_machine_mut(&mut self) -> &mut StateMachine {
        &mut self.state
    }

    /// フロー制御への参照を取得する
    #[must_use]
    pub const fn flow_control(&self) -> &FlowControl {
        &self.flow_control
    }

    /// フロー制御への可変参照を取得する
    pub fn flow_control_mut(&mut self) -> &mut FlowControl {
        &mut self.flow_control
    }

    /// ヘッダーを設定する
    pub fn set_headers(&mut self, headers: Vec<HeaderField>) {
        self.headers = headers;
    }

    /// ヘッダーを取得する
    #[must_use]
    pub fn headers(&self) -> &[HeaderField] {
        &self.headers
    }

    /// 受信バッファへの参照を取得する
    #[must_use]
    pub const fn recv_buffer(&self) -> &RecvBuffer {
        &self.recv_buffer
    }

    /// 受信バッファへの可変参照を取得する
    pub fn recv_buffer_mut(&mut self) -> &mut RecvBuffer {
        &mut self.recv_buffer
    }

    /// 送信バッファへの参照を取得する
    #[must_use]
    pub const fn send_buffer(&self) -> &SendBuffer {
        &self.send_buffer
    }

    /// 送信バッファへの可変参照を取得する
    pub fn send_buffer_mut(&mut self) -> &mut SendBuffer {
        &mut self.send_buffer
    }

    /// 送信待ちの END_STREAM フラグを取得する
    #[must_use]
    pub const fn pending_end_stream(&self) -> bool {
        self.pending_end_stream
    }

    /// 送信待ちの END_STREAM フラグを設定する
    pub fn set_pending_end_stream(&mut self, value: bool) {
        self.pending_end_stream = value;
    }

    /// ストリームがオープンかどうかを返す
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.state.state().is_open()
    }

    /// ストリームがクローズドかどうかを返す
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.state.state().is_closed()
    }

    /// 初回ヘッダーを受信済みかどうかを返す
    #[must_use]
    pub const fn initial_headers_received(&self) -> bool {
        self.initial_headers_received
    }

    /// 初回ヘッダー受信済みフラグを設定する
    pub fn set_initial_headers_received(&mut self, received: bool) {
        self.initial_headers_received = received;
    }

    /// 期待される Content-Length を設定する
    pub fn set_expected_content_length(&mut self, length: Option<u64>) {
        self.expected_content_length = length;
    }

    /// 期待される Content-Length を取得する
    #[must_use]
    pub const fn expected_content_length(&self) -> Option<u64> {
        self.expected_content_length
    }

    /// 受信した Content-Length を取得する
    #[must_use]
    pub const fn received_content_length(&self) -> u64 {
        self.received_content_length
    }

    /// 受信した Content-Length を加算する
    pub fn add_received_content_length(&mut self, length: u64) {
        self.received_content_length += length;
    }

    /// CONNECT トンネルが確立済みかどうかを返す
    #[must_use]
    pub const fn connect_established(&self) -> bool {
        self.connect_established
    }

    /// CONNECT トンネル確立済みフラグを設定する
    pub fn set_connect_established(&mut self, established: bool) {
        self.connect_established = established;
    }

    /// リクエストメソッドを取得する
    #[must_use]
    pub fn request_method(&self) -> Option<&[u8]> {
        self.request_method.as_deref()
    }

    /// リクエストメソッドを設定する
    pub fn set_request_method(&mut self, method: Vec<u8>) {
        self.request_method = Some(method);
    }

    /// Extended CONNECT (:protocol 付き) かどうかを返す
    #[must_use]
    pub const fn has_protocol(&self) -> bool {
        self.has_protocol
    }

    /// Extended CONNECT フラグを設定する
    pub fn set_has_protocol(&mut self, has_protocol: bool) {
        self.has_protocol = has_protocol;
    }

    /// コンテンツを持たないレスポンスかどうかを返す
    #[must_use]
    pub const fn no_content(&self) -> bool {
        self.no_content
    }

    /// コンテンツを持たないレスポンスフラグを設定する
    pub fn set_no_content(&mut self, no_content: bool) {
        self.no_content = no_content;
    }
}
