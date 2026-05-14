//! HTTP/2 フレーム (RFC 9113 Section 4, 6)
//!
//! HTTP/2 で使用される各種フレームの型定義とエンコード/デコードを提供する。

pub mod decoder;
pub mod encoder;
pub mod flags;

pub use decoder::FrameDecoder;
pub use encoder::FrameEncoder;
pub use flags::FrameFlags;

use crate::settings::Setting;

/// フレームヘッダーサイズ（9 バイト）
pub const FRAME_HEADER_SIZE: usize = 9;

/// ストリーム ID の型
pub type StreamId = u32;

/// 接続レベルのストリーム ID
pub const CONNECTION_STREAM_ID: StreamId = 0;

/// フレームタイプ (RFC 9113 Section 6, RFC 9218 Section 7.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameType {
    /// DATA フレーム
    Data = 0x00,
    /// HEADERS フレーム
    Headers = 0x01,
    /// PRIORITY フレーム (RFC 9113 Section 6.3)
    ///
    /// # 非推奨 (Deprecated)
    ///
    /// RFC 9113 で優先度シグナリングは非推奨となった。
    /// ただし、相互運用性のため受信と minimal processing は必須。
    /// 送信側での使用は推奨されない。
    Priority = 0x02,
    /// RST_STREAM フレーム
    RstStream = 0x03,
    /// SETTINGS フレーム
    Settings = 0x04,
    /// PUSH_PROMISE フレーム (RFC 9113 Section 6.6)
    ///
    /// # 非サポート
    ///
    /// サーバープッシュは主要ブラウザでサポートが削除されているため、
    /// このライブラリでは送信機能を提供しない。
    /// ただし、RFC 9113 に従い受信時は PROTOCOL_ERROR を返す。
    PushPromise = 0x05,
    /// PING フレーム
    Ping = 0x06,
    /// GOAWAY フレーム
    Goaway = 0x07,
    /// WINDOW_UPDATE フレーム
    WindowUpdate = 0x08,
    /// CONTINUATION フレーム
    Continuation = 0x09,
    /// PRIORITY_UPDATE フレーム (RFC 9218 Section 7.1)
    ///
    /// Extensible Priorities (RFC 9218) で定義される優先度更新フレーム。
    /// 非推奨の PRIORITY フレームの代替として使用される。
    PriorityUpdate = 0x10,
}

impl FrameType {
    /// u8 から `FrameType` を生成する
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
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
            0x10 => Some(Self::PriorityUpdate),
            _ => None,
        }
    }

    /// `FrameType` を u8 に変換する
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// フレームヘッダー
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// ペイロード長（24 ビット）
    pub length: u32,
    /// フレームタイプ
    pub frame_type: u8,
    /// フレームフラグ
    pub flags: FrameFlags,
    /// ストリーム ID（31 ビット）
    pub stream_id: StreamId,
}

impl FrameHeader {
    /// 新しい `FrameHeader` を生成する
    #[must_use]
    pub const fn new(frame_type: FrameType, flags: FrameFlags, stream_id: StreamId) -> Self {
        Self {
            length: 0,
            frame_type: frame_type.as_u8(),
            flags,
            stream_id,
        }
    }

    /// ペイロード長を設定する
    #[must_use]
    pub const fn with_length(mut self, length: u32) -> Self {
        self.length = length;
        self
    }

    /// フレームタイプを取得する
    #[must_use]
    pub const fn get_frame_type(&self) -> Option<FrameType> {
        FrameType::from_u8(self.frame_type)
    }
}

/// DATA フレーム (RFC 9113 Section 6.1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFrame {
    /// ストリーム ID
    pub stream_id: StreamId,
    /// END_STREAM フラグ
    pub end_stream: bool,
    /// データ
    pub data: Vec<u8>,
    /// パディング長 (RFC 9113 Section 6.1)
    ///
    /// Some の場合、PADDED フラグが設定され、指定されたバイト数のパディングが追加される。
    /// パディングはデータ長の曖昧化やトラフィック解析対策に使用される。
    pub pad_length: Option<u8>,
}

