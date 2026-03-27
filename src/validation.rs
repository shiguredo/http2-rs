//! HTTP セマンティクス検証 (RFC 9113 Section 8)
//!
//! HTTP/2 リクエストおよびレスポンスのヘッダー検証を提供する。

use crate::error::{Error, ErrorCode};
use crate::hpack::HeaderField;

/// 疑似ヘッダーフィールド名
pub mod pseudo_headers {
    pub const METHOD: &[u8] = b":method";
    pub const SCHEME: &[u8] = b":scheme";
    pub const AUTHORITY: &[u8] = b":authority";
    pub const PATH: &[u8] = b":path";
    pub const STATUS: &[u8] = b":status";
    pub const PROTOCOL: &[u8] = b":protocol";
}

/// 禁止されたヘッダーフィールド名 (RFC 9113 Section 8.2.2)
pub mod forbidden_headers {
    pub const CONNECTION: &[u8] = b"connection";
    pub const KEEP_ALIVE: &[u8] = b"keep-alive";
    pub const PROXY_CONNECTION: &[u8] = b"proxy-connection";
    pub const TRANSFER_ENCODING: &[u8] = b"transfer-encoding";
    pub const UPGRADE: &[u8] = b"upgrade";
}

/// TE ヘッダーで許可される値
pub const TE_ALLOWED_VALUE: &[u8] = b"trailers";

