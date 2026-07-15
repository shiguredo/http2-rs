//! field-name / field-value / 疑似ヘッダーの構文検査 (HTTP/2 セマンティクス層)
//!
//! HTTP/2 フィールド構文検査は HPACK ヘッダー圧縮 (RFC 7541) の責務ではなく、
//! HTTP/2 セマンティクス (RFC 9113 §8) の責務である。本モジュールは HPACK に
//! 依存せず、crate ルート直下に配置する。
//!
//! 以下の RFC 節に基づく検査を提供する:
//!
//! - field-name = token: RFC 9110 §5.1 + §5.6.2 (token = 1*tchar) + RFC 9113 §8.2.1
//! - field-name lowercase ASCII 必須 (MUST NOT 0x41-0x5a): RFC 9113 §8.2.1
//! - field-value NUL / CR / LF 禁止: RFC 9113 §8.2.1
//! - field-value 先頭末尾 SP / HTAB 禁止: RFC 9113 §8.2.1
//! - 疑似ヘッダー名集合: RFC 9113 §8.3.1, §8.3.2, RFC 8441 §4
//!   (`:method` / `:scheme` / `:authority` / `:path` / `:status` / `:protocol`)
//! - `:method` 値 token: RFC 9110 §9.1
//! - `:scheme` 値構文: RFC 3986 §3.1
//! - `:path` absolute-path / asterisk-form: RFC 9113 §8.3.1, RFC 9110 §4.1
//! - `:status` 3DIGIT: RFC 9112 §4, RFC 9110 §15
//! - `:protocol` 値 HTTP Upgrade Token: RFC 8441 §4 + RFC 9110 §7.8
//!
//! const fn 版 (`check_*_const`) と runtime 版 (`validate_*`) は同じ規則を
//! 別実装で持つ。runtime 版 `HeaderFieldError` が `Vec<u8>` フィールドを持つため
//! const 文脈で構築できず、ロジック共通化は行わない。
//! 両者は同一の RFC 規則をレビューで揃え、runtime 版は `HeaderField::new` と
//! validation テストでカバーする。

use crate::hpack::error::HeaderFieldError;

// === const fn 検査ヘルパ (`HeaderField::from_static` 用) ===
//
// `HeaderField::from_static` から呼ばれ、不正なリテラルを与えると
// const eval が panic することによりコンパイルエラーとなる。
//
// const fn の制約 (パターンマッチや for ループ等が使えない) のため、
// 非 const fn 版の runtime 検査と実装を共用できない。
// 検査内容を等価に保つために、本ファイルの定義を単一の参照仕様として扱うこと。

/// const fn 版 field-name 検査 (RFC 9113 §8.2.1, RFC 9110 §5.6.2)
///
/// runtime 版は [`validate_field_name`] (同ファイル内)。両者は同じ規則を別実装で持つ。
/// 同値性はレビューで保ち、runtime 版は `HeaderField::new` と validation テストでカバーする。
///
/// 検査内容:
/// - 空でないこと
/// - 大文字 ASCII (0x41-0x5a) を含まないこと
/// - ':' は先頭文字でのみ許可される (疑似ヘッダー)
/// - それ以外の文字は token (lowercase) 規則に従うこと
///
/// 違反時は `panic!` で const eval を停止し、利用者から見れば
/// コンパイルエラーとして検出される。
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
        } else if !is_token_char_lower(b) {
            panic!("HeaderField::from_static: field-name contains non-token byte (RFC 9110 5.6.2)");
        }
        i += 1;
    }
}

/// const fn 版 field-value 検査 (RFC 9113 §8.2.1)
///
/// runtime 版は [`validate_field_value`] (同ファイル内)。両者は同じ規則を別実装で持つ。
/// 同値性はレビューで保ち、runtime 版は `HeaderField::new` と validation テストでカバーする。
///
/// 検査内容:
/// - 先頭/末尾に SP (0x20) または HTAB (0x09) を含まないこと
/// - NUL (0x00) / CR (0x0d) / LF (0x0a) を含まないこと
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

/// const fn 版 疑似ヘッダー検査 (RFC 9113 §8.3.1, §8.3.2, RFC 8441 §4)
///
/// runtime 版は [`validate_pseudo_header`] (同ファイル内)。両者は同じ規則を別実装で持つ。
/// 同値性はレビューで保ち、runtime 版は `HeaderField::new` と validation テストでカバーする。
///
/// 定義済み疑似ヘッダー名のみ許可:
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
            panic!(
                "HeaderField::from_static: :status value must be 3 ASCII digits (RFC 9112 4, RFC 9110 15)"
            );
        }
        let mut i = 0;
        while i < 3 {
            let b = value[i];
            if !b.is_ascii_digit() {
                panic!(
                    "HeaderField::from_static: :status value must be 3 ASCII digits (RFC 9112 4, RFC 9110 15)"
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
        if !is_token_char_case_insensitive(b) {
            panic!("HeaderField::from_static: pseudo-header value contains non-token byte");
        }
        i += 1;
    }
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

// === runtime 検査 (`HeaderField::new` / `new_with_sensitive` 用) ===
//
// const fn 版と検査規則は同等だが、`HeaderFieldError` (Vec<u8> フィールドを持つ)
// を返すため const fn にはできない。

/// runtime 版 field-name 検査 (RFC 9113 §8.2.1, RFC 9110 §5.6.2)
///
/// const fn 版は [`check_field_name_const`] (同ファイル内)。両者は同じ規則を別実装で持つ。
/// 同値性はレビューで保ち、runtime 版は `HeaderField::new` と validation テストでカバーする。
///
/// エラー対応:
/// - 空 → `HeaderFieldError::EmptyFieldName`
/// - 大文字 ASCII → `HeaderFieldError::UppercaseFieldName`
///   (const fn 版: `"field-name must be lowercase ASCII"` panic)
/// - 非 token バイト / 非先頭の ':' → `HeaderFieldError::InvalidFieldNameByte`
///   (const fn 版: `"field-name contains non-token byte"` / `"':' is only allowed ..."` panic)
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

/// runtime 版 field-value 検査 (RFC 9113 §8.2.1)
///
/// const fn 版は [`check_field_value_const`] (同ファイル内)。両者は同じ規則を別実装で持つ。
/// 同値性はレビューで保ち、runtime 版は `HeaderField::new` と validation テストでカバーする。
///
/// エラー対応:
/// - 先頭/末尾の SP/HTAB → `HeaderFieldError::FieldValueLeadingOrTrailingWhitespace`
///   (const fn 版: `"field-value must not start/end with SP or HTAB"` panic)
/// - NUL/CR/LF → `HeaderFieldError::InvalidFieldValueByte`
///   (const fn 版: `"field-value must not contain NUL, CR, or LF"` panic)
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

/// runtime 版 疑似ヘッダー検査 (RFC 9113 §8.3.1, §8.3.2, RFC 8441 §4)
///
/// const fn 版は [`check_pseudo_header_const`] (同ファイル内)。両者は同じ規則を別実装で持つ。
/// 同値性はレビューで保ち、runtime 版は `HeaderField::new` と validation テストでカバーする。
///
/// エラー対応:
/// - 未知の疑似ヘッダー → `HeaderFieldError::UnknownPseudoHeader`
///   (const fn 版: `"unknown pseudo-header name"` panic)
/// - 値構文違反 → `HeaderFieldError::InvalidPseudoHeaderValue`
///   (const fn 版: 各疑似ヘッダーに対応する panic メッセージ)
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
            // RFC 9112 §4, RFC 9110 §15: 3DIGIT
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
