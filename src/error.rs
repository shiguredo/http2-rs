//! HTTP/2 エラー型定義
//!
//! RFC 9113 Section 7 で定義されるエラーコードと、
//! ライブラリ内部で使用するエラー型を提供する。

use std::backtrace::{Backtrace, BacktraceStatus};
use std::panic::Location;

use crate::decode_error::DecodeError;

/// HTTP/2 エラーコード (RFC 9113 Section 7)
///
/// 接続エラーやストリームエラーで使用される。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// 正常終了
    NoError,
    /// プロトコルエラー
    ProtocolError,
    /// 内部エラー
    InternalError,
    /// フロー制御エラー
    FlowControlError,
    /// SETTINGS タイムアウト
    SettingsTimeout,
    /// クローズ済みストリームでのフレーム受信
    StreamClosed,
    /// フレームサイズエラー
    FrameSizeError,
    /// ストリーム拒否
    RefusedStream,
    /// ストリームキャンセル
    Cancel,
    /// 圧縮コンテキストエラー
    CompressionError,
    /// CONNECT エラー
    ConnectError,
    /// 過負荷による処理拒否
    EnhanceYourCalm,
    /// セキュリティ要件不足
    InadequateSecurity,
    /// HTTP/1.1 使用要求
    Http11Required,

    // === WebTransport エラーコード (draft-ietf-webtrans-http2-14) ===
    /// WEBTRANSPORT_ERROR
    ///
    /// draft-ietf-webtrans-http2-14 Section 3.4: 汎用 WebTransport エラー。
    /// より具体的なエラーコードがない場合に使用する。
    ///
    /// 注: この値は暫定値。IANA 登録後に更新される可能性がある。
    WebtransportError,

    /// WEBTRANSPORT_STREAM_STATE_ERROR
    ///
    /// draft-ietf-webtrans-http2-14 Section 3.4: ストリーム関連の capsule が
    /// 無効な状態のストリームを指定した。
    ///
    /// 注: この値は暫定値。IANA 登録後に更新される可能性がある。
    WebtransportStreamStateError,

    /// WEBTRANSPORT_FLOW_CONTROL_ERROR
    ///
    /// draft-ietf-webtrans-http2-14 Section 11.3: フロー制御エラーが発生した。
    ///
    /// 注: この値は暫定値。IANA 登録後に更新される可能性がある。
    WebtransportFlowControlError,

    /// 未知のエラーコード (RFC 9113 Section 7)
    ///
    /// RFC 9113 Section 7: 未知のエラーコードは特別な意味を持たないが、
    /// 生の値を保持して情報を失わないようにする。
    Unknown(u32),
}

impl ErrorCode {
    /// u32 から `ErrorCode` を生成する
    ///
    /// RFC 9113 Section 7: 未知のエラーコードは `Unknown(value)` として保持する。
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
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
            0x100 => Self::WebtransportError,
            0x101 => Self::WebtransportStreamStateError,
            0x102 => Self::WebtransportFlowControlError,
            _ => Self::Unknown(value),
        }
    }

    /// `ErrorCode` を u32 に変換する
    #[must_use]
    pub const fn as_u32(self) -> u32 {
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
            Self::WebtransportError => 0x100,
            Self::WebtransportStreamStateError => 0x101,
            Self::WebtransportFlowControlError => 0x102,
            Self::Unknown(code) => code,
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoError => write!(f, "NO_ERROR"),
            Self::ProtocolError => write!(f, "PROTOCOL_ERROR"),
            Self::InternalError => write!(f, "INTERNAL_ERROR"),
            Self::FlowControlError => write!(f, "FLOW_CONTROL_ERROR"),
            Self::SettingsTimeout => write!(f, "SETTINGS_TIMEOUT"),
            Self::StreamClosed => write!(f, "STREAM_CLOSED"),
            Self::FrameSizeError => write!(f, "FRAME_SIZE_ERROR"),
            Self::RefusedStream => write!(f, "REFUSED_STREAM"),
            Self::Cancel => write!(f, "CANCEL"),
            Self::CompressionError => write!(f, "COMPRESSION_ERROR"),
            Self::ConnectError => write!(f, "CONNECT_ERROR"),
            Self::EnhanceYourCalm => write!(f, "ENHANCE_YOUR_CALM"),
            Self::InadequateSecurity => write!(f, "INADEQUATE_SECURITY"),
            Self::Http11Required => write!(f, "HTTP_1_1_REQUIRED"),
            Self::WebtransportError => write!(f, "WEBTRANSPORT_ERROR"),
            Self::WebtransportStreamStateError => write!(f, "WEBTRANSPORT_STREAM_STATE_ERROR"),
            Self::WebtransportFlowControlError => write!(f, "WEBTRANSPORT_FLOW_CONTROL_ERROR"),
            Self::Unknown(code) => write!(f, "UNKNOWN(0x{code:x})"),
        }
    }
}

