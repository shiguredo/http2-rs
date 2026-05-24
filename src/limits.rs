//! HTTP/2 制限設定
//!
//! 接続やストリームの制限値を設定する。

use crate::settings::{
    DEFAULT_HEADER_TABLE_SIZE, DEFAULT_INITIAL_WINDOW_SIZE, DEFAULT_MAX_FRAME_SIZE,
    MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, MIN_MAX_FRAME_SIZE,
};

/// HTTP/2 接続の制限設定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// 最大同時ストリーム数
    pub max_concurrent_streams: Option<u32>,
    /// 初期ウィンドウサイズ
    pub initial_window_size: u32,
    /// 最大フレームサイズ
    pub max_frame_size: u32,
    /// 最大ヘッダーリストサイズ
    pub max_header_list_size: Option<u32>,
    /// HPACK 動的テーブルの最大サイズ
    pub header_table_size: u32,
    /// 接続レベルの初期ウィンドウサイズ
    pub connection_window_size: u32,
    /// Extended CONNECT プロトコルの有効化 (RFC 8441)
    pub enable_connect_protocol: bool,
    /// RFC 9113 Section 5.3.1/5.3.2 で非推奨となった RFC 7540 由来の優先度の無効化 (RFC 9218)
    pub no_rfc7540_priorities: bool,
    /// SETTINGS_WT_INITIAL_MAX_DATA (0x2b61)
    pub wt_initial_max_data: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI (0x2b62)
    pub wt_initial_max_stream_data_uni: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL (0x2b63)
    pub wt_initial_max_stream_data_bidi_local: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAMS_UNI (0x2b64)
    pub wt_initial_max_streams_uni: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI (0x2b65)
    pub wt_initial_max_streams_bidi: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE (0x2b66)
    pub wt_initial_max_stream_data_bidi_remote: Option<u32>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_concurrent_streams: Some(100),
            initial_window_size: DEFAULT_INITIAL_WINDOW_SIZE,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            max_header_list_size: Some(16384),
            header_table_size: DEFAULT_HEADER_TABLE_SIZE,
            connection_window_size: DEFAULT_INITIAL_WINDOW_SIZE,
            enable_connect_protocol: false,
            no_rfc7540_priorities: false,
            wt_initial_max_data: None,
            wt_initial_max_stream_data_uni: None,
            wt_initial_max_stream_data_bidi_local: None,
            wt_initial_max_streams_uni: None,
            wt_initial_max_streams_bidi: None,
            wt_initial_max_stream_data_bidi_remote: None,
        }
    }
}

impl Limits {
    /// 新しい制限設定を生成する
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 最大同時ストリーム数を設定する
    #[must_use]
    pub const fn with_max_concurrent_streams(mut self, max: Option<u32>) -> Self {
        self.max_concurrent_streams = max;
        self
    }

    /// 初期ウィンドウサイズを設定する
    ///
    /// # Panics
    ///
    /// 値が `MAX_INITIAL_WINDOW_SIZE` を超える場合
    #[must_use]
    pub fn with_initial_window_size(mut self, size: u32) -> Self {
        assert!(
            size <= MAX_INITIAL_WINDOW_SIZE,
            "initial window size must be <= {MAX_INITIAL_WINDOW_SIZE}"
        );
        self.initial_window_size = size;
        self
    }

    /// 最大フレームサイズを設定する
    ///
    /// # Panics
    ///
    /// 値が `MIN_MAX_FRAME_SIZE` 未満または `MAX_MAX_FRAME_SIZE` を超える場合
    #[must_use]
    pub fn with_max_frame_size(mut self, size: u32) -> Self {
        assert!(
            (MIN_MAX_FRAME_SIZE..=MAX_MAX_FRAME_SIZE).contains(&size),
            "max frame size must be {MIN_MAX_FRAME_SIZE}..={MAX_MAX_FRAME_SIZE}"
        );
        self.max_frame_size = size;
        self
    }

    /// 最大ヘッダーリストサイズを設定する
    #[must_use]
    pub const fn with_max_header_list_size(mut self, size: Option<u32>) -> Self {
        self.max_header_list_size = size;
        self
    }

    /// HPACK 動的テーブルの最大サイズを設定する
    #[must_use]
    pub const fn with_header_table_size(mut self, size: u32) -> Self {
        self.header_table_size = size;
        self
    }

    /// 接続レベルのウィンドウサイズを設定する
    #[must_use]
    pub fn with_connection_window_size(mut self, size: u32) -> Self {
        assert!(
            size <= MAX_INITIAL_WINDOW_SIZE,
            "connection window size must be <= {MAX_INITIAL_WINDOW_SIZE}"
        );
        self.connection_window_size = size;
        self
    }

