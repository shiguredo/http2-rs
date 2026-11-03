//! WebTransport エラー型定義
//!
//! draft-ietf-webtrans-http2-16 で使用されるエラー型を提供する。

use std::backtrace::{Backtrace, BacktraceStatus};
use std::panic::Location;

/// WebTransport エラーの種類
///
/// draft-ietf-webtrans-http2-16 Section 3.4 は、ストリーム状態の違反を
/// WT_STREAM_STATE_ERROR、フロー制御上限の違反を WT_FLOW_CONTROL_ERROR、
/// より具体的なエラーコードを持たない違反を WT_ERROR のセッションエラーとして扱う。
/// これらが受信した capsule の検証で生じた場合、tokio-http2 の driver は
/// CONNECT ストリームを RST_STREAM で終了してセッションを終了させる。
/// ローカル API の検証エラーとして返る場合は呼び出し側へエラーを返すだけで、
/// セッションは継続する。
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
    ///
    /// draft-ietf-webtrans-http2-16 Section 6.2 / Section 6.3 / Section 6.4 /
    /// Section 6.6 / Section 6.9 が定める WT_STREAM_STATE_ERROR に対応する。
    StreamStateError,

    /// フロー制御エラー
    ///
    /// draft-ietf-webtrans-http2-16 Section 6.4 / Section 6.5 / Section 6.6 /
    /// Section 6.7 / Section 6.10 が定める WT_FLOW_CONTROL_ERROR に対応する。
    FlowControlError,

    /// セッション状態エラー
    ///
    /// draft-ietf-webtrans-http2-16 Section 3.4 が定める WT_ERROR に対応する。
    /// セッション状態の遷移違反に加え、より具体的なエラーコードを持たない
    /// 受信 capsule の検証失敗 (Application Protocol Error Code の 32 bit 超過、
    /// WT_CLOSE_SESSION の理由長超過・非 UTF-8 など) を含む。
    SessionStateError,
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
        }
    }
}

/// WebTransport エラー型
pub struct WtError {
    /// 発生したエラーの種類
    kind: WtErrorKind,

    /// エラーが発生した理由
    reason: String,

    /// エラーが作成されたソースコードの場所
    location: &'static Location<'static>,

    /// エラー発生箇所を示すバックトレース
    ///
    /// バックトレースは `RUST_BACKTRACE` 環境変数が設定されていない場合には取得されない
    backtrace: Backtrace,
}

impl WtError {
    /// エラー種別
    #[must_use]
    pub const fn kind(&self) -> WtErrorKind {
        self.kind
    }

    /// エラー理由
    #[must_use]
    pub const fn reason(&self) -> &str {
        self.reason.as_str()
    }

    /// エラー発生位置
    #[must_use]
    pub const fn location(&self) -> &'static Location<'static> {
        self.location
    }

    /// バックトレース
    ///
    /// `RUST_BACKTRACE` 環境変数が設定されていない場合には取得されない
    #[must_use = "returns the captured backtrace"]
    pub const fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }

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
