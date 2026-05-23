//! HPACK 静的テーブル (RFC 7541 Appendix A) と `HeaderField` (issue 0024)
//!
//! `HeaderField` は構築時検査により不正な値を保持しない型として再構築されている。
//! 構築は以下のいずれかで行う:
//!
//! - [`HeaderField::new`][]: ランタイム値から検査つきで構築 (`Result`)
//! - [`HeaderField::new_with_sensitive`][]: 機密フラグ指定で構築 (`Result`)
//! - [`HeaderField::from_static`][]: 静的バイト列から `const fn` で構築
//!   (不正リテラルはコンパイル時に検出される)

use crate::hpack::bytes::{
    HeaderBytes, check_field_name_const, check_field_value_const, check_pseudo_header_const,
};
use crate::hpack::error::HeaderFieldError;

/// HPACK ヘッダーフィールド (RFC 7541 §1.3)
///
/// 構築時に RFC 9113 §8.2.1 / RFC 9110 §5.6.2 / RFC 9113 §8.3 の検査を行う。
/// フィールドは private で、アクセサ ([`Self::name`], [`Self::value`],
/// [`Self::sensitive`]) 経由でのみ読み取れる。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderField {
    name: HeaderBytes,
    value: HeaderBytes,
    sensitive: bool,
}

