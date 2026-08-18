//! HTTP/2 SETTINGS パラメータ (RFC 9113 Section 6.5.2)
//!
//! # 拡張 SETTINGS
//!
//! - SETTINGS_ENABLE_CONNECT_PROTOCOL (RFC 8441): Extended CONNECT
//! - SETTINGS_WT_ENABLED (draft-ietf-webtrans-http2-15 Section 3.1 / Section 11.2): WebTransport サポート合図
//! - SETTINGS_WT_INITIAL_MAX_* (draft-ietf-webtrans-http2-15 Section 11.2): WebTransport 初期フロー制御

/// SETTINGS_HEADER_TABLE_SIZE のデフォルト値
pub const DEFAULT_HEADER_TABLE_SIZE: u32 = 4096;

/// SETTINGS_ENABLE_PUSH のデフォルト値
///
/// RFC 9113 Section 6.5.2 の初期値は 1 だが、サーバープッシュは実効性が低く (RFC 9113 Section 8.4)、
/// 主要ブラウザでもサポートが削除されているため、本ライブラリではデフォルトで無効にする (実装判断)。
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

/// 既知の SETTINGS パラメータ (RFC 9113 §6.5.2)
///
/// wire 上の (id, value) ペアから `Setting::from_wire` で構築する。
/// 既知パラメータの値が範囲外の場合は `Err(SettingError)` を返す。
/// 未知 ID は `Setting::Unknown { id, value }` として保持する
/// (RFC 9113 §6.5.2: 未知パラメータは MUST ignore)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// SETTINGS_HEADER_TABLE_SIZE (0x01)
    HeaderTableSize(u32),
    /// SETTINGS_ENABLE_PUSH (0x02)
    EnablePush(bool),
    /// SETTINGS_MAX_CONCURRENT_STREAMS (0x03)
    MaxConcurrentStreams(u32),
    /// SETTINGS_INITIAL_WINDOW_SIZE (0x04)
    InitialWindowSize(WindowSize),
    /// SETTINGS_MAX_FRAME_SIZE (0x05)
    MaxFrameSize(MaxFrameSize),
    /// SETTINGS_MAX_HEADER_LIST_SIZE (0x06)
    MaxHeaderListSize(u32),
    /// SETTINGS_ENABLE_CONNECT_PROTOCOL (0x08) (RFC 8441)
    EnableConnectProtocol(bool),
    /// SETTINGS_NO_RFC7540_PRIORITIES (0x09) (RFC 9218 Section 2.1)
    NoRfc7540Priorities(bool),
    /// SETTINGS_WT_ENABLED (0x2b60) (draft-ietf-webtrans-http2-15 Section 3.1 / Section 11.2)
    ///
    /// サーバーの WebTransport サポート合図。デフォルト値は 0 (非サポート)。
    /// 値は 0 または 1 のみ。クライアントは 1 より大きい値を
    /// 接続エラー PROTOCOL_ERROR として扱わなければならない (MUST)。
    ///
    /// 注: この識別子と値は draft-ietf-webtrans-http2-15 由来の暫定値であり、
    /// IANA 登録後に変更される可能性がある。
    WtEnabled(bool),
    /// SETTINGS_WT_INITIAL_MAX_DATA (0x2b61) (draft-ietf-webtrans-http2-15 Section 11.2)
    ///
    /// 注: この識別子と値は draft-ietf-webtrans-http2-15 由来の暫定値であり、
    /// IANA 登録後に変更される可能性がある。
    WtInitialMaxData(u32),
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI (0x2b62) (draft-ietf-webtrans-http2-15 Section 11.2)
    ///
    /// 注: この識別子と値は draft-ietf-webtrans-http2-15 由来の暫定値であり、
    /// IANA 登録後に変更される可能性がある。
    WtInitialMaxStreamDataUni(u32),
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL (0x2b63) (draft-ietf-webtrans-http2-15 Section 11.2)
    ///
    /// 注: この識別子と値は draft-ietf-webtrans-http2-15 由来の暫定値であり、
    /// IANA 登録後に変更される可能性がある。
    WtInitialMaxStreamDataBidiLocal(u32),
    /// SETTINGS_WT_INITIAL_MAX_STREAMS_UNI (0x2b64) (draft-ietf-webtrans-http2-15 Section 11.2)
    ///
    /// 注: この識別子と値は draft-ietf-webtrans-http2-15 由来の暫定値であり、
    /// IANA 登録後に変更される可能性がある。
    WtInitialMaxStreamsUni(u32),
    /// SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI (0x2b65) (draft-ietf-webtrans-http2-15 Section 11.2)
    ///
    /// 注: この識別子と値は draft-ietf-webtrans-http2-15 由来の暫定値であり、
    /// IANA 登録後に変更される可能性がある。
    WtInitialMaxStreamsBidi(u32),
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE (0x2b66) (draft-ietf-webtrans-http2-15 Section 11.2)
    ///
    /// 注: この識別子と値は draft-ietf-webtrans-http2-15 由来の暫定値であり、
    /// IANA 登録後に変更される可能性がある。
    WtInitialMaxStreamDataBidiRemote(u32),
    /// 未知の SETTINGS パラメータ (RFC 9113 §6.5.2: MUST ignore)
    Unknown { id: u16, value: u32 },
}

