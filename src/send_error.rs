//! 送信時エラー型 (issue 0029)
//!
//! `Connection::send_*` 系の API が返す構築時検査エラー。
//! 文字列ベースの [`crate::error::Error`] とは分離し、違反値を構造化フィールドで保持する。
//!
//! 現時点では `Connection` の送信 API は従来の `Error` 型を使用しており、
//! この型は未統合。統合は送信 API のリファクタリング時に行う。

/// HTTP/2 送信エラー
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_connection_closed() {
        assert_eq!(
            SendError::ConnectionClosed.to_string(),
            "connection is closed"
        );
    }

    #[test]
    fn display_goaway_sent() {
        assert_eq!(
            SendError::GoawaySent.to_string(),
            "GOAWAY has been sent; no new streams"
        );
    }

    #[test]
    fn display_stream_not_open() {
        assert_eq!(
            SendError::StreamNotOpen { stream_id: 5 }.to_string(),
            "stream 5 is not open"
        );
    }

    #[test]
    fn display_flow_control_exhausted() {
        assert_eq!(
            SendError::FlowControlExhausted.to_string(),
            "flow control window exhausted"
        );
    }

    #[test]
    fn display_header_list_too_large() {
        let err = SendError::HeaderListTooLarge {
            actual: 16385,
            limit: 16384,
        };
        assert_eq!(
            err.to_string(),
            "header list size 16385 exceeds limit 16384"
        );
    }
}
