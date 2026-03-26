//! HTTP/2 SETTINGS パラメータ (RFC 9113 Section 6.5.2)
//!
//! # 拡張 SETTINGS
//!
//! - SETTINGS_ENABLE_CONNECT_PROTOCOL (RFC 8441): Extended CONNECT
//! - SETTINGS_WT_* (draft-ietf-webtrans-http2-14): WebTransport

/// SETTINGS_HEADER_TABLE_SIZE のデフォルト値
pub const DEFAULT_HEADER_TABLE_SIZE: u32 = 4096;

/// SETTINGS_ENABLE_PUSH のデフォルト値
///
/// RFC 9113 Section 8.4: サーバープッシュは主要ブラウザでサポートが削除されているため、
/// デフォルトで無効にする。
pub const DEFAULT_ENABLE_PUSH: bool = false;

/// SETTINGS_MAX_CONCURRENT_STREAMS のデフォルト値（無制限）
pub const DEFAULT_MAX_CONCURRENT_STREAMS: Option<u32> = None;

/// SETTINGS_INITIAL_WINDOW_SIZE のデフォルト値
pub const DEFAULT_INITIAL_WINDOW_SIZE: u32 = 65535;

/// SETTINGS_MAX_FRAME_SIZE のデフォルト値
pub const DEFAULT_MAX_FRAME_SIZE: u32 = 16384;

/// SETTINGS_MAX_HEADER_LIST_SIZE のデフォルト値（無制限）
pub const DEFAULT_MAX_HEADER_LIST_SIZE: Option<u32> = None;

/// SETTINGS_MAX_FRAME_SIZE の最小値
pub const MIN_MAX_FRAME_SIZE: u32 = 16384;

/// SETTINGS_MAX_FRAME_SIZE の最大値
pub const MAX_MAX_FRAME_SIZE: u32 = 16_777_215;

/// SETTINGS_INITIAL_WINDOW_SIZE の最大値
pub const MAX_INITIAL_WINDOW_SIZE: u32 = 2_147_483_647;

// === 拡張 SETTINGS (RFC 8441 + draft-ietf-webtrans-http2-14) ===

/// SETTINGS_ENABLE_CONNECT_PROTOCOL (RFC 8441)
/// Extended CONNECT Protocol を有効にする
pub const SETTINGS_ENABLE_CONNECT_PROTOCOL: u16 = 0x08;

/// SETTINGS_WT_INITIAL_MAX_DATA (draft-ietf-webtrans-http2-14 Section 11.2)
/// WebTransport セッションの初期最大データ量
pub const SETTINGS_WT_INITIAL_MAX_DATA: u16 = 0x2b61;

/// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI (draft-ietf-webtrans-http2-14 Section 11.2)
/// WebTransport 単方向ストリームの初期最大データ量
pub const SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI: u16 = 0x2b62;

/// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL (draft-ietf-webtrans-http2-14 Section 11.2)
/// WebTransport 双方向ストリームの初期最大データ量 (送信者が開始したストリーム)
pub const SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL: u16 = 0x2b63;

/// SETTINGS_WT_INITIAL_MAX_STREAMS_UNI (draft-ietf-webtrans-http2-14 Section 11.2)
/// WebTransport 単方向ストリームの初期最大数
pub const SETTINGS_WT_INITIAL_MAX_STREAMS_UNI: u16 = 0x2b64;

/// SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI (draft-ietf-webtrans-http2-14 Section 11.2)
/// WebTransport 双方向ストリームの初期最大数
pub const SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI: u16 = 0x2b65;

/// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE (draft-ietf-webtrans-http2-14 Section 11.2)
/// WebTransport 双方向ストリームの初期最大データ量 (受信者が開始したストリーム)
pub const SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE: u16 = 0x2b66;

/// SETTINGS パラメータの識別子
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum SettingId {
    /// SETTINGS_HEADER_TABLE_SIZE (0x01)
    HeaderTableSize = 0x01,
    /// SETTINGS_ENABLE_PUSH (0x02)
    EnablePush = 0x02,
    /// SETTINGS_MAX_CONCURRENT_STREAMS (0x03)
    MaxConcurrentStreams = 0x03,
    /// SETTINGS_INITIAL_WINDOW_SIZE (0x04)
    InitialWindowSize = 0x04,
    /// SETTINGS_MAX_FRAME_SIZE (0x05)
    MaxFrameSize = 0x05,
    /// SETTINGS_MAX_HEADER_LIST_SIZE (0x06)
    MaxHeaderListSize = 0x06,
    /// SETTINGS_ENABLE_CONNECT_PROTOCOL (0x08) (RFC 8441)
    ///
    /// Extended CONNECT Protocol を有効にする。
    /// 値が 1 の場合、CONNECT メソッドで :protocol 疑似ヘッダーを使用可能。
    EnableConnectProtocol = 0x08,
    /// SETTINGS_NO_RFC7540_PRIORITIES (0x09) (RFC 9218 Section 5.1)
    ///
    /// RFC 9218 で定義される設定パラメータ。
    /// 値が 1 の場合、RFC 7540 の優先度シグナリングを使用しないことを示す。
    NoRfc7540Priorities = 0x09,
}