impl HeaderField {
    /// ランタイム値から検査つきで構築する (sensitive: false)
    ///
    /// `&str` / `&[u8]` / `Vec<u8>` などを受け付ける。内部で `.to_vec()` するため
    /// 引数の所有権は奪わない。
    ///
    /// # Errors
    ///
    /// field-name / field-value / 疑似ヘッダーの構文違反時は
    /// [`HeaderFieldError`] を返す。
    pub fn new(name: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Result<Self, HeaderFieldError> {
        Self::new_with_sensitive(name, value, false)
    }

    /// ランタイム値から検査つきで構築する (sensitive フラグ指定可能)
    ///
    /// `sensitive` が true の場合、HPACK エンコード時に Never-Indexed Literal
    /// として符号化される (RFC 7541 §7.1.3)。
    ///
    /// # Errors
    ///
    /// field-name / field-value / 疑似ヘッダーの構文違反時は
    /// [`HeaderFieldError`] を返す。
    pub fn new_with_sensitive(
        name: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
        sensitive: bool,
    ) -> Result<Self, HeaderFieldError> {
        let name = name.as_ref();
        let value = value.as_ref();
        validate_field_name(name)?;
        validate_field_value(name, value)?;
        validate_pseudo_header(name, value)?;
        Ok(Self {
            name: HeaderBytes::Owned(name.to_vec()),
            value: HeaderBytes::Owned(value.to_vec()),
            sensitive,
        })
    }

    /// 静的バイト列から検査つきで構築する (`const fn`, sensitive: false)
    ///
    /// 不正なリテラル (大文字 field-name、CR/LF を含む値など) を渡すと
    /// const eval が panic し、コンパイルエラーとして検出される。
    /// 検査内容は [`Self::new`] と等価。
    ///
    /// `sensitive: true` が必要な場合は [`Self::new_with_sensitive`] を使う。
    #[must_use]
    pub const fn from_static(name: &'static [u8], value: &'static [u8]) -> Self {
        check_field_name_const(name);
        check_field_value_const(value);
        check_pseudo_header_const(name, value);
        Self {
            name: HeaderBytes::Static(name),
            value: HeaderBytes::Static(value),
            sensitive: false,
        }
    }

    /// 検証済みバイト列から検査をスキップして構築する (crate 内部および test-helper feature 限定)
    ///
    /// HPACK decoder 経路 (`Decoder::decode_*`)、静的テーブル展開、
    /// `concatenate_cookies` のような信頼可能な内部構築箇所でのみ使用する。
    /// `__test_helpers` feature を明示的に有効化した PBT / fuzz クレートのみ
    /// 外部から呼び出せる。本番利用者は有効化してはならない (型不変条件を破壊する)。
    #[cfg(feature = "__test_helpers")]
    #[doc(hidden)]
    pub fn from_validated_parts(name: Vec<u8>, value: Vec<u8>, sensitive: bool) -> Self {
        Self {
            name: HeaderBytes::Owned(name),
            value: HeaderBytes::Owned(value),
            sensitive,
        }
    }

    /// 検証済みバイト列から検査をスキップして構築する (crate 内部限定)
    #[cfg(not(feature = "__test_helpers"))]
    pub(crate) fn from_validated_parts(name: Vec<u8>, value: Vec<u8>, sensitive: bool) -> Self {
        Self {
            name: HeaderBytes::Owned(name),
            value: HeaderBytes::Owned(value),
            sensitive,
        }
    }

    /// field-name への参照を返す
    #[must_use]
    pub fn name(&self) -> &[u8] {
        self.name.as_slice()
    }

    /// field-value への参照を返す
    #[must_use]
    pub fn value(&self) -> &[u8] {
        self.value.as_slice()
    }

    /// `sensitive` (Never-Indexed) フラグを返す
    #[must_use]
    pub fn sensitive(&self) -> bool {
        self.sensitive
    }

    /// HPACK での計算サイズを返す
    ///
    /// RFC 7541 §4.1: size = name.len() + value.len() + 32
    #[must_use]
    pub fn size(&self) -> usize {
        self.name.len() + self.value.len() + 32
    }
}

// === field-name / field-value / 疑似ヘッダー検査 (ランタイム版, issue 0024) ===
//
// `const fn` 版は `crate::hpack::bytes` 側に置く。検査規則は同等を維持する。

pub(crate) fn validate_field_name(name: &[u8]) -> Result<(), HeaderFieldError> {
    if name.is_empty() {
        return Err(HeaderFieldError::EmptyFieldName);
    }
    for (i, &b) in name.iter().enumerate() {
        if b.is_ascii_uppercase() {
            return Err(HeaderFieldError::UppercaseFieldName {
                name: name.to_vec(),
            });
        }
        if b == b':' {
            if i != 0 {
                return Err(HeaderFieldError::InvalidFieldNameByte {
                    name: name.to_vec(),
                    byte: b,
                });
            }
        } else if !is_token_char_lower(b) {
            return Err(HeaderFieldError::InvalidFieldNameByte {
                name: name.to_vec(),
                byte: b,
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_field_value(name: &[u8], value: &[u8]) -> Result<(), HeaderFieldError> {
    if let Some(&first) = value.first()
        && (first == 0x20 || first == 0x09)
    {
        return Err(HeaderFieldError::FieldValueLeadingOrTrailingWhitespace {
            name: name.to_vec(),
        });
    }
    if let Some(&last) = value.last()
        && (last == 0x20 || last == 0x09)
    {
        return Err(HeaderFieldError::FieldValueLeadingOrTrailingWhitespace {
            name: name.to_vec(),
        });
    }
    for &b in value {
        if b == 0x00 || b == 0x0d || b == 0x0a {
            return Err(HeaderFieldError::InvalidFieldValueByte {
                name: name.to_vec(),
                byte: b,
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_pseudo_header(name: &[u8], value: &[u8]) -> Result<(), HeaderFieldError> {
    if name.is_empty() || name[0] != b':' {
        return Ok(());
    }
    match name {
        b":method" => {
            // RFC 9110 §9.1: method = token
            if !is_valid_token_case_insensitive(value) {
                return Err(HeaderFieldError::InvalidPseudoHeaderValue {
                    name: name.to_vec(),
                    value: value.to_vec(),
                });
            }
        }
        b":scheme" => {
            // RFC 3986 §3.1: scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )
            if !is_valid_scheme(value) {
                return Err(HeaderFieldError::InvalidPseudoHeaderValue {
                    name: name.to_vec(),
                    value: value.to_vec(),
                });
            }
        }
        b":path" => {
            // RFC 9113 §8.3.1, RFC 9110 §4.1: absolute-path または "*" (asterisk-form)。
            // 空判定および scheme 依存の検査は validation.rs に残す。
            // 構築時は「もし空でないなら '/' 始まりまたは '*'」の最小チェックのみとする。
            if !value.is_empty() && value != b"*" && !value.starts_with(b"/") {
                return Err(HeaderFieldError::InvalidPseudoHeaderValue {
                    name: name.to_vec(),
                    value: value.to_vec(),
                });
            }
        }
        b":status" => {
            // RFC 9110 §15: 3DIGIT
            if value.len() != 3 || !value.iter().all(u8::is_ascii_digit) {
                return Err(HeaderFieldError::InvalidPseudoHeaderValue {
                    name: name.to_vec(),
                    value: value.to_vec(),
                });
            }
        }
        b":protocol" => {
            // RFC 8441 §4 + RFC 9110 §7.8 / §16.7: HTTP Upgrade Token (token)
            if !is_valid_token_case_insensitive(value) {
                return Err(HeaderFieldError::InvalidPseudoHeaderValue {
                    name: name.to_vec(),
                    value: value.to_vec(),
                });
            }
        }
        b":authority" => {
            // RFC 3986 §3.2: authority。
            // 構築時は field-value 検査 (NUL/CR/LF, SP/HTAB) のみとし、
            // userinfo 拒否 (scheme 依存) と host:port form (CONNECT 限定) は
            // validation.rs に残す。
        }
        _ => {
            return Err(HeaderFieldError::UnknownPseudoHeader {
                name: name.to_vec(),
            });
        }
    }
    Ok(())
}

/// RFC 9110 §5.6.2 の tchar (lowercase 限定) 判定
const fn is_token_char_lower(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
        b'^' | b'_' | b'`' | b'|' | b'~' |
        b'0'..=b'9' |
        b'a'..=b'z'
    )
}

/// RFC 9110 §5.6.2 の tchar (case insensitive) 判定 (:method / :protocol 値用)
const fn is_token_char_case_insensitive(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
        b'^' | b'_' | b'`' | b'|' | b'~' |
        b'0'..=b'9' |
        b'a'..=b'z' |
        b'A'..=b'Z'
    )
}

fn is_valid_token_case_insensitive(value: &[u8]) -> bool {
    !value.is_empty() && value.iter().all(|&b| is_token_char_case_insensitive(b))
}

fn is_valid_scheme(value: &[u8]) -> bool {
    if value.is_empty() {
        return false;
    }
    if !value[0].is_ascii_alphabetic() {
        return false;
    }
    value[1..]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
}

/// 静的テーブルエントリ
#[derive(Debug, Clone, Copy)]
pub struct StaticEntry {
    /// ヘッダー名
    pub name: &'static [u8],
    /// ヘッダー値
    pub value: &'static [u8],
}

impl StaticEntry {
    /// `HeaderField` に変換する
    ///
    /// 静的テーブルは RFC 7541 Appendix A により定義済みで追加検証は不要なため、
    /// `HeaderBytes::Static` を直接組み立ててゼロアロケーションで構築する。
    #[must_use]
    pub const fn to_header_field(&self) -> HeaderField {
        HeaderField {
            name: HeaderBytes::Static(self.name),
            value: HeaderBytes::Static(self.value),
            sensitive: false,
        }
    }
}

/// 静的テーブル (RFC 7541 Appendix A)
///
/// インデックスは 1 から始まる (0 は未使用)。
pub static STATIC_TABLE: [StaticEntry; 62] = [
    // Index 0 (unused)
    StaticEntry {
        name: b"",
        value: b"",
    },
    // Index 1
    StaticEntry {
        name: b":authority",
        value: b"",
    },
    // Index 2
    StaticEntry {
        name: b":method",
        value: b"GET",
    },
    // Index 3
    StaticEntry {
        name: b":method",
        value: b"POST",
    },
    // Index 4
    StaticEntry {
        name: b":path",
        value: b"/",
    },
    // Index 5
    StaticEntry {
        name: b":path",
        value: b"/index.html",
    },
    // Index 6
    StaticEntry {
        name: b":scheme",
        value: b"http",
    },
    // Index 7
    StaticEntry {
        name: b":scheme",
        value: b"https",
    },
    // Index 8
    StaticEntry {
        name: b":status",
        value: b"200",
    },
    // Index 9
    StaticEntry {
        name: b":status",
        value: b"204",
    },
    // Index 10
    StaticEntry {
        name: b":status",
        value: b"206",
    },
    // Index 11
    StaticEntry {
        name: b":status",
        value: b"304",
    },
    // Index 12
    StaticEntry {
        name: b":status",
        value: b"400",
    },
    // Index 13
    StaticEntry {
        name: b":status",
        value: b"404",
    },
    // Index 14
    StaticEntry {
        name: b":status",
        value: b"500",
    },
    // Index 15
    StaticEntry {
        name: b"accept-charset",
        value: b"",
    },
    // Index 16
    StaticEntry {
        name: b"accept-encoding",
        value: b"gzip, deflate",
    },
    // Index 17
    StaticEntry {
        name: b"accept-language",
        value: b"",
    },
    // Index 18
    StaticEntry {
        name: b"accept-ranges",
        value: b"",
    },
    // Index 19
    StaticEntry {
        name: b"accept",
        value: b"",
    },
    // Index 20
    StaticEntry {
        name: b"access-control-allow-origin",
        value: b"",
    },
    // Index 21
    StaticEntry {
        name: b"age",
        value: b"",
    },
    // Index 22
    StaticEntry {
        name: b"allow",
        value: b"",
    },
    // Index 23
    StaticEntry {
        name: b"authorization",
        value: b"",
    },
    // Index 24
    StaticEntry {
        name: b"cache-control",
        value: b"",
    },
    // Index 25
    StaticEntry {
        name: b"content-disposition",
        value: b"",
    },
    // Index 26
    StaticEntry {
        name: b"content-encoding",
        value: b"",
    },
    // Index 27
    StaticEntry {
        name: b"content-language",
        value: b"",
    },
    // Index 28
    StaticEntry {
        name: b"content-length",
        value: b"",
    },
    // Index 29
    StaticEntry {
        name: b"content-location",
        value: b"",
    },
    // Index 30
    StaticEntry {
        name: b"content-range",
        value: b"",
    },
    // Index 31
    StaticEntry {
        name: b"content-type",
        value: b"",
    },
    // Index 32
    StaticEntry {
        name: b"cookie",
        value: b"",
    },
    // Index 33
    StaticEntry {
        name: b"date",
        value: b"",
    },
    // Index 34
    StaticEntry {
        name: b"etag",
        value: b"",
    },
    // Index 35
    StaticEntry {
        name: b"expect",
        value: b"",
    },
    // Index 36
    StaticEntry {
        name: b"expires",
        value: b"",
    },
    // Index 37
    StaticEntry {
        name: b"from",
        value: b"",
    },
    // Index 38
    StaticEntry {
        name: b"host",
        value: b"",
    },
    // Index 39
    StaticEntry {
        name: b"if-match",
        value: b"",
    },
    // Index 40
    StaticEntry {
        name: b"if-modified-since",
        value: b"",
    },
    // Index 41
    StaticEntry {
        name: b"if-none-match",
        value: b"",
    },
    // Index 42
    StaticEntry {
        name: b"if-range",
        value: b"",
    },
    // Index 43
    StaticEntry {
        name: b"if-unmodified-since",
        value: b"",
    },
    // Index 44
    StaticEntry {
        name: b"last-modified",
        value: b"",
    },
    // Index 45
    StaticEntry {
        name: b"link",
        value: b"",
    },
    // Index 46
    StaticEntry {
        name: b"location",
        value: b"",
    },
    // Index 47
    StaticEntry {
        name: b"max-forwards",
        value: b"",
    },
    // Index 48
    StaticEntry {
        name: b"proxy-authenticate",
        value: b"",
    },
    // Index 49
    StaticEntry {
        name: b"proxy-authorization",
        value: b"",
    },
    // Index 50
    StaticEntry {
        name: b"range",
        value: b"",
    },
    // Index 51
    StaticEntry {
        name: b"referer",
        value: b"",
    },
    // Index 52
    StaticEntry {
        name: b"refresh",
        value: b"",
    },
    // Index 53
    StaticEntry {
        name: b"retry-after",
        value: b"",
    },
    // Index 54
    StaticEntry {
        name: b"server",
        value: b"",
    },
    // Index 55
    StaticEntry {
        name: b"set-cookie",
        value: b"",
    },
    // Index 56
    StaticEntry {
        name: b"strict-transport-security",
        value: b"",
    },
    // Index 57
    StaticEntry {
        name: b"transfer-encoding",
        value: b"",
    },
    // Index 58
    StaticEntry {
        name: b"user-agent",
        value: b"",
    },
    // Index 59
    StaticEntry {
        name: b"vary",
        value: b"",
    },
    // Index 60
    StaticEntry {
        name: b"via",
        value: b"",
    },
    // Index 61
    StaticEntry {
        name: b"www-authenticate",
        value: b"",
    },
];

/// 静的テーブルのエントリ数
pub const STATIC_TABLE_SIZE: usize = 61;

/// 静的テーブルからエントリを取得する
///
/// インデックスは 1 から 61 の範囲で有効。
#[must_use]
pub fn get_static_entry(index: usize) -> Option<&'static StaticEntry> {
    if (1..=STATIC_TABLE_SIZE).contains(&index) {
        Some(&STATIC_TABLE[index])
    } else {
        None
    }
}

/// 静的テーブルからヘッダー名でインデックスを検索する
///
/// 完全一致するエントリがある場合は `(index, true)` を返す。
/// 名前のみ一致するエントリがある場合は `(index, false)` を返す。
/// 一致するエントリがない場合は `None` を返す。
#[must_use]
pub fn find_static_index(name: &[u8], value: &[u8]) -> Option<(usize, bool)> {
    let mut name_match = None;

    for (i, entry) in STATIC_TABLE.iter().enumerate().skip(1) {
        if entry.name == name {
            if entry.value == value {
                return Some((i, true));
            }
            if name_match.is_none() {
                name_match = Some(i);
            }
        }
    }

    name_match.map(|i| (i, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_table_count() {
        assert_eq!(STATIC_TABLE.len(), 62);
    }

    #[test]
    fn get_static_entry_basic() {
        let entry = get_static_entry(1).unwrap();
        assert_eq!(entry.name, b":authority");
        assert_eq!(entry.value, b"");

        let entry = get_static_entry(2).unwrap();
        assert_eq!(entry.name, b":method");
        assert_eq!(entry.value, b"GET");

        let entry = get_static_entry(61).unwrap();
        assert_eq!(entry.name, b"www-authenticate");
        assert_eq!(entry.value, b"");

        assert!(get_static_entry(0).is_none());
        assert!(get_static_entry(62).is_none());
    }

    #[test]
    fn find_static_index_basic() {
        let result = find_static_index(b":method", b"GET");
        assert_eq!(result, Some((2, true)));

        let result = find_static_index(b":method", b"PUT");
        assert_eq!(result, Some((2, false)));

        let result = find_static_index(b"x-custom-header", b"value");
        assert_eq!(result, None);
    }

    #[test]
    fn header_field_size() {
        let field = HeaderField::new("content-type", "application/json").unwrap();
        // 12 + 16 + 32 = 60
        assert_eq!(field.size(), 60);
    }

    #[test]
    fn header_field_new_accepts_valid() {
        let h = HeaderField::new(":method", "GET").unwrap();
        assert_eq!(h.name(), b":method");
        assert_eq!(h.value(), b"GET");
        assert!(!h.sensitive());
    }

    #[test]
    fn header_field_new_with_sensitive() {
        let h = HeaderField::new_with_sensitive("authorization", "Bearer secret", true).unwrap();
        assert!(h.sensitive());
        assert_eq!(h.name(), b"authorization");
        assert_eq!(h.value(), b"Bearer secret");
    }

    #[test]
    fn header_field_new_rejects_empty_name() {
        let err = HeaderField::new("", "value").unwrap_err();
        assert!(matches!(err, HeaderFieldError::EmptyFieldName));
    }

    #[test]
    fn header_field_new_rejects_uppercase_name() {
        let err = HeaderField::new("Content-Type", "text/html").unwrap_err();
        assert!(matches!(err, HeaderFieldError::UppercaseFieldName { .. }));
    }

    #[test]
    fn header_field_new_rejects_invalid_name_byte() {
        let err = HeaderField::new("foo bar", "v").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::InvalidFieldNameByte { byte: b' ', .. }
        ));
    }

    #[test]
    fn header_field_new_rejects_colon_in_middle() {
        let err = HeaderField::new("foo:bar", "v").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::InvalidFieldNameByte { byte: b':', .. }
        ));
    }

    #[test]
    fn header_field_new_rejects_crlf_in_value() {
        let err = HeaderField::new(":path", "/\r\nX-Inject: 1").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::InvalidFieldValueByte { byte: 0x0d, .. }
        ));
    }

    #[test]
    fn header_field_new_rejects_nul_in_value() {
        let err = HeaderField::new("x", "abc\0def").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::InvalidFieldValueByte { byte: 0x00, .. }
        ));
    }

    #[test]
    fn header_field_new_rejects_leading_whitespace() {
        let err = HeaderField::new("x", " value").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::FieldValueLeadingOrTrailingWhitespace { .. }
        ));
    }