/// 検証結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// 必須の疑似ヘッダーが欠落
    MissingPseudoHeader(&'static str),
    /// 疑似ヘッダーが重複
    DuplicatePseudoHeader(&'static str),
    /// 疑似ヘッダーが通常ヘッダーの後に出現
    PseudoHeaderAfterRegular,
    /// 不正な疑似ヘッダー
    InvalidPseudoHeader(Vec<u8>),
    /// 禁止されたヘッダー
    ForbiddenHeader(Vec<u8>),
    /// TE ヘッダーの不正な値
    InvalidTeHeader,
    /// :path が空
    EmptyPath,
    /// OPTIONS 以外で :path が asterisk-form (*) になっている
    AsteriskPathOnNonOptions,
    /// CONNECT リクエストに :path または :scheme が含まれている
    ConnectWithPathOrScheme,
    /// CONNECT の :authority が authority-form (host:port) でない
    ConnectInvalidAuthority,
    /// CONNECT 以外のリクエストに :path または :scheme がない
    NonConnectMissingPathOrScheme,
    /// Extended CONNECT (RFC 8441) に :scheme または :path がない
    ExtendedConnectMissingSchemeOrPath,
    /// CONNECT 以外のリクエストに :protocol が含まれている
    ProtocolOnNonConnect,
    /// Host ヘッダーと :authority 疑似ヘッダーの値が不一致
    HostAuthorityMismatch,
    /// http/https スキームで :authority も Host もない
    MissingAuthority,
    /// 不正なヘッダー名（禁止文字を含む）
    InvalidHeaderName(Vec<u8>),
    /// 不正なヘッダー値（NUL/CR/LF を含む）
    InvalidHeaderValue(Vec<u8>),
    /// 不正なステータスコード
    InvalidStatusCode(Vec<u8>),
    /// :authority に userinfo が含まれている
    AuthorityWithUserinfo,
    /// :protocol の値が不正 (空または非 token 文字を含む)
    InvalidProtocolValue(Vec<u8>),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPseudoHeader(name) => write!(f, "missing required pseudo-header: {name}"),
            Self::DuplicatePseudoHeader(name) => write!(f, "duplicate pseudo-header: {name}"),
            Self::PseudoHeaderAfterRegular => {
                write!(f, "pseudo-header after regular header")
            }
            Self::InvalidPseudoHeader(name) => {
                write!(
                    f,
                    "invalid pseudo-header: {}",
                    String::from_utf8_lossy(name)
                )
            }
            Self::ForbiddenHeader(name) => {
                write!(f, "forbidden header: {}", String::from_utf8_lossy(name))
            }
            Self::InvalidTeHeader => write!(f, "TE header with value other than 'trailers'"),
            Self::EmptyPath => write!(f, ":path is empty"),
            Self::AsteriskPathOnNonOptions => {
                write!(f, ":path '*' is only allowed for OPTIONS requests")
            }
            Self::ConnectWithPathOrScheme => {
                write!(f, "CONNECT request must not include :path or :scheme")
            }
            Self::ConnectInvalidAuthority => {
                write!(
                    f,
                    "CONNECT :authority must be in authority-form (host:port)"
                )
            }
            Self::NonConnectMissingPathOrScheme => {
                write!(f, "non-CONNECT request must include :path and :scheme")
            }
            Self::ExtendedConnectMissingSchemeOrPath => {
                write!(f, "Extended CONNECT request must include :scheme and :path")
            }
            Self::ProtocolOnNonConnect => {
                write!(f, ":protocol is only allowed with CONNECT method")
            }
            Self::HostAuthorityMismatch => {
                write!(f, "Host header differs from :authority pseudo-header")
            }
            Self::MissingAuthority => {
                write!(
                    f,
                    "http/https request must include :authority or Host header"
                )
            }
            Self::InvalidHeaderName(name) => {
                write!(f, "invalid header name: {}", String::from_utf8_lossy(name))
            }
            Self::InvalidHeaderValue(value) => {
                write!(
                    f,
                    "invalid header value: {}",
                    String::from_utf8_lossy(value)
                )
            }
            Self::InvalidStatusCode(value) => {
                write!(f, "invalid status code: {}", String::from_utf8_lossy(value))
            }
            Self::AuthorityWithUserinfo => {
                write!(f, ":authority must not include userinfo")
            }
            Self::InvalidProtocolValue(value) => {
                write!(
                    f,
                    "invalid :protocol value: {}",
                    String::from_utf8_lossy(value)
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// RFC 3986 Section 6.2.3: scheme-based normalization でデフォルトポートを除去する
///
/// `host:80` (http) → `host`, `host:443` (https) → `host`
fn strip_default_port<'a>(authority: &'a [u8], scheme: Option<&[u8]>) -> &'a [u8] {
    let default_port: &[u8] = match scheme {
        Some(s) if s.eq_ignore_ascii_case(b"http") => b":80",
        Some(s) if s.eq_ignore_ascii_case(b"https") => b":443",
        _ => return authority,
    };
    authority.strip_suffix(default_port).unwrap_or(authority)
}

/// リクエストヘッダーを検証する
///
/// RFC 9113 Section 8.3.1 に従って、HTTP リクエストメッセージを検証する。
///
/// # Errors
///
/// 不正なリクエストの場合は `Error` を返す。
pub fn validate_request_headers(headers: &[HeaderField]) -> Result<(), Error> {
    let mut seen_method = false;
    let mut seen_scheme = false;
    let mut seen_authority = false;
    let mut seen_path = false;
    let mut seen_protocol = false;
    let mut past_pseudo = false;
    let mut method: Option<&[u8]> = None;
    let mut scheme_value: Option<&[u8]> = None;
    let mut authority_value: Option<&[u8]> = None;
    let mut host_value: Option<&[u8]> = None;
    let mut path_value: Option<&[u8]> = None;

    for header in headers {
        let name = &header.name;

        if name.starts_with(b":") {
            // 疑似ヘッダー
            if past_pseudo {
                return Err(malformed_error(ValidationError::PseudoHeaderAfterRegular));
            }

            if name == pseudo_headers::METHOD {
                if seen_method {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":method",
                    )));
                }
                seen_method = true;
                method = Some(&header.value);
            } else if name == pseudo_headers::SCHEME {
                if seen_scheme {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":scheme",
                    )));
                }
                seen_scheme = true;
                scheme_value = Some(&header.value);
            } else if name == pseudo_headers::AUTHORITY {
                if seen_authority {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":authority",
                    )));
                }
                seen_authority = true;
                authority_value = Some(&header.value);
            } else if name == pseudo_headers::PATH {
                if seen_path {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":path",
                    )));
                }
                if header.value.is_empty() {
                    return Err(malformed_error(ValidationError::EmptyPath));
                }
                path_value = Some(&header.value);
                seen_path = true;
            } else if name == pseudo_headers::PROTOCOL {
                if seen_protocol {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":protocol",
                    )));
                }
                // RFC 8441 Section 4: :protocol の値は HTTP Upgrade Token (token 形式)
                if !is_valid_token(&header.value) {
                    return Err(malformed_error(ValidationError::InvalidProtocolValue(
                        header.value.clone(),
                    )));
                }
                seen_protocol = true;
            } else if name == pseudo_headers::STATUS {
                // :status はリクエストでは使用不可
                return Err(malformed_error(ValidationError::InvalidPseudoHeader(
                    name.clone(),
                )));
            } else {
                return Err(malformed_error(ValidationError::InvalidPseudoHeader(
                    name.clone(),
                )));
            }

            // 疑似ヘッダーの値にも NUL/CR/LF チェック
            validate_header_value_chars(&header.value)?;
        } else {
            // 通常ヘッダー
            past_pseudo = true;

            // ヘッダー名の文字検証 (RFC 9110 token ルール + 小文字強制)
            validate_header_name_chars(name)?;

            // ヘッダー値の文字検証 (NUL/CR/LF 禁止)
            validate_header_value_chars(&header.value)?;

            // 禁止ヘッダーのチェック (リクエスト用: TE は "trailers" のみ許可)
            validate_forbidden_header_for_request(name, &header.value)?;

            // host ヘッダーの値を保持
            if name.eq_ignore_ascii_case(b"host") {
                host_value = Some(&header.value);
            }
        }
    }

    // RFC 9113 Section 8.3.1: Host と :authority の不一致チェック
    // RFC 9113 Section 8.3.1: 値の比較には正規化が必要 (RFC 3986 Section 6.2)。
    // RFC 3986 Section 6.2.3 (scheme-based normalization):
    // - ホスト名の大文字小文字正規化
    // - デフォルトポートの正規化 (http:80, https:443 を除去)
    if let (Some(authority), Some(host)) = (authority_value, host_value) {
        let norm_authority = strip_default_port(authority, scheme_value);
        let norm_host = strip_default_port(host, scheme_value);
        if !norm_authority.eq_ignore_ascii_case(norm_host) {
            return Err(malformed_error(ValidationError::HostAuthorityMismatch));
        }
    }

    // 必須ヘッダーのチェック
    if !seen_method {
        return Err(malformed_error(ValidationError::MissingPseudoHeader(
            ":method",
        )));
    }

    // RFC 9113 Section 8.3.1: :authority の userinfo 禁止は http/https と CONNECT に限定
    if let Some(authority) = authority_value
        && authority.contains(&b'@')
    {
        let is_http_scheme = scheme_value
            .is_some_and(|s| s.eq_ignore_ascii_case(b"http") || s.eq_ignore_ascii_case(b"https"));
        let is_connect = method == Some(b"CONNECT");
        if is_http_scheme || is_connect {
            return Err(malformed_error(ValidationError::AuthorityWithUserinfo));
        }
    }

    // CONNECT メソッドの特別処理
    if method == Some(b"CONNECT") {
        if seen_protocol {
            // Extended CONNECT (RFC 8441)
            // :protocol が存在する場合、:scheme と :path が必須
            if !seen_scheme {
                return Err(malformed_error(
                    ValidationError::ExtendedConnectMissingSchemeOrPath,
                ));
            }
            if !seen_path {
                return Err(malformed_error(
                    ValidationError::ExtendedConnectMissingSchemeOrPath,
                ));
            }
            // :authority も必須
            if !seen_authority {
                return Err(malformed_error(ValidationError::MissingPseudoHeader(
                    ":authority",
                )));
            }
        } else {
            // 通常の CONNECT
            if seen_path || seen_scheme {
                return Err(malformed_error(ValidationError::ConnectWithPathOrScheme));
            }
            // CONNECT は :authority が必須（:path と :scheme は禁止）
            if !seen_authority {
                return Err(malformed_error(ValidationError::MissingPseudoHeader(
                    ":authority",
                )));
            }
            // RFC 9113 Section 8.5: :authority は authority-form (host:port) でなければならない
            if let Some(authority) = authority_value
                && !is_valid_connect_authority(authority)
            {
                return Err(malformed_error(ValidationError::ConnectInvalidAuthority));
            }
        }
    } else {
        // CONNECT 以外で :protocol は禁止
        if seen_protocol {
            return Err(malformed_error(ValidationError::ProtocolOnNonConnect));
        }
        // CONNECT 以外は :scheme と :path が必須
        if !seen_scheme {
            return Err(malformed_error(ValidationError::MissingPseudoHeader(
                ":scheme",
            )));
        }
        if !seen_path {
            return Err(malformed_error(ValidationError::MissingPseudoHeader(
                ":path",
            )));
        }

        // RFC 9113 Section 8.3.1: asterisk-form (*) は OPTIONS のみ
        if path_value == Some(b"*") && method != Some(b"OPTIONS") {
            return Err(malformed_error(ValidationError::AsteriskPathOnNonOptions));
        }

        // RFC 9113 Section 8.3.1: http/https スキームでは :authority または Host が必須
        if let Some(scheme) = scheme_value
            && (scheme.eq_ignore_ascii_case(b"http") || scheme.eq_ignore_ascii_case(b"https"))
            && !seen_authority
            && host_value.is_none()
        {
            return Err(malformed_error(ValidationError::MissingAuthority));
        }
    }

    Ok(())
}