impl Setting {
    /// wire 上の (id, value) ペアから構築する
    ///
    /// 既知パラメータの値が範囲外の場合は `Err(SettingError)` を返す。
    /// 未知の ID の場合は `Ok(Setting::Unknown { id, value })` を返す。
    pub fn from_wire(id: u16, value: u32) -> Result<Self, SettingError> {
        match id {
            0x01 => Ok(Self::HeaderTableSize(value)),
            0x02 => {
                if value > 1 {
                    return Err(SettingError::EnablePushNotBoolean { value });
                }
                Ok(Self::EnablePush(value == 1))
            }
            0x03 => Ok(Self::MaxConcurrentStreams(value)),
            0x04 => Ok(Self::InitialWindowSize(WindowSize::new(value)?)),
            0x05 => Ok(Self::MaxFrameSize(MaxFrameSize::new(value)?)),
            0x06 => Ok(Self::MaxHeaderListSize(value)),
            0x08 => {
                if value > 1 {
                    return Err(SettingError::EnableConnectProtocolNotBoolean { value });
                }
                Ok(Self::EnableConnectProtocol(value == 1))
            }
            0x09 => {
                if value > 1 {
                    return Err(SettingError::NoRfc7540PrioritiesNotBoolean { value });
                }
                Ok(Self::NoRfc7540Priorities(value == 1))
            }
            0x2b60 => {
                if value > 1 {
                    return Err(SettingError::WtEnabledNotBoolean { value });
                }
                Ok(Self::WtEnabled(value == 1))
            }
            0x2b61 => Ok(Self::WtInitialMaxData(value)),
            0x2b62 => Ok(Self::WtInitialMaxStreamDataUni(value)),
            0x2b63 => Ok(Self::WtInitialMaxStreamDataBidiLocal(value)),
            0x2b64 => Ok(Self::WtInitialMaxStreamsUni(value)),
            0x2b65 => Ok(Self::WtInitialMaxStreamsBidi(value)),
            0x2b66 => Ok(Self::WtInitialMaxStreamDataBidiRemote(value)),
            _ => Ok(Self::Unknown { id, value }),
        }
    }

