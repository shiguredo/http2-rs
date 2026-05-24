//! HTTP セマンティクス検証 (RFC 9113 Section 8)
//!
//! HTTP/2 リクエストおよびレスポンスのヘッダー検証を提供する。
//!
//! 個別フィールドの値構文検査は [`crate::hpack::HeaderField::new`] に集約されている。
//! 本モジュールは「ヘッダーリスト全体の整合性」検査と、HPACK decoder 経路で
//! 検査をバイパスして構築された `HeaderField` の再検査を担う。
//! 再検査は [`crate::syntax`] の `validate_field_name` /
//! `validate_field_value` / `validate_pseudo_header` を直接呼ぶことで
//! 余分な alloc を避けつつ `HeaderField::new` と同等の検査を実施する。

use crate::error::{Error, ErrorCode};
use crate::hpack::{HeaderField, HeaderFieldError};

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

/// HTTP セマンティクス検証エラー
///
/// 個別フィールドの値構文違反は [`HeaderFieldError`] に集約されているため、
/// 本 enum はヘッダーリスト全体の整合性違反のみを扱う。
/// 受信経路で発生したフィールド単位のエラーは
/// [`Self::InvalidHeaderField`] でラップして報告する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// 個別フィールドの構築時検査エラー (HPACK decoder 経路で wire データから検出)
    InvalidHeaderField(HeaderFieldError),
    /// 必須の疑似ヘッダーが欠落
    MissingPseudoHeader(&'static str),
    /// 疑似ヘッダーが重複
    DuplicatePseudoHeader(&'static str),
    /// 疑似ヘッダーが通常ヘッダーの後に出現
    PseudoHeaderAfterRegular,
    /// 文脈で許可されない疑似ヘッダー (リクエストの `:status`、レスポンスの `:method` 等)
    DisallowedPseudoHeader(Vec<u8>),
    /// トレーラーに疑似ヘッダーが含まれている (RFC 9113 §8.1)
    PseudoHeaderInTrailers(Vec<u8>),
    /// 禁止されたヘッダー (RFC 9113 §8.2.2)
    ForbiddenHeader(Vec<u8>),
    /// TE ヘッダーの不正な値 (リクエストの TE は "trailers" のみ許可)
    InvalidTeHeader,
    /// `:path` が空 (http/https リクエスト)
    EmptyPath,
    /// OPTIONS 以外で `:path` が asterisk-form (`*`) になっている
    AsteriskPathOnNonOptions,
    /// CONNECT リクエストに `:path` または `:scheme` が含まれている
    ConnectWithPathOrScheme,
    /// CONNECT の `:authority` が authority-form (host:port) でない
    ConnectInvalidAuthority,
    /// CONNECT 以外のリクエストに `:path` または `:scheme` がない
    NonConnectMissingPathOrScheme,
    /// Extended CONNECT (RFC 8441) に `:scheme` または `:path` がない
    ExtendedConnectMissingSchemeOrPath,
    /// CONNECT 以外のリクエストに `:protocol` が含まれている
    ProtocolOnNonConnect,
    /// Host ヘッダーと `:authority` 疑似ヘッダーの値が不一致
    HostAuthorityMismatch,
    /// `:authority` に userinfo が含まれている (http/https/CONNECT のみ)
    AuthorityWithUserinfo,
    /// http/https スキームで `:authority` も Host もない
    MissingAuthority,
    /// `:status = 101` (Switching Protocols) は HTTP/2 では使えない (RFC 9113 §8.6)
    Status101NotSupported,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHeaderField(e) => write!(f, "invalid header field: {e}"),
            Self::MissingPseudoHeader(name) => write!(f, "missing required pseudo-header: {name}"),
            Self::DuplicatePseudoHeader(name) => write!(f, "duplicate pseudo-header: {name}"),
            Self::PseudoHeaderAfterRegular => write!(f, "pseudo-header after regular header"),
            Self::DisallowedPseudoHeader(name) => {
                write!(
                    f,
                    "pseudo-header not allowed in this context: {}",
                    String::from_utf8_lossy(name)
                )
            }
            Self::PseudoHeaderInTrailers(name) => {
                write!(
                    f,
                    "pseudo-header not allowed in trailers: {}",
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
            Self::AuthorityWithUserinfo => write!(f, ":authority must not include userinfo"),
            Self::MissingAuthority => {
                write!(
                    f,
                    "http/https request must include :authority or Host header"
                )
            }
            Self::Status101NotSupported => {
                write!(f, ":status 101 is not supported over HTTP/2")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

impl From<HeaderFieldError> for ValidationError {
    fn from(e: HeaderFieldError) -> Self {
        Self::InvalidHeaderField(e)
    }
}

/// RFC 3986 Section 6.2.3: scheme-based normalization でデフォルトポートを除去する
fn strip_default_port<'a>(authority: &'a [u8], scheme: Option<&[u8]>) -> &'a [u8] {
    let default_port: &[u8] = match scheme {
        Some(s) if s.eq_ignore_ascii_case(b"http") => b":80",
        Some(s) if s.eq_ignore_ascii_case(b"https") => b":443",
        _ => return authority,
    };
    authority.strip_suffix(default_port).unwrap_or(authority)
}

/// 個別フィールドの構築時検査と等価のチェックを再実行する
///
/// HPACK decoder 経路で `from_validated_parts` 経由に構築された `HeaderField`
/// は wire 上のバイト列を無検査で保持しているため、validation 層の入り口で
/// `HeaderField::new` と同等の検査を直接行い、大文字 field-name や CRLF を
/// 含む field-value 等を検出する。`HeaderField::new_with_sensitive` を呼ぶ
/// 実装だと Vec を 2 個確保して即捨てるため、検査関数を直接呼んで alloc を回避する。
fn check_field(header: &HeaderField) -> Result<(), Error> {
    use crate::syntax::{validate_field_name, validate_field_value, validate_pseudo_header};
    let name = header.name();
    let value = header.value();
    validate_field_name(name)
        .map_err(|e| malformed_error(ValidationError::InvalidHeaderField(e)))?;
    validate_field_value(name, value)
        .map_err(|e| malformed_error(ValidationError::InvalidHeaderField(e)))?;
    validate_pseudo_header(name, value)
        .map_err(|e| malformed_error(ValidationError::InvalidHeaderField(e)))?;
    Ok(())
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
        check_field(header)?;
        let name = header.name();

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
                method = Some(header.value());
            } else if name == pseudo_headers::SCHEME {
                if seen_scheme {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":scheme",
                    )));
                }
                seen_scheme = true;
                scheme_value = Some(header.value());
            } else if name == pseudo_headers::AUTHORITY {
                if seen_authority {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":authority",
                    )));
                }
                seen_authority = true;
                authority_value = Some(header.value());
            } else if name == pseudo_headers::PATH {
                if seen_path {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":path",
                    )));
                }
                // 空 :path 自体は http/https スキーム依存の判定なので、
                // ここでは値を保持するだけにし、後段の scheme 判定に委ねる
                // (RFC 9113 §8.3.1: http/https URI では :path は MUST NOT empty)。
                path_value = Some(header.value());
                seen_path = true;
            } else if name == pseudo_headers::PROTOCOL {
                if seen_protocol {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":protocol",
                    )));
                }
                seen_protocol = true;
            } else if name == pseudo_headers::STATUS {
                // :status はリクエストでは使用不可
                return Err(malformed_error(ValidationError::DisallowedPseudoHeader(
                    name.to_vec(),
                )));
            } else {
                // ここに到達するのは HeaderField::new で UnknownPseudoHeader として
                // 弾かれるはずの疑似ヘッダー名 (check_field 経由で既に Err になる)。
                // 防衛的に DisallowedPseudoHeader にフォールバックする。
                return Err(malformed_error(ValidationError::DisallowedPseudoHeader(
                    name.to_vec(),
                )));
            }
        } else {
            // 通常ヘッダー
            past_pseudo = true;

            // 禁止ヘッダーのチェック (リクエスト用: TE は "trailers" のみ許可)
            validate_forbidden_header_for_request(name, header.value())?;

            // host ヘッダーの値を保持
            if name.eq_ignore_ascii_case(b"host") {
                host_value = Some(header.value());
            }
        }
    }

    // RFC 9113 Section 8.3.1: Host と :authority の不一致チェック
    // RFC 3986 Section 6.2.3 (scheme-based normalization) に従って正規化して比較する。
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

    // RFC 9113 Section 8.3.1: :authority の userinfo 禁止は http/https/CONNECT に限定
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

        // RFC 9113 Section 8.3.1: http/https スキームでは :path は
        // absolute-path ("/" で始まる) または asterisk-form ("*") でなければならない。
        // この条件は HeaderField::new の :path 検査で既に弾かれるため、ここでは再検査しない。

        // RFC 9113 Section 8.3.1: http/https スキームでは :path は空であってはならない
        // ("This pseudo-header field MUST NOT be empty for 'http' or 'https' URIs")
        if let Some(scheme) = scheme_value
            && (scheme.eq_ignore_ascii_case(b"http") || scheme.eq_ignore_ascii_case(b"https"))
            && path_value == Some(b"")
        {
            return Err(malformed_error(ValidationError::EmptyPath));
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
        check_field(header)?;
        let name = header.name();

        if name.starts_with(b":") {
            if past_pseudo {
                return Err(malformed_error(ValidationError::PseudoHeaderAfterRegular));
            }

            if name == pseudo_headers::STATUS {
                if seen_status {
                    return Err(malformed_error(ValidationError::DuplicatePseudoHeader(
                        ":status",
                    )));
                }
                // HeaderField::new で 3DIGIT 検査済み。
                // RFC 9113 Section 8.6: HTTP/2 は 101 (Switching Protocols) をサポートしない
                if header.value() == b"101" {
                    return Err(malformed_error(ValidationError::Status101NotSupported));
                }
                seen_status = true;
            } else {
                // レスポンスで許可されていない疑似ヘッダー
                return Err(malformed_error(ValidationError::DisallowedPseudoHeader(
                    name.to_vec(),
                )));
            }
        } else {
            past_pseudo = true;
            validate_forbidden_header_for_response(name)?;
        }
    }

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
        check_field(header)?;
        let name = header.name();

        // トレーラーに疑似ヘッダーは含められない
        if name.starts_with(b":") {
            return Err(malformed_error(ValidationError::PseudoHeaderInTrailers(
                name.to_vec(),
            )));
        }

        // 禁止ヘッダーのチェック (トレーラー用: TE ヘッダー自体が禁止)
        validate_forbidden_header_for_response(name)?;
    }

    Ok(())
}

