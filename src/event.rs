//! HTTP/2 イベント定義
//!
//! Sans I/O パターンで使用されるイベントを定義する。

use bytes::Bytes;

use crate::error::ErrorCode;
use crate::frame::StreamId;
use crate::hpack::HeaderField;

/// HTTP/2 イベント
///
/// 接続から発生するイベントを表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// 接続プリフェイスを受信した
    ConnectionPreface,

    /// SETTINGS フレームを受信した
    SettingsReceived {
        /// ACK フラグ
        ack: bool,
    },

    /// ヘッダーを受信した
    HeadersReceived {
        /// ストリーム ID
        stream_id: StreamId,
        /// ヘッダーフィールド
        headers: Vec<HeaderField>,
        /// END_STREAM フラグ
        end_stream: bool,
        /// Extended CONNECT の :protocol 擬似ヘッダー値 (RFC 8441)
        ///
        /// `:method = CONNECT` かつ `:protocol` が指定されていた場合に `Some` になる。
        /// それ以外は `None`。WebTransport over HTTP/2 では `Some(b"webtransport")` が入る。
        protocol: Option<Bytes>,
    },

    /// データを受信した
    DataReceived {
        /// ストリーム ID
        stream_id: StreamId,
        /// データ
        data: Bytes,
        /// END_STREAM フラグ
        end_stream: bool,
    },

    /// トレーラーを受信した
    TrailersReceived {
        /// ストリーム ID
        stream_id: StreamId,
        /// トレーラーフィールド
        trailers: Vec<HeaderField>,
    },

    /// ストリームがリセットされた
    StreamReset {
        /// ストリーム ID
        stream_id: StreamId,
        /// エラーコード
        error_code: ErrorCode,
    },

    /// ストリームがクローズされた
    StreamClosed {
        /// ストリーム ID
        stream_id: StreamId,
    },

    /// PING を受信した
    PingReceived {
        /// 不透明データ
        opaque_data: [u8; 8],
        /// ACK フラグ
        ack: bool,
    },

    /// GOAWAY を受信した
    GoawayReceived {
        /// 最後に処理したストリーム ID
        last_stream_id: StreamId,
        /// エラーコード
        error_code: ErrorCode,
        /// デバッグデータ
        debug_data: Bytes,
    },

    /// WINDOW_UPDATE を受信した
    WindowUpdateReceived {
        /// ストリーム ID（0 の場合は接続レベル）
        stream_id: StreamId,
        /// ウィンドウサイズ増分
        increment: u32,
    },

    /// PRIORITY_UPDATE を受信した (RFC 9218)
    PriorityUpdateReceived {
        /// 優先度を更新するストリーム ID
        stream_id: StreamId,
        /// Priority Field Value (Structured Fields 形式)
        priority_field_value: Bytes,
    },

    /// 接続エラーが発生した
    ConnectionError {
        /// エラーコード
        error_code: ErrorCode,
        /// 理由
        reason: String,
    },
}

impl Event {
    /// イベントがストリームに関連するかどうかを返す
    #[must_use]
    pub const fn stream_id(&self) -> Option<StreamId> {
        match self {
            Self::HeadersReceived { stream_id, .. }
            | Self::DataReceived { stream_id, .. }
            | Self::TrailersReceived { stream_id, .. }
            | Self::StreamReset { stream_id, .. }
            | Self::StreamClosed { stream_id }
            | Self::PriorityUpdateReceived { stream_id, .. } => Some(*stream_id),
            Self::WindowUpdateReceived { stream_id, .. } if *stream_id != 0 => Some(*stream_id),
            _ => None,
        }
    }

    /// イベントが接続レベルかどうかを返す
    #[must_use]
    pub const fn is_connection_level(&self) -> bool {
        matches!(
            self,
            Self::ConnectionPreface
                | Self::SettingsReceived { .. }
                | Self::PingReceived { .. }
                | Self::GoawayReceived { .. }
                | Self::ConnectionError { .. }
        ) || matches!(self, Self::WindowUpdateReceived { stream_id, .. } if *stream_id == 0)
    }
}