impl SettingId {
    /// u16 から `SettingId` を生成する
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x01 => Some(Self::HeaderTableSize),
            0x02 => Some(Self::EnablePush),
            0x03 => Some(Self::MaxConcurrentStreams),
            0x04 => Some(Self::InitialWindowSize),
            0x05 => Some(Self::MaxFrameSize),
            0x06 => Some(Self::MaxHeaderListSize),
            0x08 => Some(Self::EnableConnectProtocol),
            0x09 => Some(Self::NoRfc7540Priorities),
            _ => None,
        }
    }

    /// `SettingId` を u16 に変換する
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self as u16
    }
}

/// 単一の SETTINGS パラメータ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    /// パラメータ識別子
    pub id: u16,
    /// パラメータ値
    pub value: u32,
}

impl Setting {
    /// 新しい `Setting` を生成する
    #[must_use]
    pub const fn new(id: u16, value: u32) -> Self {
        Self { id, value }
    }

    /// 既知の `SettingId` を使用して `Setting` を生成する
    #[must_use]
    pub const fn from_setting_id(id: SettingId, value: u32) -> Self {
        Self::new(id.as_u16(), value)
    }
}

/// HTTP/2 接続設定
///
/// ローカル側とリモート側の両方の SETTINGS を保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// HPACK 動的テーブルの最大サイズ
    pub header_table_size: u32,
    /// サーバープッシュの有効/無効
    pub enable_push: bool,
    /// 同時ストリーム数の上限
    pub max_concurrent_streams: Option<u32>,
    /// ストリームの初期ウィンドウサイズ
    pub initial_window_size: u32,
    /// フレームペイロードの最大サイズ
    pub max_frame_size: u32,
    /// ヘッダーリストの最大サイズ
    pub max_header_list_size: Option<u32>,
    /// Extended CONNECT Protocol の有効/無効 (RFC 8441)
    ///
    /// true の場合、CONNECT メソッドで :protocol 疑似ヘッダーを使用可能。
    /// WebSocket over HTTP/2 や WebTransport で必要。
    pub enable_connect_protocol: bool,
    /// RFC 7540 優先度シグナリングを使用しない (RFC 9218)
    ///
    /// true の場合、RFC 7540 の優先度シグナリング (PRIORITY フレーム、
    /// HEADERS フレームの優先度フィールド) を使用しないことを示す。
    pub no_rfc7540_priorities: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            header_table_size: DEFAULT_HEADER_TABLE_SIZE,
            enable_push: DEFAULT_ENABLE_PUSH,
            max_concurrent_streams: DEFAULT_MAX_CONCURRENT_STREAMS,
            initial_window_size: DEFAULT_INITIAL_WINDOW_SIZE,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            max_header_list_size: DEFAULT_MAX_HEADER_LIST_SIZE,
            enable_connect_protocol: false,
            no_rfc7540_priorities: false,
        }
    }
}

