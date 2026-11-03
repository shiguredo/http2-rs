//! WT-Available-Protocols / WT-Protocol の RFC 9651 パーサー・シリアライザ
//!
//! draft-ietf-webtrans-http2-16 Section 3.3 は両ヘッダーの定義を
//! draft-ietf-webtrans-http3-16 Section 3.3 の Application Protocol Negotiation に
//! 委譲している (HTTP/2 draft 16 の規範参照は HTTP/3 draft 16)。
//! 実体は RFC 9651 Structured Field Values で、
//! - `WT-Available-Protocols`: List of String (preference order)
//! - `WT-Protocol`: Item of String
//!
//! 本モジュールは両ヘッダーに必要な List of String / Item of String に特化した
//! 必要最小限の実装を提供する。これらヘッダーで有効なのは String のみであり、
//! RFC 9651 の他の型 (Integer など) はパース失敗として拒否する。
//! 既知の制限: パラメータ値は Integer / Decimal / String / Token / Byte Sequence /
//! Boolean のみ読み飛ばし、RFC 9651 で追加された Date (`@`) と Display String
//! (`%"..."`) には未対応である。Date / Display String を値に持つパラメータを含む
//! 正当なヘッダーは field 全体のパース失敗として扱われる (HTTP/3 draft 16
//! Section 3.3 はパラメータに意味を定めず MUST 無視とするため、仕様への適合には
//! 別途の対応が要る)。
//! Dictionary パーサー (`init.rs`) とは独立して実装する。
//!
//! WebTransport の仕様は IETF draft であり、draft の改訂や RFC 化に
//! 伴って章番号・要求項目が変わりうる。

use crate::webtransport::error::WtError;

/// WT-Available-Protocols ヘッダーのパース結果
///
/// RFC 9651 List of String。要素順は preference order (= 入力順) を保持する。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WtAvailableProtocols {
    /// 仕様順 (= 入力順) を保持する。RFC 9651 List のセマンティクス。
    pub protocols: Vec<String>,
}

impl WtAvailableProtocols {
    /// HTTP ヘッダー値バイト列を RFC 9651 List of String としてパースする
    ///
    /// 入力は HPACK でデコードされたヘッダー値そのまま。
    /// String 以外の値型・パース不能は `WtError::invalid_input` を返す
    /// (呼び出し側は仕様に従い field 全体を無視してよい)。
    pub fn parse(value: &[u8]) -> Result<Self, WtError> {
        // RFC 9651 Section 4.2 step 1: 入力は ASCII であること
        if value.iter().any(|&b| b >= 0x80) {
            return Err(WtError::invalid_input(
                "WT-Available-Protocols: non-ASCII byte in header value",
            ));
        }

        let mut parser = ListParser::new(value);
        let mut protocols = Vec::new();

        // 先頭の OWS を破棄 (RFC 9651 §4.2 step 2 は SP のみだが、本実装は OWS を許容)
        parser.skip_ows();

        // 空入力は空 List として成功
        if parser.is_empty() {
            return Ok(Self { protocols });
        }

        loop {
            // 各メンバーは sf-string でなければならない
            let s = parser.parse_string()?;
            protocols.push(s);

            // パラメータは意味を持たないため読み飛ばす (仕様 MUST)
            parser.skip_parameters()?;

            parser.skip_ows();

            // 入力末尾なら成功
            if parser.is_empty() {
                return Ok(Self { protocols });
            }

            // メンバー区切りの "," を必須消費。trailing comma は不可
            if !parser.consume_if_eq(b',') {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: expected ',' between list members",
                ));
            }
            parser.skip_ows();
            if parser.is_empty() {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: trailing comma not allowed",
                ));
            }
        }
    }
}

