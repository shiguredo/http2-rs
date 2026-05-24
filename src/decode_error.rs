//! フレーム decoder / encoder で共通利用するデコード時エラー型。
//!
//! フレームレベルの接続エラーへの昇格は
//! [`From<DecodeError> for crate::error::Error`] で行う。

/// HTTP/2 フレームデコード / エンコード時のバッファ操作エラー
///
/// フレーム decoder / encoder の内部で発生するバッファ不足や入力不足を表現する。
/// フレームレベルの接続エラーへの昇格は [`From<DecodeError> for crate::error::Error`]
/// で行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DecodeError {
    /// バッファが必要なサイズに満たない
    ///
    /// RFC 9113 では特定のエラーコードに紐づかないが、フレームヘッダー読み出しや
    /// ペイロード切り出し時に発生する。
    BufferTooShort {
        /// 必要なサイズ
        required: usize,
        /// 実際のバッファサイズ
        available: usize,
    },

    /// 入力データが不足している (ストリーミングデコード時)
    ///
    /// 追加の入力を待つ必要がある状態を示す。
    Incomplete,
}

impl DecodeError {
    /// バッファサイズを検査し、不足していれば [`DecodeError::BufferTooShort`] を返す
    pub const fn check_buffer_size(required: usize, buf: &[u8]) -> Result<(), Self> {
        if buf.len() < required {
            Err(Self::BufferTooShort {
                required,
                available: buf.len(),
            })
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferTooShort {
                required,
                available,
            } => write!(
                f,
                "buffer too short: required {required} bytes, available {available} bytes"
            ),
            Self::Incomplete => write!(f, "incomplete input"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_buffer_size_ok() {
        let buf = [0u8; 16];
        assert_eq!(DecodeError::check_buffer_size(8, &buf), Ok(()));
        assert_eq!(DecodeError::check_buffer_size(16, &buf), Ok(()));
    }

    #[test]
    fn check_buffer_size_err() {
        let buf = [0u8; 8];
        assert_eq!(
            DecodeError::check_buffer_size(16, &buf),
            Err(DecodeError::BufferTooShort {
                required: 16,
                available: 8,
            })
        );
    }

    #[test]
    fn display_buffer_too_short() {
        let err = DecodeError::BufferTooShort {
            required: 9,
            available: 4,
        };
        assert_eq!(
            err.to_string(),
            "buffer too short: required 9 bytes, available 4 bytes"
        );
    }

    #[test]
    fn display_incomplete() {
        assert_eq!(DecodeError::Incomplete.to_string(), "incomplete input");
    }
}