    #[test]
    fn header_field_new_rejects_trailing_tab() {
        let err = HeaderField::new("x", "value\t").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::FieldValueLeadingOrTrailingWhitespace { .. }
        ));
    }

    #[test]
    fn header_field_new_rejects_unknown_pseudo() {
        let err = HeaderField::new(":foo", "bar").unwrap_err();
        assert!(matches!(err, HeaderFieldError::UnknownPseudoHeader { .. }));
    }

    #[test]
    fn header_field_new_rejects_invalid_status() {
        let err = HeaderField::new(":status", "abc").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::InvalidPseudoHeaderValue { .. }
        ));
    }

    #[test]
    fn header_field_new_accepts_status_200() {
        let h = HeaderField::new(":status", "200").unwrap();
        assert_eq!(h.value(), b"200");
    }

    #[test]
    fn header_field_new_rejects_invalid_method() {
        let err = HeaderField::new(":method", "GE T").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::InvalidPseudoHeaderValue { .. }
        ));
    }

    #[test]
    fn header_field_new_accepts_scheme_https() {
        let h = HeaderField::new(":scheme", "https").unwrap();
        assert_eq!(h.value(), b"https");
    }

    #[test]
    fn header_field_new_rejects_invalid_scheme() {
        let err = HeaderField::new(":scheme", "1http").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::InvalidPseudoHeaderValue { .. }
        ));
    }

    #[test]
    fn header_field_new_accepts_path_absolute() {
        let h = HeaderField::new(":path", "/index.html").unwrap();
        assert_eq!(h.value(), b"/index.html");
    }

    #[test]
    fn header_field_new_accepts_path_asterisk() {
        let h = HeaderField::new(":path", "*").unwrap();
        assert_eq!(h.value(), b"*");
    }

    #[test]
    fn header_field_new_accepts_path_empty() {
        // 空 :path は scheme 依存のため構築時には弾かない (validation.rs 側で判定)
        let h = HeaderField::new(":path", "").unwrap();
        assert_eq!(h.value(), b"");
    }

    #[test]
    fn header_field_new_rejects_path_non_absolute() {
        let err = HeaderField::new(":path", "index.html").unwrap_err();
        assert!(matches!(
            err,
            HeaderFieldError::InvalidPseudoHeaderValue { .. }
        ));
    }

    #[test]
    fn header_field_from_static_pseudo() {
        const M: HeaderField = HeaderField::from_static(b":method", b"GET");
        assert_eq!(M.name(), b":method");
        assert_eq!(M.value(), b"GET");
        assert!(!M.sensitive());
    }

    #[test]
    fn header_field_from_static_regular() {
        const H: HeaderField = HeaderField::from_static(b"content-type", b"text/html");
        assert_eq!(H.name(), b"content-type");
        assert_eq!(H.value(), b"text/html");
    }

    #[test]
    fn header_field_from_validated_parts_skips_check() {
        // crate 内部経路: 既に検証済みのデータを受け取る前提なので
        // 大文字や CRLF を含むデータも構築は通る (検査責任は呼び出し側)
        let h = HeaderField::from_validated_parts(b"X-Test".to_vec(), b"value".to_vec(), false);
        assert_eq!(h.name(), b"X-Test");
        assert_eq!(h.value(), b"value");
    }
}