/// レスポンスヘッダーを検証する
///
/// RFC 9113 Section 8.3.2 に従って、HTTP レスポンスメッセージを検証する。
///
/// # Errors
///
/// 不正なレスポンスの場合は `Error` を返す。
pub fn validate_response_headers(headers: &[HeaderField]) -> Result<(), Error> {
    let mut seen_status = false;
    let mut past_pseudo = false;

    for header in headers {
        let name = &header.name;

        if name.starts_with(b":") {
            // 疑似ヘッダー
            if past_pseudo {
                return Err(malformed_error(ValidationError::PseudoHeaderAfterRegular));
            }

            if name == pseudo_headers::STATUS {
                if seen_status {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":status",
                    )));
                }
                // RFC 9113 Section 8.3.2: :status は HTTP status code field を運ぶ。
                // HTTP status code は 3 桁の ASCII 数字でなければならない (RFC 9110 Section 15)。
                // HTTP/2 は 101 (Switching Protocols) をサポートしない。
                if header.value.len() != 3
                    || !header.value.iter().all(|b| b.is_ascii_digit())
                    || header.value == b"101"
                {
                    return Err(malformed_error(ValidationError::InvalidStatusCode(
                        header.value.clone(),
                    )));
                }
                seen_status = true;
            } else {
                // レスポンスで許可されていない疑似ヘッダー
                return Err(malformed_error(ValidationError::InvalidPseudoHeader(
                    name.clone(),
                )));
            }

            // 疑似ヘッダーの値にも NUL/CR/LF チェック
            validate_header_value_chars(&header.value)?;
        } else {
            // 通常ヘッダー
            past_pseudo = true;

            // ヘッダー名の文字検証 (RFC 9110 token ルール + 小文字強制)
            validate_header_name_chars(name)?;

            // ヘッダー値の文字検証 (NUL/CR/LF 禁止)
            validate_header_value_chars(&header.value)?;

            // 禁止ヘッダーのチェック (レスポンス用: TE ヘッダー自体が禁止)
            validate_forbidden_header_for_response(name)?;
        }
    }

    // 必須ヘッダーのチェック
    if !seen_status {
        return Err(malformed_error(ValidationError::MissingPseudoHeader(
            ":status",
        )));
    }

    Ok(())
}