    /// wire 上の (id, value) ペアに変換する
    pub const fn as_wire(self) -> (u16, u32) {
        match self {
            Self::HeaderTableSize(v) => (0x01, v),
            Self::EnablePush(b) => (0x02, b as u32),
            Self::MaxConcurrentStreams(v) => (0x03, v),
            Self::InitialWindowSize(ws) => (0x04, ws.get()),
            Self::MaxFrameSize(mfs) => (0x05, mfs.get()),
            Self::MaxHeaderListSize(v) => (0x06, v),
            Self::EnableConnectProtocol(b) => (0x08, b as u32),
            Self::NoRfc7540Priorities(b) => (0x09, b as u32),
            Self::WtEnabled(b) => (0x2b60, b as u32),
            Self::WtInitialMaxData(v) => (0x2b61, v),
            Self::WtInitialMaxStreamDataUni(v) => (0x2b62, v),
            Self::WtInitialMaxStreamDataBidiLocal(v) => (0x2b63, v),
            Self::WtInitialMaxStreamsUni(v) => (0x2b64, v),
            Self::WtInitialMaxStreamsBidi(v) => (0x2b65, v),
            Self::WtInitialMaxStreamDataBidiRemote(v) => (0x2b66, v),
            Self::Unknown { id, value } => (id, value),
        }
    }
}

/// HTTP/2 接続設定
///
/// ローカル側とリモート側の両方の SETTINGS を保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// HPACK 動的テーブルの最大サイズ
    header_table_size: u32,
    /// サーバープッシュの有効/無効
    enable_push: bool,
    /// 同時ストリーム数の上限
    max_concurrent_streams: Option<u32>,
    /// ストリームの初期ウィンドウサイズ
    initial_window_size: WindowSize,
    /// フレームペイロードの最大サイズ
    max_frame_size: MaxFrameSize,
    /// ヘッダーリストの最大サイズ
    max_header_list_size: Option<u32>,
    /// Extended CONNECT Protocol の有効/無効 (RFC 8441)
    enable_connect_protocol: bool,
    /// RFC 9113 Section 5.3.1/5.3.2 で非推奨となった RFC 7540 由来の優先度シグナリングを
    /// 使用しない (RFC 9218)
    no_rfc7540_priorities: bool,
    /// SETTINGS_WT_ENABLED (0x2b60) (draft-ietf-webtrans-http2-15 Section 3.1)
    ///
    /// サーバーの WebTransport サポート合図。デフォルト値は false (非サポート)。
    wt_enabled: bool,
    /// SETTINGS_WT_INITIAL_MAX_DATA (0x2b61)
    wt_initial_max_data: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_UNI (0x2b62)
    wt_initial_max_stream_data_uni: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL (0x2b63)
    wt_initial_max_stream_data_bidi_local: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAMS_UNI (0x2b64)
    wt_initial_max_streams_uni: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAMS_BIDI (0x2b65)
    wt_initial_max_streams_bidi: Option<u32>,
    /// SETTINGS_WT_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE (0x2b66)
    wt_initial_max_stream_data_bidi_remote: Option<u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            header_table_size: DEFAULT_HEADER_TABLE_SIZE,
            enable_push: DEFAULT_ENABLE_PUSH,
            max_concurrent_streams: DEFAULT_MAX_CONCURRENT_STREAMS,
            initial_window_size: WindowSize::from_static(DEFAULT_INITIAL_WINDOW_SIZE),
            max_frame_size: MaxFrameSize::from_static(DEFAULT_MAX_FRAME_SIZE),
            max_header_list_size: DEFAULT_MAX_HEADER_LIST_SIZE,
            enable_connect_protocol: false,
            no_rfc7540_priorities: false,
            wt_enabled: false,
            wt_initial_max_data: None,
            wt_initial_max_stream_data_uni: None,
            wt_initial_max_stream_data_bidi_local: None,
            wt_initial_max_streams_uni: None,
            wt_initial_max_streams_bidi: None,
            wt_initial_max_stream_data_bidi_remote: None,
        }
    }
}

