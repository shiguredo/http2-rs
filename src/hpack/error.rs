//! HPACK 構築時検査エラー (issue 0024 / 0029)
//!
//! [`crate::hpack::HeaderField`] の構築時検査で使用される構造化エラー型。
//! 文字列ベースの [`crate::error::Error`] とは分離し、違反値を構造化フィールドで保持する。

/// HPACK ヘッダーフィールド構築時検査エラー
///
/// RFC 9113 §8.2 / §8.3 および RFC 9110 §5 で定義される field-name / field-value /
/// 疑似ヘッダーの構文制約を、構築点で検出した結果を表現する。
///
/// `Vec<u8>` フィールドを持つため [`Copy`] は導出不可能。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HeaderFieldError {
    /// field-name が空
    ///
    /// RFC 9110 §5.1, §5.6.2: `token = 1*tchar`
    EmptyFieldName,

    /// field-name に lowercase 以外の ASCII 英字が含まれる
    ///
    /// RFC 9113 §8.2.1: MUST NOT contain 0x41-0x5a (uppercase ASCII)
    UppercaseFieldName {
        /// 違反した field-name
        name: Vec<u8>,
    },

    /// field-name に token 文字以外が含まれる
    ///
    /// RFC 9110 §5.1, §5.6.2 / RFC 9113 §8.2.1
    InvalidFieldNameByte {
        /// 違反した field-name
        name: Vec<u8>,
        /// 違反したバイト値
        byte: u8,
    },

    /// field-value に NUL/CR/LF が含まれる
    ///
    /// RFC 9113 §8.2.1: MUST NOT contain 0x00, 0x0a, 0x0d
    InvalidFieldValueByte {
        /// 違反した field-name (デバッグ補助)
        name: Vec<u8>,
        /// 違反したバイト値
        byte: u8,
    },

    /// field-value が先頭または末尾に SP/HTAB を含む
    ///
    /// RFC 9113 §8.2.1: MUST NOT start or end with 0x20 (SP) or 0x09 (HTAB)
    FieldValueLeadingOrTrailingWhitespace {
        /// 違反した field-name (デバッグ補助)
        name: Vec<u8>,
    },

    /// 疑似ヘッダー名が未定義 (`:foo` のような不明な疑似ヘッダー)
    ///
    /// RFC 9113 §8.3 / RFC 8441 §4: 定義済み疑似ヘッダーのみ許可
    UnknownPseudoHeader {
        /// 違反した field-name
        name: Vec<u8>,
    },

    /// 疑似ヘッダー値が構文違反
    ///
    /// 各疑似ヘッダーの構文根拠:
    /// - `:method`: RFC 9110 §9.1 (token)
    /// - `:scheme`: RFC 3986 §3.1 (scheme)
    /// - `:path`: RFC 9113 §8.3.1, RFC 9110 §4.1 (absolute-path)
    /// - `:status`: RFC 9112 §4, RFC 9110 §15 (3DIGIT)
    /// - `:protocol`: RFC 8441 §4 (HTTP Upgrade Token)
    /// - `:authority`: RFC 3986 §3.2 (authority)
    InvalidPseudoHeaderValue {
        /// 疑似ヘッダー名
        name: Vec<u8>,
        /// 違反した値
        value: Vec<u8>,
    },
}

impl std::fmt::Display for HeaderFieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyFieldName => write!(f, "field name must not be empty"),
            Self::UppercaseFieldName { name } => write!(
                f,
                "field name must be lowercase: {}",
                String::from_utf8_lossy(name)
            ),
            Self::InvalidFieldNameByte { name, byte } => write!(
                f,
                "field name contains invalid byte 0x{byte:02x}: {}",
                String::from_utf8_lossy(name)
            ),
            Self::InvalidFieldValueByte { name, byte } => write!(
                f,
                "field value of {} contains forbidden byte 0x{byte:02x}",
                String::from_utf8_lossy(name)
            ),
            Self::FieldValueLeadingOrTrailingWhitespace { name } => write!(
                f,
                "field value of {} must not start or end with SP/HTAB",
                String::from_utf8_lossy(name)
            ),
            Self::UnknownPseudoHeader { name } => write!(
                f,
                "unknown pseudo-header: {}",
                String::from_utf8_lossy(name)
            ),
            Self::InvalidPseudoHeaderValue { name, value } => write!(
                f,
                "invalid value for pseudo-header {}: {}",
                String::from_utf8_lossy(name),
                String::from_utf8_lossy(value)
            ),
        }
    }
}

impl std::error::Error for HeaderFieldError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_empty_field_name() {
        assert_eq!(
            HeaderFieldError::EmptyFieldName.to_string(),
            "field name must not be empty"
        );
    }

    #[test]
    fn display_uppercase_field_name() {
        let err = HeaderFieldError::UppercaseFieldName {
            name: b"Host".to_vec(),
        };
        assert_eq!(err.to_string(), "field name must be lowercase: Host");
    }

    #[test]
    fn display_invalid_field_name_byte() {
        let err = HeaderFieldError::InvalidFieldNameByte {
            name: b"x foo".to_vec(),
            byte: b' ',
        };
        assert_eq!(
            err.to_string(),
            "field name contains invalid byte 0x20: x foo"
        );
    }

    #[test]
    fn display_invalid_field_value_byte() {
        let err = HeaderFieldError::InvalidFieldValueByte {
            name: b":path".to_vec(),
            byte: 0x0d,
        };
        assert_eq!(
            err.to_string(),
            "field value of :path contains forbidden byte 0x0d"
        );
    }

    #[test]
    fn display_field_value_whitespace() {
        let err = HeaderFieldError::FieldValueLeadingOrTrailingWhitespace {
            name: b"content-type".to_vec(),
        };
        assert_eq!(
            err.to_string(),
            "field value of content-type must not start or end with SP/HTAB"
        );
    }

    #[test]
    fn display_unknown_pseudo_header() {
        let err = HeaderFieldError::UnknownPseudoHeader {
            name: b":foo".to_vec(),
        };
        assert_eq!(err.to_string(), "unknown pseudo-header: :foo");
    }

    #[test]
    fn display_invalid_pseudo_header_value() {
        let err = HeaderFieldError::InvalidPseudoHeaderValue {
            name: b":status".to_vec(),
            value: b"abc".to_vec(),
        };
        assert_eq!(
            err.to_string(),
            "invalid value for pseudo-header :status: abc"
        );
    }
}
