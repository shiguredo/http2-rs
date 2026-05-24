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