/// 禁止ヘッダーをチェックする (リクエスト用)
///
/// RFC 9113 Section 8.2.2: TE ヘッダーは HTTP/2 リクエストでのみ "trailers" 値に限り許可される。
fn validate_forbidden_header_for_request(name: &[u8], value: &[u8]) -> Result<(), Error> {
    validate_forbidden_header_common(name)?;

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
        let Some(bracket_end) = authority.iter().position(|&b| b == b']') else {
            return false;
        };
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
    if colon_pos == 0 {
        return false;
    }
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

    fn h(name: &str, value: &str) -> HeaderField {
        HeaderField::new(name, value).unwrap()
    }

    fn h_unchecked(name: &[u8], value: &[u8]) -> HeaderField {
        HeaderField::from_validated_parts(name.to_vec(), value.to_vec(), false)
    }

    #[test]
    fn test_uppercase_header_name_via_decoder_path() {
        // HPACK decoder 経路で from_validated_parts 経由に構築された大文字 name は
        // check_field の再検査により InvalidHeaderField として弾かれる
        let headers = vec![
            h(":method", "GET"),
            h(":scheme", "https"),
            h(":path", "/"),
            h_unchecked(b"Content-Type", b"text/html"),
        ];
        assert!(validate_request_headers(&headers).is_err());
    }
}
