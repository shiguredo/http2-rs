//! `HeaderField` の内部表現用バイト列型
//!
//! `enum HeaderBytes { Static(&'static [u8]), Owned(Vec<u8>) }` を提供する。
//! 静的バイト列を `const fn` の [`crate::hpack::HeaderField::from_static`]
//! で構築できるようにするために導入する。
//!
//! 本モジュールは crate 内部実装の詳細であり、外部へは公開しない。

/// HPACK ヘッダーの name/value 用バイト列表現
///
/// - `Static(&'static [u8])`: リテラル等の静的バイト列。
///   `HeaderField::from_static` から構築される。
/// - `Owned(Vec<u8>)`: 所有バイト列。ランタイム値や HPACK decoder 経路から構築される。
///
/// 等価性とハッシュは `as_slice()` のバイト列としての一致のみを見るため、
/// `Static(b"GET")` と `Owned(b"GET".to_vec())` は同値と扱う。
#[derive(Debug, Clone)]
pub(crate) enum HeaderBytes {
    /// 'static バイト列
    Static(&'static [u8]),
    /// 所有バイト列
    Owned(Vec<u8>),
}

impl HeaderBytes {
    /// バイトスライスへの参照を取得する
    pub(crate) fn as_slice(&self) -> &[u8] {
        match self {
            Self::Static(s) => s,
            Self::Owned(v) => v.as_slice(),
        }
    }

    /// 長さを取得する
    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }
}

