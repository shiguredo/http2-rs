//! HPACK 静的テーブル (RFC 7541 Appendix A)
//!
//! HTTP/2 ヘッダー圧縮で使用される静的テーブル（61 エントリ）を提供する。

/// ヘッダーフィールド
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderField {
    /// ヘッダー名
    pub name: Vec<u8>,
    /// ヘッダー値
    pub value: Vec<u8>,
    /// 機密フラグ（Never Indexed を使用するかどうか）
    ///
    /// RFC 7541 Section 7.1.3: Never-Indexed Literal
    /// このフラグが true の場合、中間者がこのヘッダーを動的テーブルに
    /// インデックスすることを禁止する。
    pub sensitive: bool,
}

impl HeaderField {
    /// 新しい `HeaderField` を生成する
    #[must_use]
    pub fn new(name: Vec<u8>, value: Vec<u8>) -> Self {
        Self {
            name,
            value,
            sensitive: false,
        }
    }

    /// 機密フラグ付きで `HeaderField` を生成する
    #[must_use]
    pub fn new_sensitive(name: Vec<u8>, value: Vec<u8>, sensitive: bool) -> Self {
        Self {
            name,
            value,
            sensitive,
        }
    }

    /// 文字列から `HeaderField` を生成する
    #[must_use]
    pub fn from_str(name: &str, value: &str) -> Self {
        Self {
            name: name.as_bytes().to_vec(),
            value: value.as_bytes().to_vec(),
            sensitive: false,
        }
    }

    /// 文字列から機密な `HeaderField` を生成する
    #[must_use]
    pub fn sensitive(name: &str, value: &str) -> Self {
        Self {
            name: name.as_bytes().to_vec(),
            value: value.as_bytes().to_vec(),
            sensitive: true,
        }
    }

    /// HPACK での計算サイズを取得する
    ///
    /// RFC 7541 Section 4.1: size = name.len() + value.len() + 32
    #[must_use]
    pub fn size(&self) -> usize {
        self.name.len() + self.value.len() + 32
    }
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
    #[must_use]
    pub fn to_header_field(&self) -> HeaderField {
        HeaderField {
            name: self.name.to_vec(),
            value: self.value.to_vec(),
            sensitive: false,
        }
    }
}

/// 静的テーブル（RFC 7541 Appendix A）
///
/// インデックスは 1 から始まる（0 は未使用）
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
    fn test_static_table_size() {
        assert_eq!(STATIC_TABLE.len(), 62); // 0 + 61 entries
    }

    #[test]
    fn test_get_static_entry() {
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
    fn test_find_static_index() {
        // 完全一致
        let result = find_static_index(b":method", b"GET");
        assert_eq!(result, Some((2, true)));

        // 名前のみ一致
        let result = find_static_index(b":method", b"PUT");
        assert_eq!(result, Some((2, false)));

        // 一致なし
        let result = find_static_index(b"x-custom-header", b"value");
        assert_eq!(result, None);
    }

    #[test]
    fn test_header_field_size() {
        let field = HeaderField::from_str("content-type", "application/json");
        // 12 + 16 + 32 = 60
        assert_eq!(field.size(), 60);
    }
}
