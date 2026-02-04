//! nghttp2 共通型定義

/// ストリーム ID
pub type StreamId = i32;

/// HTTP/2 SETTINGS パラメータ ID (RFC 9113 Section 6.5.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum SettingsId {
    /// SETTINGS_HEADER_TABLE_SIZE (0x01)
    HeaderTableSize = 0x01,
    /// SETTINGS_ENABLE_PUSH (0x02)
    EnablePush = 0x02,
    /// SETTINGS_MAX_CONCURRENT_STREAMS (0x03)
    MaxConcurrentStreams = 0x03,
    /// SETTINGS_INITIAL_WINDOW_SIZE (0x04)
    InitialWindowSize = 0x04,
    /// SETTINGS_MAX_FRAME_SIZE (0x05)
    MaxFrameSize = 0x05,
    /// SETTINGS_MAX_HEADER_LIST_SIZE (0x06)
    MaxHeaderListSize = 0x06,
    /// SETTINGS_ENABLE_CONNECT_PROTOCOL (0x08, RFC 8441)
    EnableConnectProtocol = 0x08,
    /// SETTINGS_NO_RFC7540_PRIORITIES (0x09, RFC 9218)
    NoRfc7540Priorities = 0x09,
}

impl SettingsId {
    /// nghttp2_settings_id (i32) に変換
    pub fn as_i32(self) -> i32 {
        self as u16 as i32
    }
}

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
    #[deprecated(note = "RFC 9113 で非推奨。使用しないこと")]
    Priority = 0x02,
    /// RST_STREAM フレーム
    RstStream = 0x03,
    /// SETTINGS フレーム
    Settings = 0x04,
    /// PUSH_PROMISE フレーム (主要ブラウザで削除済み)
    #[deprecated(note = "主要ブラウザで削除済み。使用しないこと")]
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
    #[allow(deprecated)]
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

/// HTTP/2 エラーコード (RFC 9113 Section 7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// NO_ERROR (0x00)
    NoError,
    /// PROTOCOL_ERROR (0x01)
    ProtocolError,
    /// INTERNAL_ERROR (0x02)
    InternalError,
    /// FLOW_CONTROL_ERROR (0x03)
    FlowControlError,
    /// SETTINGS_TIMEOUT (0x04)
    SettingsTimeout,
    /// STREAM_CLOSED (0x05)
    StreamClosed,
    /// FRAME_SIZE_ERROR (0x06)
    FrameSizeError,
    /// REFUSED_STREAM (0x07)
    RefusedStream,
    /// CANCEL (0x08)
    Cancel,
    /// COMPRESSION_ERROR (0x09)
    CompressionError,
    /// CONNECT_ERROR (0x0a)
    ConnectError,
    /// ENHANCE_YOUR_CALM (0x0b)
    EnhanceYourCalm,
    /// INADEQUATE_SECURITY (0x0c)
    InadequateSecurity,
    /// HTTP_1_1_REQUIRED (0x0d)
    Http11Required,
    /// 未知のエラーコード
    Unknown(u32),
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
            other => Self::Unknown(other),
        }
    }

    /// u32 に変換
    pub fn as_u32(self) -> u32 {
        match self {
            Self::NoError => 0x00,
            Self::ProtocolError => 0x01,
            Self::InternalError => 0x02,
            Self::FlowControlError => 0x03,
            Self::SettingsTimeout => 0x04,
            Self::StreamClosed => 0x05,
            Self::FrameSizeError => 0x06,
            Self::RefusedStream => 0x07,
            Self::Cancel => 0x08,
            Self::CompressionError => 0x09,
            Self::ConnectError => 0x0a,
            Self::EnhanceYourCalm => 0x0b,
            Self::InadequateSecurity => 0x0c,
            Self::Http11Required => 0x0d,
            Self::Unknown(code) => code,
        }
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
    /// フレーム送信完了
    FrameSent {
        stream_id: StreamId,
        frame_type: FrameType,
    },
    /// フレーム送信失敗
    FrameNotSent {
        stream_id: StreamId,
        frame_type: FrameType,
        lib_error_code: i32,
    },
    /// 不正なフレーム受信
    InvalidFrameReceived {
        stream_id: StreamId,
        frame_type: FrameType,
        lib_error_code: i32,
    },
    /// 不正なヘッダー受信
    InvalidHeaderReceived {
        stream_id: StreamId,
        name: Vec<u8>,
        value: Vec<u8>,
    },
}