/// トレーラーヘッダーを検証する
///
/// RFC 9113 Section 8.1 に従って、トレーラーを検証する。
///
/// # Errors
///
/// 不正なトレーラーの場合は `Error` を返す。
pub fn validate_trailers(headers: &[HeaderField]) -> Result<(), Error> {
    for header in headers {
        let name = &header.name;

        // トレーラーに疑似ヘッダーは含められない
        if name.starts_with(b":") {
            return Err(malformed_error(ValidationError::InvalidPseudoHeader(
                name.clone(),
            )));
        }

        // ヘッダー名の文字検証 (RFC 9110 token ルール + 小文字強制)
        validate_header_name_chars(name)?;

        // ヘッダー値の文字検証 (NUL/CR/LF 禁止)
        validate_header_value_chars(&header.value)?;

        // 禁止ヘッダーのチェック (トレーラー用: TE ヘッダー自体が禁止)
        validate_forbidden_header_for_response(name)?;
    }

    Ok(())
}

/// ヘッダー名が RFC 9110 Section 5.1 の token ルールに従うか検証する
///
/// token = 1*tchar
/// tchar = "!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." /
///         "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA
///
/// RFC 9113 Section 8.2.1: HTTP/2 ではフィールド名は小文字でなければならない。
fn validate_header_name_chars(name: &[u8]) -> Result<(), Error> {
    if name.is_empty() {
        return Err(malformed_error(ValidationError::InvalidHeaderName(
            name.to_vec(),
        )));
    }

    for &b in name {
        if !is_token_char(b) {
            return Err(malformed_error(ValidationError::InvalidHeaderName(
                name.to_vec(),
            )));
        }
    }

    Ok(())
}

/// RFC 9110 token 文字かどうかを判定する (小文字のみ許可)
const fn is_token_char(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
        b'^' | b'_' | b'`' | b'|' | b'~' |
        b'0'..=b'9' |
        b'a'..=b'z'
    )
}

