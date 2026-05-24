//! HTTP/2 制限設定
//!
//! 接続やストリームの制限値を設定する。

use crate::settings::{
    DEFAULT_HEADER_TABLE_SIZE, DEFAULT_INITIAL_WINDOW_SIZE, DEFAULT_MAX_FRAME_SIZE, MaxFrameSize,
    WindowSize,
};

/// HTTP/2 接続の制限設定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    max_concurrent_streams: Option<u32>,
    initial_window_size: WindowSize,
    max_frame_size: MaxFrameSize,
    max_header_list_size: Option<u32>,
    header_table_size: u32,
    connection_window_size: WindowSize,
    enable_connect_protocol: bool,
    no_rfc7540_priorities: bool,
    wt_initial_max_data: Option<u32>,
    wt_initial_max_stream_data_uni: Option<u32>,
    wt_initial_max_stream_data_bidi_local: Option<u32>,
    wt_initial_max_streams_uni: Option<u32>,
    wt_initial_max_streams_bidi: Option<u32>,
    wt_initial_max_stream_data_bidi_remote: Option<u32>,
}

impl Limits {
    /// ビルダーを生成する
    pub const fn builder() -> LimitsBuilder {
        LimitsBuilder {
            max_concurrent_streams: Some(100),
            initial_window_size: WindowSize::from_static(DEFAULT_INITIAL_WINDOW_SIZE),
            max_frame_size: MaxFrameSize::from_static(DEFAULT_MAX_FRAME_SIZE),
            max_header_list_size: Some(16384),
            header_table_size: DEFAULT_HEADER_TABLE_SIZE,
            connection_window_size: WindowSize::from_static(DEFAULT_INITIAL_WINDOW_SIZE),
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

    /// 最大同時ストリーム数
    #[must_use]
    pub const fn max_concurrent_streams(&self) -> Option<u32> {
        self.max_concurrent_streams
    }

    /// 初期ウィンドウサイズ
    #[must_use]
    pub const fn initial_window_size(&self) -> WindowSize {
        self.initial_window_size
    }

    /// 最大フレームサイズ
    #[must_use]
    pub const fn max_frame_size(&self) -> MaxFrameSize {
        self.max_frame_size
    }

    /// 最大ヘッダーリストサイズ
    #[must_use]
    pub const fn max_header_list_size(&self) -> Option<u32> {
        self.max_header_list_size
    }

    /// HPACK 動的テーブルの最大サイズ
    #[must_use]
    pub const fn header_table_size(&self) -> u32 {
        self.header_table_size
    }

    /// 接続レベルの初期ウィンドウサイズ
    #[must_use]
    pub const fn connection_window_size(&self) -> WindowSize {
        self.connection_window_size
    }

    /// Extended CONNECT プロトコルの有効化 (RFC 8441)
    #[must_use]
    pub const fn enable_connect_protocol(&self) -> bool {
        self.enable_connect_protocol
    }

    /// RFC 9113 Section 5.3.1/5.3.2 で非推奨となった RFC 7540 由来の優先度の無効化 (RFC 9218)
    #[must_use]
    pub const fn no_rfc7540_priorities(&self) -> bool {
        self.no_rfc7540_priorities
    }

    /// SETTINGS_WT_INITIAL_MAX_DATA (0x2b61)
    #[must_use]
    pub const fn wt_initial_max_data(&self) -> Option<u32> {
        self.wt_initial_max_data
    }

    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI (0x2b62)
    #[must_use]
    pub const fn wt_initial_max_stream_data_uni(&self) -> Option<u32> {
        self.wt_initial_max_stream_data_uni
    }

    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL (0x2b63)
    #[must_use]
    pub const fn wt_initial_max_stream_data_bidi_local(&self) -> Option<u32> {
        self.wt_initial_max_stream_data_bidi_local
    }

    /// SETTINGS_WT_INITIAL_MAX_STREAMS_UNI (0x2b64)
    #[must_use]
    pub const fn wt_initial_max_streams_uni(&self) -> Option<u32> {
        self.wt_initial_max_streams_uni
    }

    /// SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI (0x2b65)
    #[must_use]
    pub const fn wt_initial_max_streams_bidi(&self) -> Option<u32> {
        self.wt_initial_max_streams_bidi
    }

    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE (0x2b66)
    #[must_use]
    pub const fn wt_initial_max_stream_data_bidi_remote(&self) -> Option<u32> {
        self.wt_initial_max_stream_data_bidi_remote
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits::builder()
            .build()
            .expect("default Limits values are always valid")
    }
}

/// `Limits` のビルダー
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitsBuilder {
    max_concurrent_streams: Option<u32>,
    initial_window_size: WindowSize,
    max_frame_size: MaxFrameSize,
    max_header_list_size: Option<u32>,
    header_table_size: u32,
    connection_window_size: WindowSize,
    enable_connect_protocol: bool,
    no_rfc7540_priorities: bool,
    wt_initial_max_data: Option<u32>,
    wt_initial_max_stream_data_uni: Option<u32>,
    wt_initial_max_stream_data_bidi_local: Option<u32>,
    wt_initial_max_streams_uni: Option<u32>,
    wt_initial_max_streams_bidi: Option<u32>,
    wt_initial_max_stream_data_bidi_remote: Option<u32>,
}

impl LimitsBuilder {
    /// 最大同時ストリーム数を設定する
    #[must_use]
    pub const fn max_concurrent_streams(mut self, max: Option<u32>) -> Self {
        self.max_concurrent_streams = max;
        self
    }

    /// 初期ウィンドウサイズを設定する
    #[must_use]
    pub const fn initial_window_size(mut self, size: WindowSize) -> Self {
        self.initial_window_size = size;
        self
    }

    /// 最大フレームサイズを設定する
    #[must_use]
    pub const fn max_frame_size(mut self, size: MaxFrameSize) -> Self {
        self.max_frame_size = size;
        self
    }

    /// 最大ヘッダーリストサイズを設定する
    #[must_use]
    pub const fn max_header_list_size(mut self, size: Option<u32>) -> Self {
        self.max_header_list_size = size;
        self
    }

    /// HPACK 動的テーブルの最大サイズを設定する
    #[must_use]
    pub const fn header_table_size(mut self, size: u32) -> Self {
        self.header_table_size = size;
        self
    }

    /// 接続レベルのウィンドウサイズを設定する
    #[must_use]
    pub const fn connection_window_size(mut self, size: WindowSize) -> Self {
        self.connection_window_size = size;
        self
    }

    /// Extended CONNECT プロトコルの有効化を設定する (RFC 8441)
    #[must_use]
    pub const fn enable_connect_protocol(mut self, enable: bool) -> Self {
        self.enable_connect_protocol = enable;
        self
    }

    /// RFC 9113 Section 5.3.1/5.3.2 で非推奨となった RFC 7540 由来の優先度の無効化を設定する (RFC 9218)
    #[must_use]
    pub const fn no_rfc7540_priorities(mut self, enable: bool) -> Self {
        self.no_rfc7540_priorities = enable;
        self
    }

    /// WebTransport 初期設定を一括設定する (draft-ietf-webtrans-http2-14 Section 11.2)
    #[must_use]
    pub const fn webtransport(
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

    /// 複合制約を検査して `Limits` を構築する
    ///
    /// # Errors
    ///
    /// - WebTransport 関連フィールドが設定されているのに `enable_connect_protocol = false`
    ///   → [`LimitsError::WebtransportRequiresConnectProtocol`]
    pub fn build(self) -> Result<Limits, LimitsError> {
        if !self.enable_connect_protocol && self.has_webtransport_settings() {
            return Err(LimitsError::WebtransportRequiresConnectProtocol);
        }

        Ok(Limits {
            max_concurrent_streams: self.max_concurrent_streams,
            initial_window_size: self.initial_window_size,
            max_frame_size: self.max_frame_size,
            max_header_list_size: self.max_header_list_size,
            header_table_size: self.header_table_size,
            connection_window_size: self.connection_window_size,
            enable_connect_protocol: self.enable_connect_protocol,
            no_rfc7540_priorities: self.no_rfc7540_priorities,
            wt_initial_max_data: self.wt_initial_max_data,
            wt_initial_max_stream_data_uni: self.wt_initial_max_stream_data_uni,
            wt_initial_max_stream_data_bidi_local: self.wt_initial_max_stream_data_bidi_local,
            wt_initial_max_streams_uni: self.wt_initial_max_streams_uni,
            wt_initial_max_streams_bidi: self.wt_initial_max_streams_bidi,
            wt_initial_max_stream_data_bidi_remote: self.wt_initial_max_stream_data_bidi_remote,
        })
    }

    /// const コンテキスト用。不正制約でコンパイル時 panic になる
    pub const fn build_static(self) -> Limits {
        if !self.enable_connect_protocol && self.has_webtransport_settings() {
            panic!(
                "LimitsBuilder::build_static: WebTransport settings require enable_connect_protocol = true"
            );
        }

        Limits {
            max_concurrent_streams: self.max_concurrent_streams,
            initial_window_size: self.initial_window_size,
            max_frame_size: self.max_frame_size,
            max_header_list_size: self.max_header_list_size,
            header_table_size: self.header_table_size,
            connection_window_size: self.connection_window_size,
            enable_connect_protocol: self.enable_connect_protocol,
            no_rfc7540_priorities: self.no_rfc7540_priorities,
            wt_initial_max_data: self.wt_initial_max_data,
            wt_initial_max_stream_data_uni: self.wt_initial_max_stream_data_uni,
            wt_initial_max_stream_data_bidi_local: self.wt_initial_max_stream_data_bidi_local,
            wt_initial_max_streams_uni: self.wt_initial_max_streams_uni,
            wt_initial_max_streams_bidi: self.wt_initial_max_streams_bidi,
            wt_initial_max_stream_data_bidi_remote: self.wt_initial_max_stream_data_bidi_remote,
        }
    }

    const fn has_webtransport_settings(&self) -> bool {
        self.wt_initial_max_data.is_some()
            || self.wt_initial_max_stream_data_uni.is_some()
            || self.wt_initial_max_stream_data_bidi_local.is_some()
            || self.wt_initial_max_streams_uni.is_some()
            || self.wt_initial_max_streams_bidi.is_some()
            || self.wt_initial_max_stream_data_bidi_remote.is_some()
    }
}

/// `Limits` 構築時検査エラー
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
