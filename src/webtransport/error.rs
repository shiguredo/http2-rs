//! WebTransport エラー型定義
//!
//! draft-ietf-webtrans-http2-15 で使用されるエラー型を提供する。

use std::backtrace::{Backtrace, BacktraceStatus};
use std::panic::Location;

/// WebTransport エラーの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WtErrorKind {
    /// 入力データが不足している
    Incomplete,

    /// バッファが不足している
    BufferTooShort,

    /// 入力データが無効
    InvalidInput,

    /// Capsule デコードエラー
    CapsuleDecode,

    /// 無効なストリーム ID
    InvalidStreamId,

    /// ストリーム状態エラー
    StreamStateError,

    /// フロー制御エラー
    FlowControlError,

    /// セッション状態エラー
    SessionStateError,

    /// セッションがクローズされた
    SessionClosed,
}

impl std::fmt::Display for WtErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incomplete => write!(f, "Incomplete"),
            Self::BufferTooShort => write!(f, "BufferTooShort"),
            Self::InvalidInput => write!(f, "InvalidInput"),
            Self::CapsuleDecode => write!(f, "CapsuleDecode"),
            Self::InvalidStreamId => write!(f, "InvalidStreamId"),
            Self::StreamStateError => write!(f, "StreamStateError"),
            Self::FlowControlError => write!(f, "FlowControlError"),
            Self::SessionStateError => write!(f, "SessionStateError"),
            Self::SessionClosed => write!(f, "SessionClosed"),
        }
    }
}

/// WebTransport エラー型
pub struct WtError {
    /// 発生したエラーの種類
    pub kind: WtErrorKind,

    /// エラーが発生した理由
    pub reason: String,

    /// エラーが作成されたソースコードの場所
    pub location: &'static Location<'static>,

    /// エラー発生箇所を示すバックトレース
    ///
    /// バックトレースは `RUST_BACKTRACE` 環境変数が設定されていない場合には取得されない
    pub backtrace: Backtrace,
}

impl WtError {
    /// [`WtError`] インスタンスを生成する
    #[track_caller]
    pub fn new(kind: WtErrorKind) -> Self {
        Self::with_reason(kind, String::new())
    }

    /// エラー理由つきで [`WtError`] インスタンスを生成する
    #[track_caller]
    pub fn with_reason<T: Into<String>>(kind: WtErrorKind, reason: T) -> Self {
        Self {
            kind,
            reason: reason.into(),
            location: Location::caller(),
            backtrace: Backtrace::capture(),
        }
    }

    /// 入力データ不足エラーを生成する
    #[track_caller]
    pub fn incomplete() -> Self {
        Self::new(WtErrorKind::Incomplete)
    }

    /// バッファ不足エラーを生成する
    #[track_caller]
    pub fn buffer_too_short() -> Self {
        Self::new(WtErrorKind::BufferTooShort)
    }

    /// 無効な入力エラーを生成する
    #[track_caller]
    pub fn invalid_input<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(WtErrorKind::InvalidInput, reason)
    }

    /// Capsule デコードエラーを生成する
    #[track_caller]
    pub fn capsule_decode<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(WtErrorKind::CapsuleDecode, reason)
    }

    /// 無効なストリーム ID エラーを生成する
    #[track_caller]
    pub fn invalid_stream_id<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(WtErrorKind::InvalidStreamId, reason)
    }

    /// ストリーム状態エラーを生成する
    #[track_caller]
    pub fn stream_state_error<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(WtErrorKind::StreamStateError, reason)
    }

    /// フロー制御エラーを生成する
    #[track_caller]
    pub fn flow_control_error<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(WtErrorKind::FlowControlError, reason)
    }

    /// セッション状態エラーを生成する
    #[track_caller]
    pub fn session_state_error<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(WtErrorKind::SessionStateError, reason)
    }

    /// セッションクローズエラーを生成する
    #[track_caller]
    pub fn session_closed<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(WtErrorKind::SessionClosed, reason)
    }
}

impl std::fmt::Debug for WtError {
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

impl std::fmt::Display for WtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        if !self.reason.is_empty() {
            write!(f, ": {}", self.reason)?;
        }
        Ok(())
    }
}

impl std::error::Error for WtError {}

/// [`WtResult`] のエイリアス
pub type WtResult<T> = std::result::Result<T, WtError>;