impl PartialEq for HeaderBytes {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for HeaderBytes {}

impl std::hash::Hash for HeaderBytes {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

// === const fn 検査ヘルパ (`HeaderField::from_static` 用) ===
//
// `HeaderField::from_static` から呼ばれ、不正なリテラルを与えると
// const eval が panic することによりコンパイルエラーとなる。
//
// const fn の制約 (パターンマッチや for ループ等が使えない) のため、
// 非 const fn 版の `HeaderField::new` 系検査と実装を共用できない。
// 検査内容を等価に保つために、本ファイルの定義を単一の参照仕様として扱うこと。

/// `from_static` 用の field-name 検査 (RFC 9113 §8.2.1, RFC 9110 §5.6.2)
///
/// 検査内容:
/// - 空でないこと
/// - 大文字 ASCII (0x41-0x5a) を含まないこと
/// - ':' は先頭文字でのみ許可される (疑似ヘッダー)
/// - それ以外の文字は token (lowercase) 規則に従うこと
///
/// 違反時は `panic!` で const eval を停止し、利用者から見れば
/// コンパイルエラーとして検出される。
#[allow(clippy::missing_panics_doc)]
pub(crate) const fn check_field_name_const(name: &[u8]) {
    if name.is_empty() {
        panic!("HeaderField::from_static: field-name must not be empty (RFC 9110 5.6.2)");
    }
    let mut i = 0;
    while i < name.len() {
        let b = name[i];
        if b.is_ascii_uppercase() {
            panic!("HeaderField::from_static: field-name must be lowercase ASCII (RFC 9113 8.2.1)");
        }
        if b == b':' {
            if i != 0 {
                panic!(
                    "HeaderField::from_static: ':' is only allowed at the start of a pseudo-header name (RFC 9113 8.3)"
                );
            }
        } else if !is_token_char_lower_const(b) {
            panic!("HeaderField::from_static: field-name contains non-token byte (RFC 9110 5.6.2)");
        }
        i += 1;
    }
}

/// `from_static` 用の field-value 検査 (RFC 9113 §8.2.1)
///
/// 検査内容:
/// - 先頭/末尾に SP (0x20) または HTAB (0x09) を含まないこと
/// - NUL (0x00) / CR (0x0d) / LF (0x0a) を含まないこと
#[allow(clippy::missing_panics_doc)]
pub(crate) const fn check_field_value_const(value: &[u8]) {
    let len = value.len();
    if len > 0 {
        let first = value[0];
        if first == 0x20 || first == 0x09 {
            panic!(
                "HeaderField::from_static: field-value must not start with SP or HTAB (RFC 9113 8.2.1)"
            );
        }
        let last = value[len - 1];
        if last == 0x20 || last == 0x09 {
            panic!(
                "HeaderField::from_static: field-value must not end with SP or HTAB (RFC 9113 8.2.1)"
            );
        }
    }
    let mut i = 0;
    while i < len {
        let b = value[i];
        if b == 0x00 || b == 0x0d || b == 0x0a {
            panic!(
                "HeaderField::from_static: field-value must not contain NUL, CR, or LF (RFC 9113 8.2.1)"
            );
        }
        i += 1;
    }
}

/// RFC 9110 §5.6.2 の tchar (lowercase 限定) 判定 (const fn 版)
const fn is_token_char_lower_const(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
        b'^' | b'_' | b'`' | b'|' | b'~' |
        b'0'..=b'9' |
        b'a'..=b'z'
    )
}

/// 疑似ヘッダー (`:` 始まり) の名前と値が定義済みの集合に含まれるかを検証する (const fn 版)
///
/// RFC 9113 §8.3.1, §8.3.2 / RFC 8441 §4 で定義された疑似ヘッダー名のみ許可:
/// `:method`, `:scheme`, `:authority`, `:path`, `:status`, `:protocol`
///
/// 値構文の検査:
/// - `:status`: 3 桁 ASCII 数字
/// - `:method`: 非空の token (RFC 9110 §9.1)
/// - `:scheme`: 非空、ALPHA で始まり ALPHA / DIGIT / `+` / `-` / `.` のみ (RFC 3986 §3.1)
/// - `:path`: 空、`*` または `/` 始まり (RFC 9113 §8.3.1)。
///   空の禁止は http/https スキーム依存のためランタイム検査 (`validation.rs`) に委ねる。
/// - `:protocol`: 非空の token (RFC 8441 §4, RFC 9110 §7.8 で token 構文を参照)
/// - `:authority`: 値構文検査はランタイムにも実装せず `validation.rs` の文脈依存検査に委ねる
///
/// `:` 始まりでない場合は何もしない。
#[allow(clippy::missing_panics_doc)]
pub(crate) const fn check_pseudo_header_const(name: &[u8], value: &[u8]) {
    if name.is_empty() || name[0] != b':' {
        return;
    }
    if bytes_eq(name, b":method") {
        // RFC 9110 §9.1: method = token
        check_token_nonempty_const(value);
        return;
    }
    if bytes_eq(name, b":scheme") {
        check_scheme_const(value);
        return;
    }
    if bytes_eq(name, b":authority") {
        // host[:port] の構文は const fn で扱うのが煩雑なため
        // ランタイム検査 (HeaderField::new) に委ねる
        return;
    }
    if bytes_eq(name, b":path") {
        check_path_const(value);
        return;
    }
    if bytes_eq(name, b":protocol") {
        // RFC 8441 §4 + RFC 9110 §7.8 / §16.7: protocol-name = token
        check_token_nonempty_const(value);
        return;
    }
    if bytes_eq(name, b":status") {
        if value.len() != 3 {
            panic!("HeaderField::from_static: :status value must be 3 ASCII digits (RFC 9110 15)");
        }
        let mut i = 0;
        while i < 3 {
            let b = value[i];
            if !b.is_ascii_digit() {
                panic!(
                    "HeaderField::from_static: :status value must be 3 ASCII digits (RFC 9110 15)"
                );
            }
            i += 1;
        }
        return;
    }
    panic!("HeaderField::from_static: unknown pseudo-header name (RFC 9113 8.3, RFC 8441 4)");
}

/// 値が空でなく、全文字が tchar (lowercase 限定でなく、大文字 ALPHA も許容) であることを検査
const fn check_token_nonempty_const(value: &[u8]) {
    if value.is_empty() {
        panic!("HeaderField::from_static: pseudo-header value must not be empty");
    }
    let mut i = 0;
    while i < value.len() {
        let b = value[i];
        if !is_tchar_const(b) {
            panic!("HeaderField::from_static: pseudo-header value contains non-token byte");
        }
        i += 1;
    }
}

/// RFC 9110 §5.6.2 の tchar (大文字小文字両方を許容)
const fn is_tchar_const(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
        b'^' | b'_' | b'`' | b'|' | b'~' |
        b'0'..=b'9' |
        b'a'..=b'z' |
        b'A'..=b'Z'
    )
}