impl DataFrame {
    /// 新しい `DataFrame` を生成する
    #[must_use]
    pub fn new(stream_id: StreamId, data: Vec<u8>) -> Self {
        Self {
            stream_id,
            end_stream: false,
            data,
            pad_length: None,
        }
    }

    /// END_STREAM フラグを設定する
    #[must_use]
    pub const fn with_end_stream(mut self, end_stream: bool) -> Self {
        self.end_stream = end_stream;
        self
    }

    /// パディング長を設定する
    ///
    /// # パラメータ
    ///
    /// - `pad_length`: パディングバイト数 (0-255)
    #[must_use]
    pub const fn with_padding(mut self, pad_length: u8) -> Self {
        self.pad_length = Some(pad_length);
        self
    }
}

/// HEADERS フレーム (RFC 9113 Section 6.2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadersFrame {
    /// ストリーム ID
    pub stream_id: StreamId,
    /// END_STREAM フラグ
    pub end_stream: bool,
    /// END_HEADERS フラグ
    pub end_headers: bool,
    /// 優先度フィールド (非推奨、受信時のみ)
    ///
    /// # 非推奨 (Deprecated)
    ///
    /// RFC 9113 で非推奨となった。相互運用性のため受信は処理する。
    pub priority_fields: Option<PriorityFields>,
    /// ヘッダーブロックフラグメント（HPACK エンコード済み）
    pub header_block_fragment: Vec<u8>,
    /// パディング長 (RFC 9113 Section 6.2)
    ///
    /// Some の場合、PADDED フラグが設定され、指定されたバイト数のパディングが追加される。
    pub pad_length: Option<u8>,
}

impl HeadersFrame {
    /// 新しい `HeadersFrame` を生成する
    #[must_use]
    pub fn new(stream_id: StreamId, header_block_fragment: Vec<u8>) -> Self {
        Self {
            stream_id,
            end_stream: false,
            end_headers: true,
            priority_fields: None,
            header_block_fragment,
            pad_length: None,
        }
    }

    /// END_STREAM フラグを設定する
    #[must_use]
    pub const fn with_end_stream(mut self, end_stream: bool) -> Self {
        self.end_stream = end_stream;
        self
    }

    /// END_HEADERS フラグを設定する
    #[must_use]
    pub const fn with_end_headers(mut self, end_headers: bool) -> Self {
        self.end_headers = end_headers;
        self
    }

    /// パディング長を設定する
    #[must_use]
    pub const fn with_padding(mut self, pad_length: u8) -> Self {
        self.pad_length = Some(pad_length);
        self
    }
}

/// HEADERS フレーム内の優先度フィールド (RFC 9113 Section 6.2)
///
/// # 非推奨 (Deprecated)
///
/// RFC 9113 で非推奨となった。相互運用性のため受信は処理する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriorityFields {
    /// Exclusive フラグ
    pub exclusive: bool,
    /// 依存するストリーム ID
    pub stream_dependency: StreamId,
    /// 重み (1-256、ワイヤーフォーマットでは 0-255)
    pub weight: u8,
}

/// PRIORITY フレーム (RFC 9113 Section 6.3)
///
/// # 非推奨 (Deprecated)
///
/// RFC 9113 で優先度シグナリングは非推奨となった。
/// 相互運用性のため受信は処理するが、送信は行わない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriorityFrame {
    /// ストリーム ID
    pub stream_id: StreamId,
    /// Exclusive フラグ
    pub exclusive: bool,
    /// 依存するストリーム ID
    pub stream_dependency: StreamId,
    /// 重み (1-256、ワイヤーフォーマットでは 0-255)
    pub weight: u8,
}

/// RST_STREAM フレーム (RFC 9113 Section 6.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RstStreamFrame {
    /// ストリーム ID
    pub stream_id: StreamId,
    /// エラーコード
    pub error_code: u32,
}

impl RstStreamFrame {
    /// 新しい `RstStreamFrame` を生成する
    #[must_use]
    pub const fn new(stream_id: StreamId, error_code: u32) -> Self {
        Self {
            stream_id,
            error_code,
        }
    }
}

/// SETTINGS フレーム (RFC 9113 Section 6.5)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFrame {
    /// ACK フラグ
    pub ack: bool,
    /// 設定パラメータのリスト
    pub settings: Vec<Setting>,
}