/// RFC 9110 token 文字かどうかを判定する (大文字・小文字両方許可)
///
/// :protocol 値など、HTTP token 形式の値検証に使用する。
const fn is_token_char_case_insensitive(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
        b'^' | b'_' | b'`' | b'|' | b'~' |
        b'0'..=b'9' |
        b'a'..=b'z' |
        b'A'..=b'Z'
    )
}

/// バイト列が有効な HTTP token (1*tchar) かどうかを検証する
fn is_valid_token(value: &[u8]) -> bool {
    !value.is_empty() && value.iter().all(|&b| is_token_char_case_insensitive(b))
}

/// ヘッダー値に禁止文字が含まれていないか検証する
///
/// RFC 9110 Section 5.5: フィールド値に NUL (0x00), CR (0x0d), LF (0x0a) は禁止。
/// RFC 9113 Section 8.2.1: フィールド値の先頭/末尾に SP (0x20) / HTAB (0x09) は禁止。
fn validate_header_value_chars(value: &[u8]) -> Result<(), Error> {
    // RFC 9113 Section 8.2.1: 先頭の SP/HTAB は禁止
    if let Some(&first) = value.first()
        && (first == 0x20 || first == 0x09)
    {
        return Err(malformed_error(ValidationError::InvalidHeaderValue(
            value.to_vec(),
        )));
    }
    // RFC 9113 Section 8.2.1: 末尾の SP/HTAB は禁止
    if let Some(&last) = value.last()
        && (last == 0x20 || last == 0x09)
    {
        return Err(malformed_error(ValidationError::InvalidHeaderValue(
            value.to_vec(),
        )));
    }
    for &b in value {
        if b == 0x00 || b == 0x0d || b == 0x0a {
            return Err(malformed_error(ValidationError::InvalidHeaderValue(
                value.to_vec(),
            )));
        }
    }

    Ok(())
}

/// 禁止ヘッダーをチェックする (リクエスト用)
///
/// RFC 9113 Section 8.2.2: TE ヘッダーは HTTP/2 リクエストでのみ "trailers" 値に限り許可される。
fn validate_forbidden_header_for_request(name: &[u8], value: &[u8]) -> Result<(), Error> {
    validate_forbidden_header_common(name)?;

    // RFC 9113 Section 8.2.2: TE ヘッダーはリクエストでのみ "trailers" 値に限り許可
    if name.eq_ignore_ascii_case(b"te") && !value.eq_ignore_ascii_case(TE_ALLOWED_VALUE) {
        return Err(malformed_error(ValidationError::InvalidTeHeader));
    }

    Ok(())
}

/// 禁止ヘッダーをチェックする (レスポンス・トレーラー用)
///
/// RFC 9113 Section 8.2.2: TE ヘッダーの例外はリクエストに限定されるため、
/// レスポンスやトレーラーでは TE ヘッダー自体が禁止される。
fn validate_forbidden_header_for_response(name: &[u8]) -> Result<(), Error> {
    validate_forbidden_header_common(name)?;

    // RFC 9113 Section 8.2.2: レスポンス・トレーラーでは TE ヘッダーは禁止
    if name.eq_ignore_ascii_case(b"te") {
        return Err(malformed_error(ValidationError::ForbiddenHeader(
            name.to_vec(),
        )));
    }

    Ok(())
}

/// 接続固有の禁止ヘッダーをチェックする (共通部分)
fn validate_forbidden_header_common(name: &[u8]) -> Result<(), Error> {
    if name.eq_ignore_ascii_case(forbidden_headers::CONNECTION)
        || name.eq_ignore_ascii_case(forbidden_headers::KEEP_ALIVE)
        || name.eq_ignore_ascii_case(forbidden_headers::PROXY_CONNECTION)
        || name.eq_ignore_ascii_case(forbidden_headers::TRANSFER_ENCODING)
        || name.eq_ignore_ascii_case(forbidden_headers::UPGRADE)
    {
        return Err(malformed_error(ValidationError::ForbiddenHeader(
            name.to_vec(),
        )));
    }

    Ok(())
}

