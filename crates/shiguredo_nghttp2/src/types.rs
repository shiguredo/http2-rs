//! nghttp2 共通型定義

/// ストリーム ID
pub type StreamId = i32;

/// HTTP/2 ヘッダー
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// ヘッダー名
    pub name: Vec<u8>,
    /// ヘッダー値
    pub value: Vec<u8>,
    /// 機密フラグ（Never Indexed）
    pub sensitive: bool,
}

impl Header {
    /// 新しいヘッダーを作成
    pub fn new(name: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            sensitive: false,
        }
    }

    /// 機密ヘッダーを作成
    pub fn sensitive(name: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            sensitive: true,
        }
    }

    /// 疑似ヘッダー :method
    pub fn method(method: &str) -> Self {
        Self::new(b":method".to_vec(), method.as_bytes().to_vec())
    }

    /// 疑似ヘッダー :scheme
    pub fn scheme(scheme: &str) -> Self {
        Self::new(b":scheme".to_vec(), scheme.as_bytes().to_vec())
    }

    /// 疑似ヘッダー :authority
    pub fn authority(authority: &str) -> Self {
        Self::new(b":authority".to_vec(), authority.as_bytes().to_vec())
    }

    /// 疑似ヘッダー :path
    pub fn path(path: &str) -> Self {
        Self::new(b":path".to_vec(), path.as_bytes().to_vec())
    }

    /// 疑似ヘッダー :status
    pub fn status(status: u16) -> Self {
        Self::new(b":status".to_vec(), status.to_string().into_bytes())
    }

    /// ヘッダー名を文字列として取得
    pub fn name_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.name).ok()
    }

    /// ヘッダー値を文字列として取得
    pub fn value_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.value).ok()
    }
}

/// HTTP/2 フレームタイプ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    /// DATA フレーム
    Data = 0x00,
    /// HEADERS フレーム
    Headers = 0x01,
    /// PRIORITY フレーム (RFC 9113 で非推奨)
    Priority = 0x02,
    /// RST_STREAM フレーム
    RstStream = 0x03,
    /// SETTINGS フレーム
    Settings = 0x04,
    /// PUSH_PROMISE フレーム (主要ブラウザで削除済み)
    PushPromise = 0x05,
    /// PING フレーム
    Ping = 0x06,
    /// GOAWAY フレーム
    Goaway = 0x07,
    /// WINDOW_UPDATE フレーム
    WindowUpdate = 0x08,
    /// CONTINUATION フレーム
    Continuation = 0x09,
}

impl FrameType {
    /// u8 から FrameType を生成
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Data),
            0x01 => Some(Self::Headers),
            0x02 => Some(Self::Priority),
            0x03 => Some(Self::RstStream),
            0x04 => Some(Self::Settings),
            0x05 => Some(Self::PushPromise),
            0x06 => Some(Self::Ping),
            0x07 => Some(Self::Goaway),
            0x08 => Some(Self::WindowUpdate),
            0x09 => Some(Self::Continuation),
            _ => None,
        }
    }
}

/// HTTP/2 エラーコード
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ErrorCode {
    /// NO_ERROR
    NoError = 0x00,
    /// PROTOCOL_ERROR
    ProtocolError = 0x01,
    /// INTERNAL_ERROR
    InternalError = 0x02,
    /// FLOW_CONTROL_ERROR
    FlowControlError = 0x03,
    /// SETTINGS_TIMEOUT
    SettingsTimeout = 0x04,
    /// STREAM_CLOSED
    StreamClosed = 0x05,
    /// FRAME_SIZE_ERROR
    FrameSizeError = 0x06,
    /// REFUSED_STREAM
    RefusedStream = 0x07,
    /// CANCEL
    Cancel = 0x08,
    /// COMPRESSION_ERROR
    CompressionError = 0x09,
    /// CONNECT_ERROR
    ConnectError = 0x0a,
    /// ENHANCE_YOUR_CALM
    EnhanceYourCalm = 0x0b,
    /// INADEQUATE_SECURITY
    InadequateSecurity = 0x0c,
    /// HTTP_1_1_REQUIRED
    Http11Required = 0x0d,
}

impl ErrorCode {
    /// u32 から ErrorCode を生成
    pub fn from_u32(value: u32) -> Self {
        match value {
            0x00 => Self::NoError,
            0x01 => Self::ProtocolError,
            0x02 => Self::InternalError,
            0x03 => Self::FlowControlError,
            0x04 => Self::SettingsTimeout,
            0x05 => Self::StreamClosed,
            0x06 => Self::FrameSizeError,
            0x07 => Self::RefusedStream,
            0x08 => Self::Cancel,
            0x09 => Self::CompressionError,
            0x0a => Self::ConnectError,
            0x0b => Self::EnhanceYourCalm,
            0x0c => Self::InadequateSecurity,
            0x0d => Self::Http11Required,
            _ => Self::InternalError,
        }
    }

    /// u32 に変換
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

/// HTTP/2 イベント
#[derive(Debug)]
pub enum Http2Event {
    /// ヘッダー受信
    HeadersReceived {
        stream_id: StreamId,
        headers: Vec<Header>,
        end_stream: bool,
    },
    /// データ受信
    DataReceived {
        stream_id: StreamId,
        data: Vec<u8>,
        end_stream: bool,
    },
    /// ストリームクローズ
    StreamClosed {
        stream_id: StreamId,
        error_code: ErrorCode,
    },
    /// GOAWAY 受信
    GoawayReceived {
        last_stream_id: StreamId,
        error_code: ErrorCode,
        debug_data: Vec<u8>,
    },
    /// PING 受信
    PingReceived { opaque_data: [u8; 8], ack: bool },
    /// SETTINGS 受信
    SettingsReceived { ack: bool },
    /// WINDOW_UPDATE 受信
    WindowUpdateReceived { stream_id: StreamId, increment: u32 },
}