impl SettingsFrame {
    /// 空の SETTINGS フレームを生成する
    #[must_use]
    pub fn new() -> Self {
        Self {
            ack: false,
            settings: Vec::new(),
        }
    }

    /// ACK フレームを生成する
    #[must_use]
    pub fn ack() -> Self {
        Self {
            ack: true,
            settings: Vec::new(),
        }
    }

    /// 設定を追加する
    pub fn add_setting(&mut self, setting: Setting) {
        self.settings.push(setting);
    }
}

impl Default for SettingsFrame {
    fn default() -> Self {
        Self::new()
    }
}

/// PING フレーム (RFC 9113 Section 6.7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingFrame {
    /// ACK フラグ
    pub ack: bool,
    /// 不透明データ（8 バイト）
    pub opaque_data: [u8; 8],
}

impl PingFrame {
    /// 新しい `PingFrame` を生成する
    #[must_use]
    pub const fn new(opaque_data: [u8; 8]) -> Self {
        Self {
            ack: false,
            opaque_data,
        }
    }

    /// ACK フレームを生成する
    #[must_use]
    pub const fn ack(opaque_data: [u8; 8]) -> Self {
        Self {
            ack: true,
            opaque_data,
        }
    }
}

/// GOAWAY フレーム (RFC 9113 Section 6.8)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoawayFrame {
    /// 最後に処理したストリーム ID
    pub last_stream_id: StreamId,
    /// エラーコード
    pub error_code: u32,
    /// 追加のデバッグデータ
    pub debug_data: Vec<u8>,
}

impl GoawayFrame {
    /// 新しい `GoawayFrame` を生成する
    #[must_use]
    pub fn new(last_stream_id: StreamId, error_code: u32) -> Self {
        Self {
            last_stream_id,
            error_code,
            debug_data: Vec::new(),
        }
    }

    /// デバッグデータを追加する
    #[must_use]
    pub fn with_debug_data(mut self, debug_data: Vec<u8>) -> Self {
        self.debug_data = debug_data;
        self
    }
}

/// WINDOW_UPDATE フレーム (RFC 9113 Section 6.9)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowUpdateFrame {
    /// ストリーム ID（0 の場合は接続レベル）
    pub stream_id: StreamId,
    /// ウィンドウサイズ増分（31 ビット）
    pub window_size_increment: u32,
}

impl WindowUpdateFrame {
    /// 新しい `WindowUpdateFrame` を生成する
    #[must_use]
    pub const fn new(stream_id: StreamId, window_size_increment: u32) -> Self {
        Self {
            stream_id,
            window_size_increment,
        }
    }
}

/// CONTINUATION フレーム (RFC 9113 Section 6.10)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationFrame {
    /// ストリーム ID
    pub stream_id: StreamId,
    /// END_HEADERS フラグ
    pub end_headers: bool,
    /// ヘッダーブロックフラグメント
    pub header_block_fragment: Vec<u8>,
}

impl ContinuationFrame {
    /// 新しい `ContinuationFrame` を生成する
    #[must_use]
    pub fn new(stream_id: StreamId, header_block_fragment: Vec<u8>) -> Self {
        Self {
            stream_id,
            end_headers: false,
            header_block_fragment,
        }
    }

    /// END_HEADERS フラグを設定する
    #[must_use]
    pub const fn with_end_headers(mut self, end_headers: bool) -> Self {
        self.end_headers = end_headers;
        self
    }
}

/// PRIORITY_UPDATE フレーム (RFC 9218 Section 7.1)
///
/// Extensible Priorities で定義される優先度更新フレーム。
/// クライアントがサーバーに対してストリームの優先度を通知するために使用する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityUpdateFrame {
    /// 優先度を更新するストリーム ID
    ///
    /// RFC 9218 Section 7.1: Prioritized Stream ID
    /// クライアント開始ストリーム (奇数) の ID を指定する。
    pub prioritized_element_id: StreamId,
    /// Priority Field Value
    ///
    /// RFC 9218 Section 7.1: Structured Fields (RFC 8941) の Dictionary 形式。
    /// 空の場合はデフォルト優先度を使用。
    pub priority_field_value: Vec<u8>,
}