/// RFC 9113 Section 8.5: CONNECT の :authority が authority-form (host:port) か検証する
///
/// authority-form = uri-host ":" port (RFC 9112 Section 3.2.3)
/// IPv6 リテラル ([::1]:443) を考慮する。
fn is_valid_connect_authority(authority: &[u8]) -> bool {
    if authority.is_empty() {
        return false;
    }

    // IPv6 リテラルの場合: [host]:port
    if authority.starts_with(b"[") {
        // ']' を探す
        let Some(bracket_end) = authority.iter().position(|&b| b == b']') else {
            return false;
        };
        // ']:' の後に port が続く必要がある
        let rest = &authority[bracket_end + 1..];
        if !rest.starts_with(b":") || rest.len() < 2 {
            return false;
        }
        return rest[1..].iter().all(|b| b.is_ascii_digit());
    }

    // IPv4 / ホスト名の場合: 最後の ':' 以降が port
    let Some(colon_pos) = authority.iter().rposition(|&b| b == b':') else {
        return false;
    };
    // host 部分が空でないこと
    if colon_pos == 0 {
        return false;
    }
    // port 部分が空でなく全て数字であること
    let port = &authority[colon_pos + 1..];
    !port.is_empty() && port.iter().all(|b| b.is_ascii_digit())
}

/// Malformed メッセージエラーを生成する
fn malformed_error(validation_error: ValidationError) -> Error {
    Error::stream_error(ErrorCode::ProtocolError, validation_error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_get_request() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
        ];

        assert!(validate_request_headers(&headers).is_ok());
    }

    #[test]
    fn test_valid_connect_request() {
        let headers = vec![
            HeaderField::from_str(":method", "CONNECT"),
            HeaderField::from_str(":authority", "example.com:443"),
        ];

        assert!(validate_request_headers(&headers).is_ok());
    }

    #[test]
    fn test_missing_method() {
        let headers = vec![
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_missing_scheme() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":path", "/"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_missing_path() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_duplicate_method() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":method", "POST"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_pseudo_header_after_regular() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str("content-type", "text/html"),
            HeaderField::from_str(":scheme", "https"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_forbidden_connection_header() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str("connection", "close"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_forbidden_transfer_encoding() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str("transfer-encoding", "chunked"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_te_trailers_allowed() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::from_str("te", "trailers"),
        ];

        assert!(validate_request_headers(&headers).is_ok());
    }

    #[test]
    fn test_te_gzip_forbidden() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str("te", "gzip"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_te_trailers_forbidden_in_response() {
        // RFC 9113 Section 8.2.2: TE ヘッダーの例外はリクエストに限定される
        let headers = vec![
            HeaderField::from_str(":status", "200"),
            HeaderField::from_str("te", "trailers"),
        ];

        assert!(validate_response_headers(&headers).is_err());
    }

    #[test]
    fn test_te_trailers_forbidden_in_trailers() {
        // RFC 9113 Section 8.2.2: TE ヘッダーの例外はリクエストに限定される
        let headers = vec![HeaderField::from_str("te", "trailers")];

        assert!(validate_trailers(&headers).is_err());
    }

    #[test]
    fn test_uppercase_header_name() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str("Content-Type", "text/html"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_empty_path() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", ""),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_connect_with_path() {
        let headers = vec![
            HeaderField::from_str(":method", "CONNECT"),
            HeaderField::from_str(":authority", "example.com:443"),
            HeaderField::from_str(":path", "/"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_valid_response() {
        let headers = vec![
            HeaderField::from_str(":status", "200"),
            HeaderField::from_str("content-type", "text/html"),
        ];

        assert!(validate_response_headers(&headers).is_ok());
    }

    #[test]
    fn test_response_missing_status() {
        let headers = vec![HeaderField::from_str("content-type", "text/html")];

        assert!(validate_response_headers(&headers).is_err());
    }

    #[test]
    fn test_response_with_method() {
        let headers = vec![
            HeaderField::from_str(":status", "200"),
            HeaderField::from_str(":method", "GET"),
        ];

        assert!(validate_response_headers(&headers).is_err());
    }

    #[test]
    fn test_valid_trailers() {
        let headers = vec![
            HeaderField::from_str("x-checksum", "abc123"),
            HeaderField::from_str("x-trailer", "value"),
        ];

        assert!(validate_trailers(&headers).is_ok());
    }

    #[test]
    fn test_host_authority_mismatch() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::from_str("host", "other.com"),
        ];

        assert!(validate_request_headers(&headers).is_err());
    }

    #[test]
    fn test_host_authority_match() {
        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":path", "/"),
            HeaderField::from_str(":authority", "example.com"),
            HeaderField::from_str("host", "example.com"),
        ];

        assert!(validate_request_headers(&headers).is_ok());
    }

    #[test]
    fn test_trailers_with_pseudo_header() {
        let headers = vec![
            HeaderField::from_str(":status", "200"),
            HeaderField::from_str("x-trailer", "value"),
        ];

        assert!(validate_trailers(&headers).is_err());
    }
}