    /// Extended CONNECT プロトコルの有効化を設定する (RFC 8441)
    #[must_use]
    pub const fn with_enable_connect_protocol(mut self, enable: bool) -> Self {
        self.enable_connect_protocol = enable;
        self
    }

    /// RFC 9113 Section 5.3.1/5.3.2 で非推奨となった RFC 7540 由来の優先度の無効化を設定する (RFC 9218)
    #[must_use]
    pub const fn with_no_rfc7540_priorities(mut self, enable: bool) -> Self {
        self.no_rfc7540_priorities = enable;
        self
    }

    /// WebTransport 初期設定を一括設定する (draft-ietf-webtrans-http2-14 Section 11.2)
    ///
    /// WebTransport サーバーを構築する場合、`with_enable_connect_protocol(true)` と
    /// 併用して初期 SETTINGS に WebTransport 関連パラメータを広告する。
    #[must_use]
    pub const fn with_webtransport(
        mut self,
        max_data: Option<u32>,
        max_stream_data_uni: Option<u32>,
        max_stream_data_bidi_local: Option<u32>,
        max_streams_uni: Option<u32>,
        max_streams_bidi: Option<u32>,
        max_stream_data_bidi_remote: Option<u32>,
    ) -> Self {
        self.wt_initial_max_data = max_data;
        self.wt_initial_max_stream_data_uni = max_stream_data_uni;
        self.wt_initial_max_stream_data_bidi_local = max_stream_data_bidi_local;
        self.wt_initial_max_streams_uni = max_streams_uni;
        self.wt_initial_max_streams_bidi = max_streams_bidi;
        self.wt_initial_max_stream_data_bidi_remote = max_stream_data_bidi_remote;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_limits() {
        let limits = Limits::default();
        assert_eq!(limits.max_concurrent_streams, Some(100));
        assert_eq!(limits.initial_window_size, 65535);
        assert_eq!(limits.max_frame_size, 16384);
    }

    #[test]
    fn test_builder_pattern() {
        let limits = Limits::new()
            .with_max_concurrent_streams(Some(50))
            .with_initial_window_size(1 << 20)
            .with_max_frame_size(1 << 20);

        assert_eq!(limits.max_concurrent_streams, Some(50));
        assert_eq!(limits.initial_window_size, 1 << 20);
        assert_eq!(limits.max_frame_size, 1 << 20);
    }

    #[test]
    #[should_panic]
    fn test_invalid_initial_window_size() {
        let _ = Limits::new().with_initial_window_size(MAX_INITIAL_WINDOW_SIZE + 1);
    }

    #[test]
    #[should_panic]
    fn test_invalid_max_frame_size_too_small() {
        let _ = Limits::new().with_max_frame_size(MIN_MAX_FRAME_SIZE - 1);
    }

    #[test]
    #[should_panic]
    fn test_invalid_max_frame_size_too_large() {
        let _ = Limits::new().with_max_frame_size(MAX_MAX_FRAME_SIZE + 1);
    }
}

// === issue 0028 / 0029: Limits 構築時検査エラー (Phase 1) ===
//
// 既存の `with_*` ビルダーは Phase 2 で `LimitsBuilder::build() -> Result` に
// 置き換えられる予定。Phase 1 では `LimitsError` 型のみ追加する。

/// `Limits` 構築時検査エラー (issue 0028 / 0029)
///
/// 注: Phase 2 で `Limits::with_initial_window_size` 等の `assert!` 経路を
/// `LimitsBuilder::build() -> Result<Limits, LimitsError>` に置き換える際、
/// `InitialWindowSizeOutOfRange` / `MaxFrameSizeOutOfRange` /
/// `ConnectionWindowSizeOutOfRange` などの variant を追加する予定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LimitsError {
    /// WebTransport 関連設定があるのに `enable_connect_protocol = false`
    ///
    /// draft-ietf-webtrans-http2-14 §3.1: サーバーは WebTransport 対応を示すために
    /// `SETTINGS_ENABLE_CONNECT_PROTOCOL = 1` を SETTINGS フレームで MUST 送信する。
    /// SETTINGS の定義は §11.2 (将来変更される可能性がある)。
    WebtransportRequiresConnectProtocol,
}

impl std::fmt::Display for LimitsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WebtransportRequiresConnectProtocol => write!(
                f,
                "WebTransport settings require enable_connect_protocol = true"
            ),
        }
    }
}

impl std::error::Error for LimitsError {}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn display_webtransport_requires_connect_protocol() {
        assert_eq!(
            LimitsError::WebtransportRequiresConnectProtocol.to_string(),
            "WebTransport settings require enable_connect_protocol = true"
        );
    }
}
