//! Sans I/O HTTP/2 ライブラリ
//!
//! RFC 9113 (HTTP/2) および RFC 7541 (HPACK) の実装を提供する。
//!
//! # 特徴
//!
//! - Sans I/O: I/O を行わず、バイト列の入出力のみを扱う
//! - 0 依存: 標準ライブラリのみを使用
//! - RFC 準拠: RFC 9113 および RFC 7541 に準拠
//!
//! # モジュール構成
//!
//! - [`error`] - エラー型
//! - [`frame`] - HTTP/2 フレーム
//! - [`hpack`] - HPACK ヘッダー圧縮
//! - [`settings`] - HTTP/2 SETTINGS パラメータ
//! - [`stream`] - ストリーム管理
//! - [`flow_control`] - フロー制御
//! - [`event`] - イベント定義
//! - [`limits`] - 制限設定
//! - [`connection`] - 接続管理
//! - [`webtransport`] - WebTransport over HTTP/2

pub mod connection;
pub mod decode_error;
pub mod error;
pub mod event;
pub mod flow_control;
pub mod frame;
pub mod hpack;
pub mod limits;
pub mod settings;
pub mod stream;
pub mod stream_id;
pub mod validation;
pub mod webtransport;

pub(crate) mod bounded_set;
pub(crate) mod syntax;

pub use connection::{Connection, ConnectionState, Role};
pub use decode_error::DecodeError;
pub use error::{Error, ErrorCode, ErrorKind, Result};
pub use event::Event;
pub use flow_control::{FlowControl, MAX_WINDOW_SIZE};
pub use frame::{
    ContinuationFrame, DataFrame, FRAME_HEADER_SIZE, Frame, FrameDecoder, FrameEncoder, FrameError,
    FrameFlags, FrameHeader, FrameType, GoawayFrame, HeadersFrame, LastStreamId, PingFrame,
    PriorityUpdateFrame, RstStreamFrame, SettingsFrame, StreamId, Weight, WindowIncrement,
    WindowUpdateFrame,
};
pub use hpack::{Decoder as HpackDecoder, Encoder as HpackEncoder, HeaderField, HeaderFieldError};
pub use limits::{Limits, LimitsBuilder, LimitsError};
pub use settings::{MaxFrameSize, Setting, SettingError, Settings, WindowSize};
pub use stream::{Stream, StreamState};
pub use stream_id::{ClientStreamId, NonZeroStreamId, Parity, ServerStreamId, StreamIdError};

/// HTTP/2 接続プリフェイス (RFC 9113 Section 3.4)
pub const CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// HTTP/2 接続プリフェイスの長さ
pub const CONNECTION_PREFACE_LEN: usize = 24;