impl Settings {
    /// デフォルト設定で新しい `Settings` を生成する
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 設定パラメータを適用する
    ///
    /// 無効な値の場合は `Err` を返す。
    pub fn apply(&mut self, setting: Setting) -> Result<(), SettingsError> {
        match SettingId::from_u16(setting.id) {
            Some(SettingId::HeaderTableSize) => {
                self.header_table_size = setting.value;
            }
            Some(SettingId::EnablePush) => {
                if setting.value > 1 {
                    return Err(SettingsError::InvalidEnablePush(setting.value));
                }
                self.enable_push = setting.value == 1;
            }
            Some(SettingId::MaxConcurrentStreams) => {
                self.max_concurrent_streams = Some(setting.value);
            }
            Some(SettingId::InitialWindowSize) => {
                if setting.value > MAX_INITIAL_WINDOW_SIZE {
                    return Err(SettingsError::InvalidInitialWindowSize(setting.value));
                }
                self.initial_window_size = setting.value;
            }
            Some(SettingId::MaxFrameSize) => {
                if setting.value < MIN_MAX_FRAME_SIZE || setting.value > MAX_MAX_FRAME_SIZE {
                    return Err(SettingsError::InvalidMaxFrameSize(setting.value));
                }
                self.max_frame_size = setting.value;
            }
            Some(SettingId::MaxHeaderListSize) => {
                self.max_header_list_size = Some(setting.value);
            }
            Some(SettingId::EnableConnectProtocol) => {
                // RFC 8441: 0 または 1 のみ有効
                if setting.value > 1 {
                    return Err(SettingsError::InvalidEnableConnectProtocol(setting.value));
                }
                self.enable_connect_protocol = setting.value == 1;
            }
            Some(SettingId::NoRfc7540Priorities) => {
                // RFC 9218 Section 5.1: 0 または 1 のみ有効
                if setting.value > 1 {
                    return Err(SettingsError::InvalidNoRfc7540Priorities(setting.value));
                }
                self.no_rfc7540_priorities = setting.value == 1;
            }
            None => {
                // RFC 9113 Section 6.5.2: unknown settings MUST be ignored
            }
        }
        Ok(())
    }

    /// 設定を `Setting` のリストとして取得する
    #[must_use]
    pub fn to_settings_list(&self) -> Vec<Setting> {
        let mut list = Vec::with_capacity(8);
        list.push(Setting::from_setting_id(
            SettingId::HeaderTableSize,
            self.header_table_size,
        ));
        list.push(Setting::from_setting_id(
            SettingId::EnablePush,
            u32::from(self.enable_push),
        ));
        if let Some(max) = self.max_concurrent_streams {
            list.push(Setting::from_setting_id(
                SettingId::MaxConcurrentStreams,
                max,
            ));
        }
        list.push(Setting::from_setting_id(
            SettingId::InitialWindowSize,
            self.initial_window_size,
        ));
        list.push(Setting::from_setting_id(
            SettingId::MaxFrameSize,
            self.max_frame_size,
        ));
        if let Some(max) = self.max_header_list_size {
            list.push(Setting::from_setting_id(SettingId::MaxHeaderListSize, max));
        }
        // RFC 8441: Extended CONNECT Protocol を有効にする場合のみ送信
        if self.enable_connect_protocol {
            list.push(Setting::from_setting_id(
                SettingId::EnableConnectProtocol,
                1,
            ));
        }
        // RFC 9218: RFC 7540 優先度シグナリングを使用しない場合のみ送信
        // この設定は最初の SETTINGS フレームで送信し、以後変更できない
        if self.no_rfc7540_priorities {
            list.push(Setting::from_setting_id(SettingId::NoRfc7540Priorities, 1));
        }
        list
    }
}

/// SETTINGS パラメータのバリデーションエラー
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    /// ENABLE_PUSH の値が無効（0 または 1 以外）
    InvalidEnablePush(u32),
    /// INITIAL_WINDOW_SIZE の値が無効（2^31-1 を超える）
    InvalidInitialWindowSize(u32),
    /// MAX_FRAME_SIZE の値が無効（16384 未満または 16777215 を超える）
    InvalidMaxFrameSize(u32),
    /// ENABLE_CONNECT_PROTOCOL の値が無効（0 または 1 以外）
    InvalidEnableConnectProtocol(u32),
    /// NO_RFC7540_PRIORITIES の値が無効（0 または 1 以外）
    InvalidNoRfc7540Priorities(u32),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEnablePush(v) => {
                write!(f, "invalid ENABLE_PUSH value: {v} (must be 0 or 1)")
            }
            Self::InvalidInitialWindowSize(v) => {
                write!(
                    f,
                    "invalid INITIAL_WINDOW_SIZE value: {v} (must be <= 2147483647)"
                )
            }
            Self::InvalidMaxFrameSize(v) => {
                write!(
                    f,
                    "invalid MAX_FRAME_SIZE value: {v} (must be 16384..=16777215)"
                )
            }
            Self::InvalidEnableConnectProtocol(v) => {
                write!(
                    f,
                    "invalid ENABLE_CONNECT_PROTOCOL value: {v} (must be 0 or 1)"
                )
            }
            Self::InvalidNoRfc7540Priorities(v) => {
                write!(
                    f,
                    "invalid NO_RFC7540_PRIORITIES value: {v} (must be 0 or 1)"
                )
            }
        }
    }
}

impl std::error::Error for SettingsError {}

// === WebTransport SETTINGS 統合 (draft-ietf-webtrans-http2-14 Section 11.2) ===