impl Settings {
    /// デフォルト設定で新しい `Settings` を生成する
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `Limits` から `Settings` を構築する
    ///
    /// `Limits` に存在しないフィールド (`enable_push`) はデフォルト値で初期化する。
    /// `Limits` にしか存在しないフィールド (`connection_window_size`) は使用しない
    /// (`Connection` 側で別途参照する)。
    pub(crate) fn from_limits(limits: &crate::limits::Limits) -> Self {
        Self {
            header_table_size: limits.header_table_size(),
            enable_push: DEFAULT_ENABLE_PUSH,
            max_concurrent_streams: limits.max_concurrent_streams(),
            initial_window_size: limits.initial_window_size(),
            max_frame_size: limits.max_frame_size(),
            max_header_list_size: limits.max_header_list_size(),
            enable_connect_protocol: limits.enable_connect_protocol(),
            no_rfc7540_priorities: limits.no_rfc7540_priorities(),
            wt_enabled: limits.wt_enabled(),
            wt_initial_max_data: limits.wt_initial_max_data(),
            wt_initial_max_stream_data_uni: limits.wt_initial_max_stream_data_uni(),
            wt_initial_max_stream_data_bidi_local: limits.wt_initial_max_stream_data_bidi_local(),
            wt_initial_max_streams_uni: limits.wt_initial_max_streams_uni(),
            wt_initial_max_streams_bidi: limits.wt_initial_max_streams_bidi(),
            wt_initial_max_stream_data_bidi_remote: limits.wt_initial_max_stream_data_bidi_remote(),
        }
    }

    /// HPACK 動的テーブルの最大サイズ
    #[must_use]
    pub const fn header_table_size(&self) -> u32 {
        self.header_table_size
    }

    /// サーバープッシュの有効/無効
    #[must_use]
    pub const fn enable_push(&self) -> bool {
        self.enable_push
    }

    /// 同時ストリーム数の上限
    #[must_use]
    pub const fn max_concurrent_streams(&self) -> Option<u32> {
        self.max_concurrent_streams
    }

    /// ストリームの初期ウィンドウサイズ
    #[must_use]
    pub const fn initial_window_size(&self) -> WindowSize {
        self.initial_window_size
    }

    /// フレームペイロードの最大サイズ
    #[must_use]
    pub const fn max_frame_size(&self) -> MaxFrameSize {
        self.max_frame_size
    }

    /// ヘッダーリストの最大サイズ
    #[must_use]
    pub const fn max_header_list_size(&self) -> Option<u32> {
        self.max_header_list_size
    }

    /// Extended CONNECT Protocol の有効/無効 (RFC 8441)
    #[must_use]
    pub const fn enable_connect_protocol(&self) -> bool {
        self.enable_connect_protocol
    }

    /// RFC 7540 由来の優先度シグナリングを使用しないかどうか (RFC 9218)
    #[must_use]
    pub const fn no_rfc7540_priorities(&self) -> bool {
        self.no_rfc7540_priorities
    }

