//! フレーム構築時検査エラー (issue 0027 / 0029)
//!
//! 各 HTTP/2 フレーム型の構築 API で発生する検査エラーを表現する。
//! 文字列ベースの [`crate::error::Error`] とは分離し、違反値を構造化フィールドで保持する。
//!
//! 本ファイルは issue 0027 構築時検査リファクタリングの Phase 1 として追加された。
//! Phase 2 で各 [`crate::frame::DataFrame`] 等の構築 API と統合される予定。

use crate::frame::FrameType;

/// HTTP/2 フレーム構築時検査エラー
///
/// RFC 9113 §6 各フレーム定義における stream_id / payload / window-increment 等の
/// 制約を構築点で検出した結果を表現する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FrameError {
    /// stream_id = 0 を許可しないフレーム種別で 0 が指定された
    ///
    /// RFC 9113 §6.1 DATA / §6.2 HEADERS / §6.3 PRIORITY / §6.4 RST_STREAM /
    /// §6.6 PUSH_PROMISE / §6.10 CONTINUATION は stream_id 0 を許可しない。
    ZeroStreamIdNotAllowed {
        /// 違反したフレーム種別
        frame_type: FrameType,
    },

    /// stream_id != 0 を許可しないフレーム種別で非 0 が指定された
    ///
    /// RFC 9113 §6.5 SETTINGS / §6.7 PING / §6.8 GOAWAY、および
    /// RFC 9218 §7.1 PRIORITY_UPDATE はフレームヘッダの stream_id 0 でなければならない。
    NonZeroStreamIdNotAllowed {
        /// 違反したフレーム種別
        frame_type: FrameType,
        /// 違反した stream_id
        stream_id: u32,
    },

    /// WINDOW_UPDATE の increment が 0
    ///
    /// RFC 9113 §6.9: window-increment は 1..=2^31-1
    ZeroWindowIncrement,

    /// WINDOW_UPDATE の increment が 2^31-1 を超える
    ///
    /// RFC 9113 §6.9 / §6.9.1: 最大値 2^31-1
    WindowIncrementOutOfRange {
        /// 違反した値
        value: u32,
    },

    /// PRIORITY の weight が範囲外
    ///
    /// RFC 9113 §6.3 はフィールド型 (unsigned 8-bit integer) を定義する。
    /// 「wire 0..=255 / 実体 1..=256」の意味付けは RFC 7540 §5.3.2 由来
    /// (RFC 9113 §5.3.1 で deprecated だが受信処理用に維持)。
    InvalidWeight {
        /// 違反した weight (wire 表現で 0..=255 を超えた値)
        value: u16,
    },

    /// padding 長が payload を超える
    ///
    /// RFC 9113 §6.1 DATA / §6.2 HEADERS PADDED フラグ
    PaddingExceedsPayload {
        /// padding 長
        padding: u8,
        /// payload 長
        payload_len: usize,
    },

    /// GOAWAY の last_stream_id が 2^31-1 を超える
    ///
    /// RFC 9113 §6.8: last_stream_id は 31-bit
    LastStreamIdOutOfRange {
        /// 違反した値
        value: u32,
    },
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroStreamIdNotAllowed { frame_type } => {
                write!(f, "{frame_type} frame must not use stream ID 0")
            }
            Self::NonZeroStreamIdNotAllowed {
                frame_type,
                stream_id,
            } => write!(
                f,
                "{frame_type} frame requires stream ID 0 but got {stream_id}"
            ),
            Self::ZeroWindowIncrement => write!(f, "WINDOW_UPDATE increment must not be 0"),
            Self::WindowIncrementOutOfRange { value } => write!(
                f,
                "WINDOW_UPDATE increment {value} exceeds maximum 2147483647"
            ),
            Self::InvalidWeight { value } => {
                write!(f, "PRIORITY weight {value} out of range 0..=255")
            }
            Self::PaddingExceedsPayload {
                padding,
                payload_len,
            } => write!(
                f,
                "padding length {padding} exceeds payload length {payload_len}"
            ),
            Self::LastStreamIdOutOfRange { value } => {
                write!(
                    f,
                    "GOAWAY last_stream_id {value} exceeds maximum 2147483647"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// WINDOW_UPDATE 増分 (1..=2^31-1)
///
/// RFC 9113 §6.9 で定義される値範囲を型レベルで強制する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowIncrement(core::num::NonZeroU32);

impl WindowIncrement {
    /// 許容される最大値 (2^31 - 1)
    pub const MAX: u32 = (1u32 << 31) - 1;

    /// 構築時検査つきで生成する
    ///
    /// # Errors
    ///
    /// - `increment == 0` → [`FrameError::ZeroWindowIncrement`]
    /// - `increment > Self::MAX` → [`FrameError::WindowIncrementOutOfRange`]
    pub fn new(increment: u32) -> Result<Self, FrameError> {
        if increment == 0 {
            return Err(FrameError::ZeroWindowIncrement);
        }
        if increment > Self::MAX {
            return Err(FrameError::WindowIncrementOutOfRange { value: increment });
        }
        // SAFETY: 直前で 0 を弾いている
        Ok(Self(
            core::num::NonZeroU32::new(increment).expect("non-zero checked above"),
        ))
    }

    /// const 文脈で生成する
    ///
    /// 不正な値ではコンパイル時 panic (= コンパイルエラー) になる。
    ///
    /// # 不正リテラルの compile-fail 例
    ///
    /// 0 は WINDOW_UPDATE に対する PROTOCOL_ERROR (RFC 9113 §6.9):
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::WindowIncrement =
    ///     shiguredo_http2::WindowIncrement::from_static(0);
    /// ```
    ///
    /// 2^31 - 1 を超える値も拒否:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::WindowIncrement =
    ///     shiguredo_http2::WindowIncrement::from_static(u32::MAX);
    /// ```
    pub const fn from_static(increment: u32) -> Self {
        assert!(
            increment <= Self::MAX,
            "WindowIncrement::from_static: increment must be <= 2^31-1 (RFC 9113 §6.9)"
        );
        // NonZeroU32::new(0) で None になるため 0 もこの match で弾く
        match core::num::NonZeroU32::new(increment) {
            Some(v) => Self(v),
            None => panic!("WindowIncrement::from_static: increment must not be 0 (RFC 9113 §6.9)"),
        }
    }

    /// decoder 内部で検証済みの値から構築する
    ///
    /// 呼び出し側が「非ゼロかつ 31-bit 範囲」を保証していること。
    pub(crate) fn from_validated_parts(increment: core::num::NonZeroU32) -> Self {
        debug_assert!(
            increment.get() <= Self::MAX,
            "WindowIncrement::from_validated_parts: increment must be <= 2^31-1"
        );
        Self(increment)
    }

    /// 値を取得する
    pub const fn get(self) -> core::num::NonZeroU32 {
        self.0
    }

    /// u32 として取得する
    pub const fn as_u32(self) -> u32 {
        self.0.get()
    }
}

/// PRIORITY weight (wire 上 0..=255、実体 1..=256)
///
/// RFC 9113 §6.3 (RFC 9113 で deprecated だが受信処理用に維持) で定義される値範囲。
/// 内部表現は wire の u8 値 (0..=255)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Weight(u8);

impl Weight {
    /// 構築時検査つきで生成する
    ///
    /// 引数は wire 表現 (0..=255)。実体の優先度値は `as_u16() + 1` で 1..=256 になる。
    ///
    /// # Errors
    ///
    /// - `wire_value > 255` → [`FrameError::InvalidWeight`]
    pub fn new(wire_value: u16) -> Result<Self, FrameError> {
        if wire_value > 255 {
            return Err(FrameError::InvalidWeight { value: wire_value });
        }
        Ok(Self(wire_value as u8))
    }

    /// const 文脈で生成する
    ///
    /// # 不正リテラルの compile-fail 例
    ///
    /// wire 値の上限 255 (実体 256) を超える値は拒否:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::Weight =
    ///     shiguredo_http2::Weight::from_static(256);
    /// ```
    pub const fn from_static(wire_value: u16) -> Self {
        assert!(
            wire_value <= 255,
            "Weight::from_static: wire value must be 0..=255 (RFC 9113 §6.3)"
        );
        Self(wire_value as u8)
    }

    /// decoder 内部で検証済みの値から構築する
    ///
    /// 呼び出し側が「0..=255 の範囲」を保証していること。
    pub(crate) fn from_validated_parts(wire_value: u8) -> Self {
        Self(wire_value)
    }

    /// wire 表現 (0..=255) を取得する
    pub const fn as_wire(self) -> u8 {
        self.0
    }

    /// 実体の優先度値 (1..=256) を取得する
    pub const fn weight_value(self) -> u16 {
        self.0 as u16 + 1
    }
}

/// GOAWAY last_stream_id (0..=2^31-1)
///
/// RFC 9113 §6.8 で定義される 31-bit 値範囲。0 も合法 (どのストリームも処理していない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LastStreamId(u32);

impl LastStreamId {
    /// 許容される最大値 (2^31 - 1)
    pub const MAX: u32 = (1u32 << 31) - 1;

    /// 構築時検査つきで生成する
    ///
    /// # Errors
    ///
    /// - `id > Self::MAX` → [`FrameError::LastStreamIdOutOfRange`]
    pub fn new(id: u32) -> Result<Self, FrameError> {
        if id > Self::MAX {
            return Err(FrameError::LastStreamIdOutOfRange { value: id });
        }
        Ok(Self(id))
    }

    /// const 文脈で生成する
    ///
    /// # 不正リテラルの compile-fail 例
    ///
    /// 2^31 - 1 を超える値は RFC 9113 §6.8 違反:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::LastStreamId =
    ///     shiguredo_http2::LastStreamId::from_static(u32::MAX);
    /// ```
    pub const fn from_static(id: u32) -> Self {
        assert!(
            id <= Self::MAX,
            "LastStreamId::from_static: id must be <= 2^31-1 (RFC 9113 §6.8)"
        );
        Self(id)
    }

    /// decoder 内部で検証済みの値から構築する
    ///
    /// 呼び出し側が「0..=2^31-1 の範囲」を保証していること。
    pub(crate) fn from_validated_parts(id: u32) -> Self {
        debug_assert!(
            id <= Self::MAX,
            "LastStreamId::from_validated_parts: id must be <= 2^31-1"
        );
        Self(id)
    }

    /// 値を取得する
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_increment_new_ok() {
        let w = WindowIncrement::new(1).unwrap();
        assert_eq!(w.as_u32(), 1);
        let w = WindowIncrement::new(WindowIncrement::MAX).unwrap();
        assert_eq!(w.as_u32(), WindowIncrement::MAX);
    }

    #[test]
    fn window_increment_new_zero() {
        assert_eq!(
            WindowIncrement::new(0),
            Err(FrameError::ZeroWindowIncrement)
        );
    }

    #[test]
    fn window_increment_new_overflow() {
        assert_eq!(
            WindowIncrement::new(WindowIncrement::MAX + 1),
            Err(FrameError::WindowIncrementOutOfRange {
                value: WindowIncrement::MAX + 1
            })
        );
    }

    #[test]
    fn window_increment_from_static_ok() {
        const W: WindowIncrement = WindowIncrement::from_static(1024);
        assert_eq!(W.as_u32(), 1024);
    }

    #[test]
    #[should_panic(expected = "increment must not be 0")]
    fn window_increment_from_static_zero_panics() {
        let _ = WindowIncrement::from_static(0);
    }

    #[test]
    #[should_panic(expected = "must be <= 2^31-1")]
    fn window_increment_from_static_overflow_panics() {
        let _ = WindowIncrement::from_static(WindowIncrement::MAX + 1);
    }

    #[test]
    fn weight_new_ok() {
        let w = Weight::new(0).unwrap();
        assert_eq!(w.as_wire(), 0);
        assert_eq!(w.weight_value(), 1);

        let w = Weight::new(255).unwrap();
        assert_eq!(w.as_wire(), 255);
        assert_eq!(w.weight_value(), 256);
    }

    #[test]
    fn weight_new_out_of_range() {
        assert_eq!(
            Weight::new(256),
            Err(FrameError::InvalidWeight { value: 256 })
        );
    }

    #[test]
    fn weight_from_static_ok() {
        const W: Weight = Weight::from_static(15);
        assert_eq!(W.as_wire(), 15);
        assert_eq!(W.weight_value(), 16);
    }

    #[test]
    fn last_stream_id_new_ok() {
        let id = LastStreamId::new(0).unwrap();
        assert_eq!(id.get(), 0);
        let id = LastStreamId::new(LastStreamId::MAX).unwrap();
        assert_eq!(id.get(), LastStreamId::MAX);
    }

    #[test]
    fn last_stream_id_new_out_of_range() {
        assert_eq!(
            LastStreamId::new(LastStreamId::MAX + 1),
            Err(FrameError::LastStreamIdOutOfRange {
                value: LastStreamId::MAX + 1
            })
        );
    }

    #[test]
    fn last_stream_id_from_static_ok() {
        const ID: LastStreamId = LastStreamId::from_static(42);
        assert_eq!(ID.get(), 42);
    }

    #[test]
    fn frame_error_display_zero_stream_id() {
        let err = FrameError::ZeroStreamIdNotAllowed {
            frame_type: FrameType::Data,
        };
        assert_eq!(err.to_string(), "DATA frame must not use stream ID 0");
    }

    #[test]
    fn frame_error_display_non_zero_stream_id() {
        let err = FrameError::NonZeroStreamIdNotAllowed {
            frame_type: FrameType::Settings,
            stream_id: 5,
        };
        assert_eq!(
            err.to_string(),
            "SETTINGS frame requires stream ID 0 but got 5"
        );
    }

    #[test]
    fn frame_error_display_zero_window_increment() {
        assert_eq!(
            FrameError::ZeroWindowIncrement.to_string(),
            "WINDOW_UPDATE increment must not be 0"
        );
    }

    #[test]
    fn frame_error_display_window_overflow() {
        let err = FrameError::WindowIncrementOutOfRange { value: u32::MAX };
        assert!(err.to_string().contains("exceeds maximum"));
    }

    #[test]
    fn frame_error_display_invalid_weight() {
        let err = FrameError::InvalidWeight { value: 1024 };
        assert_eq!(err.to_string(), "PRIORITY weight 1024 out of range 0..=255");
    }

    #[test]
    fn frame_error_display_padding_exceeds_payload() {
        let err = FrameError::PaddingExceedsPayload {
            padding: 100,
            payload_len: 50,
        };
        assert_eq!(
            err.to_string(),
            "padding length 100 exceeds payload length 50"
        );
    }

    #[test]
    fn frame_error_display_last_stream_id_out_of_range() {
        let err = FrameError::LastStreamIdOutOfRange { value: u32::MAX };
        assert!(err.to_string().contains("exceeds maximum"));
    }

    mod validated_parts {
        use proptest::prelude::*;

        use super::{LastStreamId, Weight, WindowIncrement};

        proptest! {
            #[test]
            fn window_increment_validated_matches_new(
                v in 1u32..=WindowIncrement::MAX,
            ) {
                let via_new = WindowIncrement::new(v).unwrap();
                let nz = core::num::NonZeroU32::new(v).unwrap();
                let via_validated = WindowIncrement::from_validated_parts(nz);
                prop_assert_eq!(via_new, via_validated);
            }

            #[test]
            fn weight_validated_matches_new(
                w in 0u16..=255,
            ) {
                let via_new = Weight::new(w).unwrap();
                let via_validated = Weight::from_validated_parts(w as u8);
                prop_assert_eq!(via_new, via_validated);
            }

            #[test]
            fn last_stream_id_validated_matches_new(
                id in 0u32..=LastStreamId::MAX,
            ) {
                let via_new = LastStreamId::new(id).unwrap();
                let via_validated = LastStreamId::from_validated_parts(id);
                prop_assert_eq!(via_new, via_validated);
            }
        }
    }
}