impl PriorityUpdateFrame {
    /// 新しい `PriorityUpdateFrame` を生成する
    #[must_use]
    pub fn new(prioritized_element_id: StreamId, priority_field_value: Vec<u8>) -> Self {
        Self {
            prioritized_element_id,
            priority_field_value,
        }
    }

    /// デフォルト優先度の `PriorityUpdateFrame` を生成する
    #[must_use]
    pub fn default_priority(prioritized_element_id: StreamId) -> Self {
        Self {
            prioritized_element_id,
            priority_field_value: Vec::new(),
        }
    }
}

/// HTTP/2 フレーム
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// DATA フレーム
    Data(DataFrame),
    /// HEADERS フレーム
    Headers(HeadersFrame),
    /// PRIORITY フレーム (非推奨)
    ///
    /// # 非推奨 (Deprecated)
    ///
    /// RFC 9113 で優先度シグナリングは非推奨となった。
    /// 相互運用性のため受信は処理するが、送信は行わない。
    Priority(PriorityFrame),
    /// RST_STREAM フレーム
    RstStream(RstStreamFrame),
    /// SETTINGS フレーム
    Settings(SettingsFrame),
    /// PUSH_PROMISE フレーム (非サポート)
    ///
    /// # 非サポート
    ///
    /// サーバープッシュは主要ブラウザでサポートが削除されているため、
    /// 受信時は PROTOCOL_ERROR を返す。ストリーム ID のみ保持。
    PushPromise {
        /// ストリーム ID
        stream_id: StreamId,
    },
    /// PING フレーム
    Ping(PingFrame),
    /// GOAWAY フレーム
    Goaway(GoawayFrame),
    /// WINDOW_UPDATE フレーム
    WindowUpdate(WindowUpdateFrame),
    /// CONTINUATION フレーム
    Continuation(ContinuationFrame),
    /// PRIORITY_UPDATE フレーム (RFC 9218)
    ///
    /// Extensible Priorities で定義される優先度更新フレーム。
    PriorityUpdate(PriorityUpdateFrame),
    /// 未知のフレームタイプ
    Unknown {
        /// フレームヘッダー
        header: FrameHeader,
        /// ペイロード
        payload: Vec<u8>,
    },
}

impl Frame {
    /// フレームのストリーム ID を取得する
    #[must_use]
    pub const fn stream_id(&self) -> StreamId {
        match self {
            Self::Data(f) => f.stream_id,
            Self::Headers(f) => f.stream_id,
            Self::Priority(f) => f.stream_id,
            Self::RstStream(f) => f.stream_id,
            Self::Settings(_) => CONNECTION_STREAM_ID,
            Self::PushPromise { stream_id } => *stream_id,
            Self::Ping(_) => CONNECTION_STREAM_ID,
            Self::Goaway(_) => CONNECTION_STREAM_ID,
            Self::WindowUpdate(f) => f.stream_id,
            Self::Continuation(f) => f.stream_id,
            // RFC 9218 Section 7.1: PRIORITY_UPDATE は stream identifier 0 で送信される
            Self::PriorityUpdate(_) => CONNECTION_STREAM_ID,
            Self::Unknown { header, .. } => header.stream_id,
        }
    }

    /// フレームタイプを取得する
    #[must_use]
    pub const fn frame_type(&self) -> Option<FrameType> {
        match self {
            Self::Data(_) => Some(FrameType::Data),
            Self::Headers(_) => Some(FrameType::Headers),
            Self::Priority(_) => Some(FrameType::Priority),
            Self::RstStream(_) => Some(FrameType::RstStream),
            Self::Settings(_) => Some(FrameType::Settings),
            Self::PushPromise { .. } => Some(FrameType::PushPromise),
            Self::Ping(_) => Some(FrameType::Ping),
            Self::Goaway(_) => Some(FrameType::Goaway),
            Self::WindowUpdate(_) => Some(FrameType::WindowUpdate),
            Self::Continuation(_) => Some(FrameType::Continuation),
            Self::PriorityUpdate(_) => Some(FrameType::PriorityUpdate),
            Self::Unknown { .. } => None,
        }
    }
}