    /// SETTINGS_WT_ENABLED (0x2b60) (draft-ietf-webtrans-http2-15 Section 3.1)
    ///
    /// サーバーの WebTransport サポート合図。
    #[must_use]
    pub const fn wt_enabled(&self) -> bool {
        self.wt_enabled
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

    /// 検証済み `Setting` を適用する
    ///
    /// `Setting` は `from_wire` で構築済みのため値検査は不要。
    /// `Setting::Unknown` は RFC 9113 §6.5.2 に従い無視する。
    pub fn apply(&mut self, setting: Setting) {
        match setting {
            Setting::HeaderTableSize(v) => self.header_table_size = v,
            Setting::EnablePush(b) => self.enable_push = b,
            Setting::MaxConcurrentStreams(v) => self.max_concurrent_streams = Some(v),
            Setting::InitialWindowSize(ws) => self.initial_window_size = ws,
            Setting::MaxFrameSize(mfs) => self.max_frame_size = mfs,
            Setting::MaxHeaderListSize(v) => self.max_header_list_size = Some(v),
            Setting::EnableConnectProtocol(b) => self.enable_connect_protocol = b,
            Setting::NoRfc7540Priorities(b) => self.no_rfc7540_priorities = b,
            Setting::WtEnabled(b) => self.wt_enabled = b,
            Setting::WtInitialMaxData(v) => self.wt_initial_max_data = Some(v),
            Setting::WtInitialMaxStreamDataUni(v) => {
                self.wt_initial_max_stream_data_uni = Some(v);
            }
            Setting::WtInitialMaxStreamDataBidiLocal(v) => {
                self.wt_initial_max_stream_data_bidi_local = Some(v);
            }
            Setting::WtInitialMaxStreamsUni(v) => self.wt_initial_max_streams_uni = Some(v),
            Setting::WtInitialMaxStreamsBidi(v) => self.wt_initial_max_streams_bidi = Some(v),
            Setting::WtInitialMaxStreamDataBidiRemote(v) => {
                self.wt_initial_max_stream_data_bidi_remote = Some(v);
            }
            Setting::Unknown { .. } => {
                // RFC 9113 §6.5.2: 未知の SETTINGS は無視
            }
        }
    }

    /// 設定を `Setting` のリストとして取得する
    #[must_use]
    pub fn to_settings_list(&self) -> Vec<Setting> {
        let mut list = Vec::new();
        list.push(Setting::HeaderTableSize(self.header_table_size));
        list.push(Setting::EnablePush(self.enable_push));
        if let Some(max) = self.max_concurrent_streams {
            list.push(Setting::MaxConcurrentStreams(max));
        }
        list.push(Setting::InitialWindowSize(self.initial_window_size));
        list.push(Setting::MaxFrameSize(self.max_frame_size));
        if let Some(max) = self.max_header_list_size {
            list.push(Setting::MaxHeaderListSize(max));
        }
        if self.enable_connect_protocol {
            list.push(Setting::EnableConnectProtocol(true));
        }
        if self.no_rfc7540_priorities {
            list.push(Setting::NoRfc7540Priorities(true));
        }
        if self.wt_enabled {
            list.push(Setting::WtEnabled(true));
        }
        if let Some(v) = self.wt_initial_max_data {
            list.push(Setting::WtInitialMaxData(v));
        }
        if let Some(v) = self.wt_initial_max_stream_data_uni {
            list.push(Setting::WtInitialMaxStreamDataUni(v));
        }
        if let Some(v) = self.wt_initial_max_stream_data_bidi_local {
            list.push(Setting::WtInitialMaxStreamDataBidiLocal(v));
        }
        if let Some(v) = self.wt_initial_max_streams_uni {
            list.push(Setting::WtInitialMaxStreamsUni(v));
        }
        if let Some(v) = self.wt_initial_max_streams_bidi {
            list.push(Setting::WtInitialMaxStreamsBidi(v));
        }
        if let Some(v) = self.wt_initial_max_stream_data_bidi_remote {
            list.push(Setting::WtInitialMaxStreamDataBidiRemote(v));
        }
        list
    }
}

/// SETTINGS 値範囲検査エラー
///
/// `Setting::from_wire` / `WindowSize::new` / `MaxFrameSize::new` の戻り値で使用される。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingError {
    /// `SETTINGS_ENABLE_PUSH` が 0/1 以外
    ///
    /// RFC 9113 §6.5.2: PROTOCOL_ERROR
    EnablePushNotBoolean {
        /// 違反した値
        value: u32,
    },

    /// `SETTINGS_INITIAL_WINDOW_SIZE` が 2^31-1 を超える
    ///
    /// RFC 9113 §6.5.2: FLOW_CONTROL_ERROR
    InitialWindowSizeOutOfRange {
        /// 違反した値
        value: u32,
        /// 上限
        max: u32,
    },

    /// `SETTINGS_MAX_FRAME_SIZE` が範囲外
    ///
    /// RFC 9113 §6.5.2: 16384..=16777215, 範囲外は PROTOCOL_ERROR
    MaxFrameSizeOutOfRange {
        /// 違反した値
        value: u32,
        /// 下限
        min: u32,
        /// 上限
        max: u32,
    },

    /// `SETTINGS_ENABLE_CONNECT_PROTOCOL` が 0/1 以外
    ///
    /// RFC 8441 §3
    EnableConnectProtocolNotBoolean {
        /// 違反した値
        value: u32,
    },

    /// `SETTINGS_NO_RFC7540_PRIORITIES` が 0/1 以外
    ///
    /// RFC 9218 §2.1
    NoRfc7540PrioritiesNotBoolean {
        /// 違反した値
        value: u32,
    },