/// RFC 3986 §3.1: scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )
const fn check_scheme_const(value: &[u8]) {
    if value.is_empty() {
        panic!("HeaderField::from_static: :scheme value must not be empty (RFC 3986 3.1)");
    }
    let first = value[0];
    if !first.is_ascii_alphabetic() {
        panic!("HeaderField::from_static: :scheme value must start with ALPHA (RFC 3986 3.1)");
    }
    let mut i = 1;
    while i < value.len() {
        let b = value[i];
        if !(b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.') {
            panic!("HeaderField::from_static: :scheme value contains invalid byte (RFC 3986 3.1)");
        }
        i += 1;
    }
}

/// RFC 9113 §8.3.1: :path は absolute-path ("/" 始まり) または asterisk-form ("*")
///
/// 空の :path はランタイム検査 (`validate_pseudo_header`) と同様に許可する
/// (http/https スキーム依存の禁止は scheme と組で判断するため `validation.rs` に委ねる)。
const fn check_path_const(value: &[u8]) {
    if value.is_empty() {
        return;
    }
    if value.len() == 1 && value[0] == b'*' {
        return;
    }
    if value[0] != b'/' {
        panic!("HeaderField::from_static: :path must start with '/' or be '*' (RFC 9113 8.3.1)");
    }
}

/// const fn で使える `&[u8]` の等価比較
const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    #[test]
    fn header_bytes_as_slice() {
        let s = HeaderBytes::Static(b"foo");
        assert_eq!(s.as_slice(), b"foo");
        let o = HeaderBytes::Owned(b"bar".to_vec());
        assert_eq!(o.as_slice(), b"bar");
    }

    #[test]
    fn header_bytes_len() {
        assert_eq!(HeaderBytes::Static(b"").len(), 0);
        assert_eq!(HeaderBytes::Owned(b"abc".to_vec()).len(), 3);
    }

    #[test]
    fn header_bytes_static_owned_equal_when_same_slice() {
        let s = HeaderBytes::Static(b"GET");
        let o = HeaderBytes::Owned(b"GET".to_vec());
        assert_eq!(s, o);
        assert_eq!(o, s);
    }

    #[test]
    fn header_bytes_hash_consistent_across_variants() {
        let s = HeaderBytes::Static(b"content-type");
        let o = HeaderBytes::Owned(b"content-type".to_vec());
        let mut hs = DefaultHasher::new();
        s.hash(&mut hs);
        let mut ho = DefaultHasher::new();
        o.hash(&mut ho);
        assert_eq!(hs.finish(), ho.finish());
    }

    #[test]
    fn const_check_accepts_valid_pseudo() {
        const _: () = check_field_name_const(b":method");
        const _: () = check_pseudo_header_const(b":method", b"GET");
        const _: () = check_pseudo_header_const(b":status", b"200");
        const _: () = check_pseudo_header_const(b":scheme", b"https");
        const _: () = check_pseudo_header_const(b":path", b"/");
        const _: () = check_pseudo_header_const(b":path", b"*");
        const _: () = check_pseudo_header_const(b":protocol", b"webtransport");
        const _: () = check_field_value_const(b"GET");
    }

    #[test]
    fn const_check_accepts_valid_regular() {
        const _: () = check_field_name_const(b"content-type");
        const _: () = check_pseudo_header_const(b"content-type", b"text/html");
        const _: () = check_field_value_const(b"text/html");
    }
}