/// WebTransport 初期設定
///
/// draft-ietf-webtrans-http2-14 Section 11.2 で定義される WebTransport 関連の
/// HTTP/2 SETTINGS パラメータ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WtInitialSettings {
    /// セッションレベルの初期最大データ量
    ///
    /// SETTINGS_WT_INITIAL_MAX_DATA (0x2b61)
    /// HTTP/2 SETTINGS の値は 32-bit に制限される (RFC 9113 Section 6.5.1)。
    pub initial_max_data: Option<u32>,
    /// 単方向ストリームの初期最大データ量
    ///
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI (0x2b62)
    /// HTTP/2 SETTINGS の値は 32-bit に制限される (RFC 9113 Section 6.5.1)。
    pub initial_max_stream_data_uni: Option<u32>,
    /// 双方向ストリームの初期最大データ量 (送信者が開始したストリーム)
    ///
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL (0x2b63)
    /// HTTP/2 SETTINGS の値は 32-bit に制限される (RFC 9113 Section 6.5.1)。
    pub initial_max_stream_data_bidi_local: Option<u32>,
    /// 双方向ストリームの初期最大データ量 (受信者が開始したストリーム)
    ///
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE (0x2b66)
    /// HTTP/2 SETTINGS の値は 32-bit に制限される (RFC 9113 Section 6.5.1)。
    pub initial_max_stream_data_bidi_remote: Option<u32>,
    /// 単方向ストリームの初期最大数
    ///
    /// SETTINGS_WT_INITIAL_MAX_STREAMS_UNI (0x2b64)
    /// HTTP/2 SETTINGS の値は 32-bit に制限される (RFC 9113 Section 6.5.1)。
    pub initial_max_streams_uni: Option<u32>,
    /// 双方向ストリームの初期最大数
    ///
    /// SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI (0x2b65)
    /// HTTP/2 SETTINGS の値は 32-bit に制限される (RFC 9113 Section 6.5.1)。
    pub initial_max_streams_bidi: Option<u32>,
}

impl WtInitialSettings {
    /// 新しい `WtInitialSettings` を生成する
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 設定パラメータを適用する
    ///
    /// WebTransport 関連の SETTINGS のみを処理し、その他は無視する。
    pub fn apply(&mut self, setting: Setting) {
        match setting.id {
            SETTINGS_WT_INITIAL_MAX_DATA => {
                self.initial_max_data = Some(setting.value);
            }
            SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI => {
                self.initial_max_stream_data_uni = Some(setting.value);
            }
            SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL => {
                self.initial_max_stream_data_bidi_local = Some(setting.value);
            }
            SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE => {
                self.initial_max_stream_data_bidi_remote = Some(setting.value);
            }
            SETTINGS_WT_INITIAL_MAX_STREAMS_UNI => {
                self.initial_max_streams_uni = Some(setting.value);
            }
            SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI => {
                self.initial_max_streams_bidi = Some(setting.value);
            }
            _ => {
                // WebTransport 関連以外の SETTINGS は無視
            }
        }
    }

    /// 設定を `Setting` のリストとして取得する
    #[must_use]
    pub fn to_settings_list(&self) -> Vec<Setting> {
        let mut list = Vec::new();
        if let Some(v) = self.initial_max_data {
            list.push(Setting::new(SETTINGS_WT_INITIAL_MAX_DATA, v));
        }
        if let Some(v) = self.initial_max_stream_data_uni {
            list.push(Setting::new(SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI, v));
        }
        if let Some(v) = self.initial_max_stream_data_bidi_local {
            list.push(Setting::new(
                SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL,
                v,
            ));
        }
        if let Some(v) = self.initial_max_stream_data_bidi_remote {
            list.push(Setting::new(
                SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE,
                v,
            ));
        }
        if let Some(v) = self.initial_max_streams_uni {
            list.push(Setting::new(SETTINGS_WT_INITIAL_MAX_STREAMS_UNI, v));
        }
        if let Some(v) = self.initial_max_streams_bidi {
            list.push(Setting::new(SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI, v));
        }
        list
    }
}

impl Settings {
    /// WebTransport 設定を適用する
    ///
    /// WebTransport 関連の SETTINGS パラメータを抽出して `WtInitialSettings` を生成する。
    #[must_use]
    pub fn apply_webtransport(&self, settings: &[Setting]) -> WtInitialSettings {
        let mut wt_settings = WtInitialSettings::new();
        for setting in settings {
            wt_settings.apply(*setting);
        }
        wt_settings
    }
}
