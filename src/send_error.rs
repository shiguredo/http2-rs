//! 送信時エラー型
//!
//! `Connection::send_*` 系の API が返す構築時検査エラー。
//! 文字列ベースの [`crate::error::Error`] とは分離し、違反値を構造化フィールドで保持する。
//!
//! 現時点では `Connection` の送信 API は従来の `Error` 型を使用しており、
//! この型は未統合。統合は送信 API のリファクタリング時に行う。

/// HTTP/2 送信エラー
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SendError {
    /// 接続が既にクローズされている
    ConnectionClosed,

    /// GOAWAY 送信済みのため新規ストリームを開けない
    GoawaySent,

    /// 指定したストリームが open ではない
    StreamNotOpen {
        /// 違反した stream_id
        stream_id: u32,
    },

    /// フロー制御ウィンドウが枯渇している
    FlowControlExhausted,

    /// ヘッダーリストサイズが受信側の `MAX_HEADER_LIST_SIZE` を超える
    ///
    /// RFC 9113 §6.5.2: `SETTINGS_MAX_HEADER_LIST_SIZE` は `u32`。
    HeaderListTooLarge {
        /// 実際のサイズ
        actual: u32,
        /// 上限
        limit: u32,
    },
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionClosed => write!(f, "connection is closed"),
            Self::GoawaySent => write!(f, "GOAWAY has been sent; no new streams"),
            Self::StreamNotOpen { stream_id } => {
                write!(f, "stream {stream_id} is not open")
            }
            Self::FlowControlExhausted => write!(f, "flow control window exhausted"),
            Self::HeaderListTooLarge { actual, limit } => {
                write!(f, "header list size {actual} exceeds limit {limit}")
            }
        }
    }
}

impl std::error::Error for SendError {}