    /// `SETTINGS_WT_ENABLED` が 0/1 以外
    ///
    /// draft-ietf-webtrans-http2-15 Section 3.1: クライアントは 1 より大きい値を
    /// 接続エラー PROTOCOL_ERROR として扱わなければならない (MUST)。
    WtEnabledNotBoolean {
        /// 違反した値
        value: u32,
    },
}

impl std::fmt::Display for SettingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EnablePushNotBoolean { value } => {
                write!(f, "SETTINGS_ENABLE_PUSH must be 0 or 1, got {value}")
            }
            Self::InitialWindowSizeOutOfRange { value, max } => write!(
                f,
                "SETTINGS_INITIAL_WINDOW_SIZE {value} exceeds maximum {max}"
            ),
            Self::MaxFrameSizeOutOfRange { value, min, max } => write!(
                f,
                "SETTINGS_MAX_FRAME_SIZE {value} out of range {min}..={max}"
            ),
            Self::EnableConnectProtocolNotBoolean { value } => write!(
                f,
                "SETTINGS_ENABLE_CONNECT_PROTOCOL must be 0 or 1, got {value}"
            ),
            Self::NoRfc7540PrioritiesNotBoolean { value } => write!(
                f,
                "SETTINGS_NO_RFC7540_PRIORITIES must be 0 or 1, got {value}"
            ),
            Self::WtEnabledNotBoolean { value } => {
                write!(f, "SETTINGS_WT_ENABLED must be 0 or 1, got {value}")
            }
        }
    }
}

impl std::error::Error for SettingError {}

/// `SETTINGS_INITIAL_WINDOW_SIZE` / `connection_window_size` 用の制約付き型 (0..=2^31-1)
///
/// RFC 9113 §6.5.2 / §6.9.1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowSize(u32);

impl WindowSize {
    /// 許容される最大値 (2^31 - 1)
    pub const MAX: u32 = MAX_INITIAL_WINDOW_SIZE;

    /// 構築時検査つきで生成する
    ///
    /// # Errors
    ///
    /// - `size > Self::MAX` → [`SettingError::InitialWindowSizeOutOfRange`]
    pub fn new(size: u32) -> Result<Self, SettingError> {
        if size > Self::MAX {
            return Err(SettingError::InitialWindowSizeOutOfRange {
                value: size,
                max: Self::MAX,
            });
        }
        Ok(Self(size))
    }

    /// const 文脈で生成する
    ///
    /// 不正な値ではコンパイル時 panic (= コンパイルエラー) になる。
    ///
    /// # 不正リテラルの compile-fail 例
    ///
    /// 2^31 - 1 を超える値は RFC 9113 §6.5.2 違反:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::WindowSize =
    ///     shiguredo_http2::WindowSize::from_static(u32::MAX);
    /// ```
    pub const fn from_static(size: u32) -> Self {
        assert!(
            size <= Self::MAX,
            "WindowSize::from_static: size must be <= 2^31-1 (RFC 9113 §6.5.2)"
        );
        Self(size)
    }

    /// decoder 内部で検証済みの値から構築する
    ///
    /// 呼び出し側が「0..=2^31-1 の範囲」を保証していること。
    /// 現時点では `Setting::from_wire` が `new` 経由で検査するため未使用だが、
    /// 他の構築時検査型との API 一貫性のために用意する。
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "構築時検査型 API の一貫性のために用意")
    )]
    pub(crate) fn from_validated_parts(size: u32) -> Self {
        debug_assert!(
            size <= Self::MAX,
            "WindowSize::from_validated_parts: size must be <= 2^31-1"
        );
        Self(size)
    }

    /// 値を取得する
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// `SETTINGS_MAX_FRAME_SIZE` 用の制約付き型 (16384..=16777215)
///
/// RFC 9113 §6.5.2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaxFrameSize(u32);

impl MaxFrameSize {
    /// 許容される最小値 (2^14 = 16384)
    pub const MIN: u32 = MIN_MAX_FRAME_SIZE;
    /// 許容される最大値 (2^24 - 1 = 16777215)
    pub const MAX: u32 = MAX_MAX_FRAME_SIZE;

