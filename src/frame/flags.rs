//! HTTP/2 フレームフラグ (RFC 9113 Section 6)

/// フレームフラグを表すビットフラグ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameFlags(u8);

impl FrameFlags {
    /// END_STREAM フラグ (0x01)
    ///
    /// DATA, HEADERS フレームで使用
    pub const END_STREAM: u8 = 0x01;

    /// ACK フラグ (0x01)
    ///
    /// SETTINGS, PING フレームで使用
    pub const ACK: u8 = 0x01;

    /// END_HEADERS フラグ (0x04)
    ///
    /// HEADERS, CONTINUATION フレームで使用
    pub const END_HEADERS: u8 = 0x04;

    /// PADDED フラグ (0x08)
    ///
    /// DATA, HEADERS フレームで使用
    pub const PADDED: u8 = 0x08;

    /// PRIORITY フラグ (0x20)
    ///
    /// # 非推奨 (Deprecated)
    ///
    /// RFC 9113 で非推奨となった。相互運用性のため受信は処理する。
    /// HEADERS フレームで使用。
    pub const PRIORITY: u8 = 0x20;

    /// 空のフラグを生成する
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// 指定した値からフラグを生成する
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// フラグのビット値を取得する
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// 指定したフラグが設定されているか確認する
    #[must_use]
    pub const fn contains(self, flag: u8) -> bool {
        (self.0 & flag) == flag
    }

    /// フラグを設定する
    #[must_use]
    pub const fn set(self, flag: u8) -> Self {
        Self(self.0 | flag)
    }

    /// END_STREAM フラグが設定されているか確認する
    #[must_use]
    pub const fn is_end_stream(self) -> bool {
        self.contains(Self::END_STREAM)
    }

    /// ACK フラグが設定されているか確認する
    #[must_use]
    pub const fn is_ack(self) -> bool {
        self.contains(Self::ACK)
    }

    /// END_HEADERS フラグが設定されているか確認する
    #[must_use]
    pub const fn is_end_headers(self) -> bool {
        self.contains(Self::END_HEADERS)
    }

    /// PADDED フラグが設定されているか確認する
    #[must_use]
    pub const fn is_padded(self) -> bool {
        self.contains(Self::PADDED)
    }

    /// PRIORITY フラグが設定されているか確認する
    ///
    /// # 非推奨 (Deprecated)
    ///
    /// RFC 9113 で非推奨となった。相互運用性のため受信は処理する。
    #[must_use]
    pub const fn is_priority(self) -> bool {
        self.contains(Self::PRIORITY)
    }
}

impl From<u8> for FrameFlags {
    fn from(bits: u8) -> Self {
        Self::from_bits(bits)
    }
}

impl From<FrameFlags> for u8 {
    fn from(flags: FrameFlags) -> Self {
        flags.bits()
    }
}