/// エラーの種類
///
/// 接続エラー / ストリームエラー / HPACK エラーの 3 種類を表現する。
/// 構築時検査エラーは [`crate::HeaderFieldError`] / [`crate::SettingError`] /
/// [`crate::LimitsError`] などのドメイン特化型で表現する。
/// フレームレベルのバッファ操作エラーは [`DecodeError`] で表現し、
/// [`From<DecodeError> for Error`] 経由でフレームサイズエラーに昇格する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// 接続エラー（GOAWAY を送信する必要がある）
    ConnectionError(ErrorCode),

    /// ストリームエラー（RST_STREAM を送信する必要がある）
    StreamError(ErrorCode),

    /// HPACK デコードエラー
    HpackError,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionError(code) => write!(f, "ConnectionError({code})"),
            Self::StreamError(code) => write!(f, "StreamError({code})"),
            Self::HpackError => write!(f, "HpackError"),
        }
    }
}

/// HTTP/2 エラー型
pub struct Error {
    /// 発生したエラーの種類
    pub kind: ErrorKind,

    /// エラーが発生した理由
    pub reason: String,

    /// エラーが作成されたソースコードの場所
    pub location: &'static Location<'static>,

    /// エラー発生箇所を示すバックトレース
    ///
    /// バックトレースは `RUST_BACKTRACE` 環境変数が設定されていない場合には取得されない
    pub backtrace: Backtrace,
}

impl Error {
    /// [`Error`] インスタンスを生成する
    #[track_caller]
    pub fn new(kind: ErrorKind) -> Self {
        Self::with_reason(kind, String::new())
    }

    /// エラー理由つきで [`Error`] インスタンスを生成する
    #[track_caller]
    pub fn with_reason<T: Into<String>>(kind: ErrorKind, reason: T) -> Self {
        Self {
            kind,
            reason: reason.into(),
            location: Location::caller(),
            backtrace: Backtrace::capture(),
        }
    }

    /// 接続エラーを生成する
    #[track_caller]
    pub fn connection_error<T: Into<String>>(code: ErrorCode, reason: T) -> Self {
        Self::with_reason(ErrorKind::ConnectionError(code), reason)
    }

    /// ストリームエラーを生成する
    #[track_caller]
    pub fn stream_error<T: Into<String>>(code: ErrorCode, reason: T) -> Self {
        Self::with_reason(ErrorKind::StreamError(code), reason)
    }

    /// HPACK エラーを生成する
    #[track_caller]
    pub fn hpack_error<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(ErrorKind::HpackError, reason)
    }

    /// プロトコルエラー（接続レベル）を生成する
    #[track_caller]
    pub fn protocol_error<T: Into<String>>(reason: T) -> Self {
        Self::connection_error(ErrorCode::ProtocolError, reason)
    }

    /// フレームサイズエラーを生成する
    #[track_caller]
    pub fn frame_size_error<T: Into<String>>(reason: T) -> Self {
        Self::connection_error(ErrorCode::FrameSizeError, reason)
    }

    /// 接続エラーかどうかを返す
    #[must_use]
    pub const fn is_connection_error(&self) -> bool {
        matches!(self.kind, ErrorKind::ConnectionError(_))
    }

    /// ストリームエラーかどうかを返す
    #[must_use]
    pub const fn is_stream_error(&self) -> bool {
        matches!(self.kind, ErrorKind::StreamError(_))
    }

    /// エラーコードを返す（接続エラーまたはストリームエラーの場合）
    #[must_use]
    pub const fn error_code(&self) -> Option<ErrorCode> {
        match self.kind {
            ErrorKind::ConnectionError(code) | ErrorKind::StreamError(code) => Some(code),
            _ => None,
        }
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        if !self.reason.is_empty() {
            write!(f, ": {}", self.reason)?;
        }
        write!(f, " (at {}:{})", self.location.file(), self.location.line())?;
        if f.alternate() && self.backtrace.status() == BacktraceStatus::Captured {
            write!(f, "\n\nBacktrace:\n{}", self.backtrace)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        if !self.reason.is_empty() {
            write!(f, ": {}", self.reason)?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// [`DecodeError`] をフレームレベルの接続エラーに昇格させる
///
/// フレーム decoder / encoder 経路でのバッファ不足・入力不足を
/// RFC 9113 §4.2 (Frame Size) に基づき FRAME_SIZE_ERROR に変換する。
///
/// HPACK 経路では本変換を使用しない。HPACK の入力不足は
/// [`ErrorKind::HpackError`] (COMPRESSION_ERROR 相当) で直接構築する。
impl From<DecodeError> for Error {
    #[track_caller]
    fn from(err: DecodeError) -> Self {
        Self::connection_error(ErrorCode::FrameSizeError, err.to_string())
    }
}

/// [`Result`] のエイリアス
pub type Result<T> = std::result::Result<T, Error>;