    /// 構築時検査つきで生成する
    ///
    /// # Errors
    ///
    /// - 範囲外 → [`SettingError::MaxFrameSizeOutOfRange`]
    pub fn new(size: u32) -> Result<Self, SettingError> {
        if !(Self::MIN..=Self::MAX).contains(&size) {
            return Err(SettingError::MaxFrameSizeOutOfRange {
                value: size,
                min: Self::MIN,
                max: Self::MAX,
            });
        }
        Ok(Self(size))
    }

    /// const 文脈で生成する
    ///
    /// 不正な値ではコンパイル時 panic (= コンパイルエラー) になる。
    ///
    /// # 不正リテラルの compile-fail 例
    ///
    /// 2^14 (16384) 未満は RFC 9113 §6.5.2 違反:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::MaxFrameSize =
    ///     shiguredo_http2::MaxFrameSize::from_static(100);
    /// ```
    ///
    /// 2^24 - 1 (16777215) 超過も拒否:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::MaxFrameSize =
    ///     shiguredo_http2::MaxFrameSize::from_static(u32::MAX);
    /// ```
    pub const fn from_static(size: u32) -> Self {
        assert!(
            size >= Self::MIN,
            "MaxFrameSize::from_static: size must be >= 16384 (RFC 9113 §6.5.2)"
        );
        assert!(
            size <= Self::MAX,
            "MaxFrameSize::from_static: size must be <= 16777215 (RFC 9113 §6.5.2)"
        );
        Self(size)
    }

    /// decoder 内部で検証済みの値から構築する
    ///
    /// 呼び出し側が「16384..=16777215 の範囲」を保証していること。
    /// 現時点では `Setting::from_wire` が `new` 経由で検査するため未使用だが、
    /// 他の構築時検査型との API 一貫性のために用意する。
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "構築時検査型 API の一貫性のために用意")
    )]
    pub(crate) fn from_validated_parts(size: u32) -> Self {
        debug_assert!(
            (Self::MIN..=Self::MAX).contains(&size),
            "MaxFrameSize::from_validated_parts: size must be in 16384..=16777215"
        );
        Self(size)
    }

    /// 値を取得する
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    mod validated_parts {
        use crate::settings::{MaxFrameSize, WindowSize};

        #[test]
        fn window_size_validated_matches_new() -> noprop::TestResult {
            let seed = noprop::seed_from_env_or_time("HTTP2_PBT_SEED")?;
            let mut runner = noprop::Runner::new(seed);
            runner.run(256, |ctx| {
                let size = noprop::sample_with_boundaries(
                    ctx,
                    &[0u32, WindowSize::MAX],
                    noprop::Ratio::one_nth(5),
                    |ctx| noprop::sample_u64_in(ctx, 0..=WindowSize::MAX as u64) as u32,
                );
                let via_new = WindowSize::new(size).expect("valid SETTINGS value");
                let via_validated = WindowSize::from_validated_parts(size);
                assert_eq!(via_new, via_validated);
                Ok(())
            })?;
            Ok(())
        }

        #[test]
        fn max_frame_size_validated_matches_new() -> noprop::TestResult {
            let seed = noprop::seed_from_env_or_time("HTTP2_PBT_SEED")?;
            let mut runner = noprop::Runner::new(seed);
            runner.run(256, |ctx| {
                let size = noprop::sample_with_boundaries(
                    ctx,
                    &[MaxFrameSize::MIN, MaxFrameSize::MAX],
                    noprop::Ratio::one_nth(5),
                    |ctx| {
                        noprop::sample_u64_in(
                            ctx,
                            MaxFrameSize::MIN as u64..=MaxFrameSize::MAX as u64,
                        ) as u32
                    },
                );
                let via_new = MaxFrameSize::new(size).expect("valid SETTINGS value");
                let via_validated = MaxFrameSize::from_validated_parts(size);
                assert_eq!(via_new, via_validated);
                Ok(())
            })?;
            Ok(())
        }
    }
}