/// WT-Protocol 値を RFC 9651 sf-string としてシリアライズする
///
/// RFC 9651 Section 4.1.6 (Serializing a String)。production は
/// Appendix C (非規範) の `sf-string = DQUOTE *( unescaped / "%" / bs-escaped ) DQUOTE`。
///
/// 値は ASCII printable (0x20-0x7E) のみ。それ以外は `WtError::invalid_input`。
pub fn serialize_wt_protocol(value: &[u8]) -> Result<Vec<u8>, WtError> {
    for &b in value {
        if !(0x20..=0x7E).contains(&b) {
            return Err(WtError::invalid_input(
                "wt-protocol value contains non-printable byte",
            ));
        }
    }

    // 入力由来サイズでの事前割当は行わない (shiguredo-rust 規約)
    let mut out = Vec::new();
    out.push(b'"');
    for &b in value {
        if b == b'"' || b == b'\\' {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b'"');
    Ok(out)
}

/// RFC 9651 List 専用パーサー
struct ListParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> ListParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    /// 先頭が指定バイトなら 1 つ消費して true を返す
    fn consume_if_eq(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// OWS (SP=0x20 / HTAB=0x09) を全て読み飛ばす
    fn skip_ows(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// RFC 9651 Section 4.2.5: Parsing a String
    ///
    /// List メンバーとして String 以外の型が来た場合は invalid_input。
    fn parse_string(&mut self) -> Result<String, WtError> {
        match self.peek() {
            Some(b'"') => {}
            // Token / Integer / Boolean / Byte Sequence / Inner List などは拒否
            Some(b) if b == b'-' || b.is_ascii_digit() => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: list member must be a String, got Integer/Decimal",
                ));
            }
            Some(b'?') => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: list member must be a String, got Boolean",
                ));
            }
            Some(b':') => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: list member must be a String, got Byte Sequence",
                ));
            }
            Some(b'(') => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: list member must be a String, got Inner List",
                ));
            }
            Some(b) if b == b'*' || b.is_ascii_alphabetic() => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: list member must be a String, got Token",
                ));
            }
            _ => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: list member must be a String",
                ));
            }
        }

        // 先頭の `"` を消費
        self.advance();

        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            if b == b'\\' {
                // RFC 9651 Section 4.2.5: エスケープは `\\` または `\"` のみ
                self.advance();
                match self.peek() {
                    Some(escaped @ (b'\\' | b'"')) => {
                        // エスケープ後の文字 (`"` または `\`) を値として格納する
                        out.push(escaped);
                        self.advance();
                    }
                    _ => {
                        return Err(WtError::invalid_input(
                            "WT-Available-Protocols: invalid escape sequence in string",
                        ));
                    }
                }
            } else if b == b'"' {
                self.advance();
                // ASCII printable のみなので from_utf8 は必ず成功する
                let s = String::from_utf8(out).expect("sf-string bytes are ASCII printable");
                return Ok(s);
            } else if !(0x20..=0x7E).contains(&b) {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: invalid character in string",
                ));
            } else {
                out.push(b);
                self.advance();
            }
        }
        Err(WtError::invalid_input(
            "WT-Available-Protocols: unterminated string",
        ))
    }

    /// RFC 9651 Section 4.2.3.2: Parsing Parameters
    ///
    /// `*( ";" *SP parameter )` を全て読み飛ばす。`parameter = key [ "=" bare-item ]`。
    fn skip_parameters(&mut self) -> Result<(), WtError> {
        while self.peek() == Some(b';') {
            self.advance();
            while self.consume_if_eq(b' ') {}
            self.parse_key()?;
            if self.consume_if_eq(b'=') {
                self.skip_bare_item()?;
            }
        }
        Ok(())
    }

    /// RFC 9651 Section 4.2.3.3: Parsing a Key
    fn parse_key(&mut self) -> Result<(), WtError> {
        match self.peek() {
            Some(b) if b.is_ascii_lowercase() || b == b'*' => {}
            _ => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: parameter key must start with lcalpha or '*'",
                ));
            }
        }
        while let Some(b) = self.peek() {
            if b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'_'
                || b == b'-'
                || b == b'*'
                || b == b'.'
            {
                self.advance();
            } else {
                break;
            }
        }
        Ok(())
    }

    /// パラメータ値として現れる bare item を読み飛ばす
    ///
    /// Integer / Decimal / String / Token / Byte Sequence / Boolean のみを扱う。
    /// RFC 9651 の Date / Display String は未対応で、`invalid_input` を返す
    /// (モジュール doc の既知の制限を参照)。
    fn skip_bare_item(&mut self) -> Result<(), WtError> {
        match self.peek() {
            Some(b) if b == b'-' || b.is_ascii_digit() => self.skip_number(),
            Some(b'"') => self.skip_string(),
            Some(b'?') => self.skip_boolean(),
            Some(b':') => self.skip_byte_sequence(),
            Some(b) if b == b'*' || b.is_ascii_alphabetic() => self.skip_token(),
            _ => Err(WtError::invalid_input(
                "WT-Available-Protocols: unexpected character in parameter value",
            )),
        }
    }

    fn skip_number(&mut self) -> Result<(), WtError> {
        if self.peek() == Some(b'-') {
            self.advance();
        }
        let digit_start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        if self.pos == digit_start {
            return Err(WtError::invalid_input(
                "WT-Available-Protocols: integer must contain at least one digit",
            ));
        }
        if self.peek() == Some(b'.') {
            self.advance();
            let frac_start = self.pos;
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            if self.pos == frac_start {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: decimal fractional part must contain digits",
                ));
            }
        }
        Ok(())
    }

    fn skip_string(&mut self) -> Result<(), WtError> {
        if !self.consume_if_eq(b'"') {
            return Err(WtError::invalid_input(
                "WT-Available-Protocols: string must start with '\"'",
            ));
        }
        while let Some(b) = self.peek() {
            if b == b'\\' {
                self.advance();
                match self.peek() {
                    Some(b'\\') | Some(b'"') => self.advance(),
                    _ => {
                        return Err(WtError::invalid_input(
                            "WT-Available-Protocols: invalid escape sequence in string",
                        ));
                    }
                }
            } else if b == b'"' {
                self.advance();
                return Ok(());
            } else if !(0x20..=0x7E).contains(&b) {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: invalid character in string",
                ));
            } else {
                self.advance();
            }
        }
        Err(WtError::invalid_input(
            "WT-Available-Protocols: unterminated string",
        ))
    }

    fn skip_token(&mut self) -> Result<(), WtError> {
        match self.peek() {
            Some(b) if b == b'*' || b.is_ascii_alphabetic() => self.advance(),
            _ => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: token must start with ALPHA or '*'",
                ));
            }
        }
        while let Some(b) = self.peek() {
            if is_tchar(b) || b == b':' || b == b'/' {
                self.advance();
            } else {
                break;
            }
        }
        Ok(())
    }

    fn skip_byte_sequence(&mut self) -> Result<(), WtError> {
        if !self.consume_if_eq(b':') {
            return Err(WtError::invalid_input(
                "WT-Available-Protocols: byte sequence must start with ':'",
            ));
        }
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=' {
                self.advance();
            } else {
                break;
            }
        }
        if !self.consume_if_eq(b':') {
            return Err(WtError::invalid_input(
                "WT-Available-Protocols: byte sequence must end with ':'",
            ));
        }
        Ok(())
    }

    fn skip_boolean(&mut self) -> Result<(), WtError> {
        if !self.consume_if_eq(b'?') {
            return Err(WtError::invalid_input(
                "WT-Available-Protocols: boolean must start with '?'",
            ));
        }
        match self.peek() {
            Some(b'0') | Some(b'1') => self.advance(),
            _ => {
                return Err(WtError::invalid_input(
                    "WT-Available-Protocols: boolean value must be '0' or '1'",
                ));
            }
        }
        Ok(())
    }
}

use super::is_tchar;
