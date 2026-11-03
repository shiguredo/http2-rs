//! WebTransport-Init ヘッダーフィールドのパーサー
//!
//! draft-ietf-webtrans-http2-16 Section 4.3.2 に定義されている
//! `WebTransport-Init` HTTP ヘッダーフィールドをパースする。
//! 実体は RFC 9651 (Structured Field Values for HTTP) Dictionary。
//! RFC 9651 で追加された Date / Display String も読み飛ばしと構文検証のために扱う。
//!
//! 本モジュールは known キー (`u` / `bl` / `br`) のみ抽出する必要最小限の
//! Dictionary パーサーを提供する。仕様の MUST 要件:
//! - 未知キーとパラメータは MUST 無視する (Section 4.3.2)
//! - パース不能・型不一致・値範囲外は MUST 4xx で拒否する (Section 4.3.2)
//! - 重複キーは RFC 9651 Section 4.2.2 の規則で last-wins
//!
//! 本実装が参照する仕様は IETF draft (`-16`) であり、draft の改訂や RFC 化に
//! 伴って章番号・要求項目が変わりうる。

use crate::webtransport::error::WtError;

/// WebTransport-Init ヘッダーフィールドの値
///
/// draft-ietf-webtrans-http2-16 Section 4.3.2 で定義された
/// 3 つの Integer キーを optional に保持する。
///
/// `None` はキーがヘッダーに含まれていなかったことを示し、
/// SETTINGS 由来の値とマージする際に上書き対象外として扱う。
///
/// このヘッダーが持つのはストリームデータの初期上限のみであり、
/// セッション上限とストリーム数上限は、セッション開始時に `WT_MAX_DATA` /
/// `WT_MAX_STREAMS` capsule を要求または応答とともに送ることで提供できる
/// (Section 4.3.2)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WtInit {
    /// `u`: ピア (=送信者から見た recipient) が開く単方向ストリームの初期最大データ量
    pub u: Option<u64>,
    /// `bl`: 送信者が開く双方向ストリームの初期最大データ量
    pub bl: Option<u64>,
    /// `br`: ピア (=送信者から見た recipient) が開く双方向ストリームの初期最大データ量
    pub br: Option<u64>,
}

impl WtInit {
    /// HTTP ヘッダー値バイト列をパースする
    ///
    /// 入力は HPACK でデコードされたヘッダー値そのまま (RFC 9651 Section 4.2 step 1:
    /// 非 ASCII バイトを含む入力はパース失敗)。RFC 9651 のパース手順を必要最小限で実装する。
    pub fn parse(value: &[u8]) -> Result<Self, WtError> {
        // RFC 9651 Section 4.2 step 1: 入力は ASCII であること
        if value.iter().any(|&b| b >= 0x80) {
            return Err(WtError::invalid_input(
                "WebTransport-Init: non-ASCII byte in header value",
            ));
        }

        let mut parser = DictionaryParser::new(value);
        let mut out = WtInit::default();

        // RFC 9651 Section 4.2 step 2 は先頭の SP のみ破棄を規定する。
        // 本実装は OWS (SP / HTAB) まで許容する独自緩和
        parser.skip_ows();

        while !parser.is_empty() {
            // RFC 9651 Section 4.2.2 step 2.1: Parse a Key
            let key = parser.parse_key()?;

            // RFC 9651 Section 4.2.2 step 2.2/2.3: "=" の有無で値を決める
            let value = if parser.consume_if_eq(b'=') {
                // Bare Item または Inner List をパースする
                parser.parse_value()?
            } else {
                // "=" 省略時は Boolean true (本実装は値そのものを使わないのでタグだけ立てる)
                SfValue::Boolean
            };

            // RFC 9651 Section 3.1.2: Parameters を消費 (本実装ではすべて無視)
            parser.skip_parameters()?;

            // known キーのみ WtInit に格納する
            // (RFC 9651 §4.2.2 step 2.4 は last-wins、未知キーは Section 4.3.2 で無視)。
            // 既知キーの型検査は辞書メンバーごとに行うため、先行するメンバーが型違反なら
            // last-wins の解決を待たずにその時点で Err を返す
            match key.as_slice() {
                b"u" => out.u = Some(extract_nonneg_integer(&value, "u")?),
                b"bl" => out.bl = Some(extract_nonneg_integer(&value, "bl")?),
                b"br" => out.br = Some(extract_nonneg_integer(&value, "br")?),
                _ => {} // 未知キーは値の型に関わらず無視する
            }

            // RFC 9651 Section 4.2.2 step 2.6: trailing OWS を破棄
            parser.skip_ows();

            // RFC 9651 Section 4.2.2 step 2.7: 入力末尾なら成功
            if parser.is_empty() {
                return Ok(out);
            }

            // RFC 9651 Section 4.2.2 step 2.8-2.10: "," を必須消費、trailing comma は不可
            if !parser.consume_if_eq(b',') {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: expected ',' between dictionary members",
                ));
            }
            parser.skip_ows();
            if parser.is_empty() {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: trailing comma not allowed",
                ));
            }
        }

        Ok(out)
    }
}

/// known キー (`u` / `bl` / `br`) の値が Integer で、かつ 0 以上の範囲に収まることを確認する
///
/// 仕様 (Section 4.3.2) は「Integer」とのみ規定。フロー制御の初期値として
/// 負値・小数・他の型は意味を持たないため `WtError::invalid_input` を返す。
fn extract_nonneg_integer(value: &SfValue, key: &str) -> Result<u64, WtError> {
    match value {
        SfValue::Integer(n) if *n >= 0 => Ok(*n as u64),
        SfValue::Integer(_) => Err(WtError::invalid_input(format!(
            "WebTransport-Init: key '{key}' must be non-negative integer"
        ))),
        _ => Err(WtError::invalid_input(format!(
            "WebTransport-Init: key '{key}' must be an Integer"
        ))),
    }
}

/// `parse_number` が返す数値 (Integer か Decimal か)
#[derive(Debug, Clone, Copy)]
enum SfNumber {
    Integer(i64),
    Decimal,
}

/// RFC 9651 の Bare Item (`u`/`bl`/`br` で実際に意味を持つのは Integer のみ)
///
/// 未知キーの値読み飛ばしでも型を識別する必要があるため、サポート対象型を
/// 列挙して保持する。Inner List は RFC 9651 §3.1 で Member Value として
/// 許容されるが、本実装では `_` で受けて読み飛ばす。
#[derive(Debug, Clone, Copy)]
enum SfValue {
    /// Integer (RFC 9651 Section 3.3.1)
    Integer(i64),
    /// Decimal (RFC 9651 Section 3.3.2)
    Decimal,
    /// String (RFC 9651 Section 3.3.3)
    String,
    /// Token (RFC 9651 Section 3.3.4)
    Token,
    /// Byte Sequence (RFC 9651 Section 3.3.5)
    ByteSequence,
    /// Boolean (RFC 9651 Section 3.3.6)
    Boolean,
    /// RFC 9651 Section 3.3.7 / Section 4.2.9: Date (`@` + Integer)
    Date,
    /// RFC 9651 Section 3.3.8 / Section 4.2.10: Display String (`%` + DQUOTE + ...)
    DisplayString,
    /// Inner List (RFC 9651 Section 3.1.1)
    InnerList,
}

struct DictionaryParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> DictionaryParser<'a> {
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

    /// RFC 9651 Section 4.2.3.3: Parsing a Key
    ///
    /// RFC 9651 Appendix C (非規範) の ABNF:
    /// `key = ( lcalpha / "*" ) *( lcalpha / DIGIT / "_" / "-" / "." / "*" )`
    fn parse_key(&mut self) -> Result<Vec<u8>, WtError> {
        match self.peek() {
            Some(b) if b.is_ascii_lowercase() || b == b'*' => {}
            _ => {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: dictionary key must start with lcalpha or '*'",
                ));
            }
        }
        let start = self.pos;
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
        Ok(self.input[start..self.pos].to_vec())
    }

    /// RFC 9651 Section 4.2.1.1: Parsing an Item or Inner List
    fn parse_value(&mut self) -> Result<SfValue, WtError> {
        match self.peek() {
            Some(b'(') => self.parse_inner_list(),
            _ => self.parse_bare_item(),
        }
    }

    /// RFC 9651 Section 4.2.3.1: Parsing a Bare Item
    ///
    /// 先頭バイトで型を識別して bare item 末尾までを消費する。
    /// known キーへの値判定 (Integer かどうか) と、未知キーの値読み飛ばしの両方で使われる。
    fn parse_bare_item(&mut self) -> Result<SfValue, WtError> {
        match self.peek() {
            Some(b) if b == b'-' || b.is_ascii_digit() => match self.parse_number()? {
                SfNumber::Integer(n) => Ok(SfValue::Integer(n)),
                SfNumber::Decimal => Ok(SfValue::Decimal),
            },
            Some(b'"') => {
                self.skip_string()?;
                Ok(SfValue::String)
            }
            Some(b'?') => {
                self.skip_boolean()?;
                Ok(SfValue::Boolean)
            }
            Some(b':') => {
                self.skip_byte_sequence()?;
                Ok(SfValue::ByteSequence)
            }
            Some(b'@') => {
                self.skip_date()?;
                Ok(SfValue::Date)
            }
            Some(b'%') => {
                self.skip_display_string()?;
                Ok(SfValue::DisplayString)
            }
            Some(b) if b == b'*' || b.is_ascii_alphabetic() => {
                self.skip_token()?;
                Ok(SfValue::Token)
            }
            _ => Err(WtError::invalid_input(
                "WebTransport-Init: unexpected character at start of bare item",
            )),
        }
    }

    /// RFC 9651 Section 4.2.4: Parsing an Integer or Decimal
    ///
    /// Integer は `[ "-" ] 1*15DIGIT`、Decimal は `[ "-" ] 1*12DIGIT "." 1*3DIGIT`。
    /// Decimal は WtInit のキーには使われないが、未知キーの値として現れた場合に
    /// 正しく読み飛ばすため両方をハンドリングする。
    fn parse_number(&mut self) -> Result<SfNumber, WtError> {
        let start = self.pos;
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
        let int_digits = self.pos - digit_start;
        if int_digits == 0 {
            return Err(WtError::invalid_input(
                "WebTransport-Init: integer must contain at least one digit",
            ));
        }
        // RFC 9651 Section 4.2.4 step 7.3.1: Decimal なら "." の前で 12 桁まで
        if self.peek() == Some(b'.') {
            if int_digits > 12 {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: decimal integer part exceeds 12 digits",
                ));
            }
            self.advance();
            let frac_start = self.pos;
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            let frac_digits = self.pos - frac_start;
            if !(1..=3).contains(&frac_digits) {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: decimal fractional part must be 1-3 digits",
                ));
            }
            return Ok(SfNumber::Decimal);
        }
        // Integer: 15 桁制限 (RFC 9651 §3.3.1)
        if int_digits > 15 {
            return Err(WtError::invalid_input(
                "WebTransport-Init: integer exceeds 15 digits",
            ));
        }
        // 数値文字列を i64 に変換する
        // 桁数は 15 桁以下に制限済みのため、絶対値は 10^15-1 以下で i64 に収まる
        let text = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| WtError::invalid_input("WebTransport-Init: integer is not valid UTF-8"))?;
        let n: i64 = text
            .parse()
            .map_err(|_| WtError::invalid_input("WebTransport-Init: failed to parse integer"))?;
        Ok(SfNumber::Integer(n))
    }

    /// RFC 9651 Section 4.2.5: Parsing a String
    fn skip_string(&mut self) -> Result<(), WtError> {
        // 先頭の `"` を消費
        if !self.consume_if_eq(b'"') {
            return Err(WtError::invalid_input(
                "WebTransport-Init: string must start with '\"'",
            ));
        }
        while let Some(b) = self.peek() {
            if b == b'\\' {
                // RFC 9651 Section 4.2.5: エスケープは `\\` または `\"`
                self.advance();
                match self.peek() {
                    Some(b'\\') | Some(b'"') => self.advance(),
                    _ => {
                        return Err(WtError::invalid_input(
                            "WebTransport-Init: invalid escape sequence in string",
                        ));
                    }
                }
            } else if b == b'"' {
                self.advance();
                return Ok(());
            } else if !(0x20..=0x7E).contains(&b) {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: invalid character in string",
                ));
            } else {
                self.advance();
            }
        }
        Err(WtError::invalid_input(
            "WebTransport-Init: unterminated string",
        ))
    }

    /// RFC 9651 Section 4.2.6: Parsing a Token
    ///
    /// `( ALPHA / "*" ) *( tchar / ":" / "/" )`
    fn skip_token(&mut self) -> Result<(), WtError> {
        match self.peek() {
            Some(b) if b == b'*' || b.is_ascii_alphabetic() => self.advance(),
            _ => {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: token must start with ALPHA or '*'",
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

    /// RFC 9651 Section 4.2.7: Parsing a Byte Sequence
    ///
    /// `":" *base64 ":"` (base64 文字: ALPHA / DIGIT / "+" / "/" / "=")
    fn skip_byte_sequence(&mut self) -> Result<(), WtError> {
        if !self.consume_if_eq(b':') {
            return Err(WtError::invalid_input(
                "WebTransport-Init: byte sequence must start with ':'",
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
                "WebTransport-Init: byte sequence must end with ':'",
            ));
        }
        Ok(())
    }

    /// RFC 9651 Section 4.2.8: Parsing a Boolean
    ///
    /// `"?" ( "0" / "1" )`
    fn skip_boolean(&mut self) -> Result<(), WtError> {
        if !self.consume_if_eq(b'?') {
            return Err(WtError::invalid_input(
                "WebTransport-Init: boolean must start with '?'",
            ));
        }
        match self.peek() {
            Some(b'0') | Some(b'1') => self.advance(),
            _ => {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: boolean value must be '0' or '1'",
                ));
            }
        }
        Ok(())
    }

    /// RFC 9651 Section 4.2.9: Parsing a Date
    ///
    /// `"@" Integer` の形式で、Decimal は許容されない。
    /// 整数の桁数・範囲は RFC 9651 Section 4.2.4 / Section 3.3.1 と同じ。
    fn skip_date(&mut self) -> Result<(), WtError> {
        // 呼び出し元 (`parse_bare_item`) が先頭の '@' を確認済みのため、ここで消費する
        debug_assert_eq!(self.peek(), Some(b'@'));
        self.advance();
        match self.parse_number()? {
            SfNumber::Integer(_) => Ok(()),
            SfNumber::Decimal => Err(WtError::invalid_input(
                "WebTransport-Init: date must not be a decimal",
            )),
        }
    }

    /// RFC 9651 Section 4.2.10: Parsing a Display String
    ///
    /// `"%" DQUOTE` で始まり、DQUOTE で終わる。`%` に続く 2 文字は 0-9 または
    /// 小文字 a-f の 16 進数でなければならない。未エンコード文字は SP と VCHAR
    /// (%x20-7e) に限られ、復号後のバイト列は UTF-8 として妥当でなければならない。
    fn skip_display_string(&mut self) -> Result<(), WtError> {
        // 呼び出し元 (`parse_bare_item`) が先頭の '%' を確認済みのため、ここで消費する
        debug_assert_eq!(self.peek(), Some(b'%'));
        self.advance();
        if !self.consume_if_eq(b'"') {
            return Err(WtError::invalid_input(
                "WebTransport-Init: display string must start with percent-DQUOTE",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(b) = self.peek() {
            self.advance();
            // SP と VCHAR 以外 (制御文字・非 ASCII) は不正
            if !(0x20..=0x7E).contains(&b) {
                return Err(WtError::invalid_input(
                    "WebTransport-Init: invalid character in display string",
                ));
            }
            if b == b'%' {
                let high = self.next_hex_digit()?;
                let low = self.next_hex_digit()?;
                bytes.push((high << 4) | low);
            } else if b == b'"' {
                std::str::from_utf8(&bytes).map_err(|_| {
                    WtError::invalid_input("WebTransport-Init: display string is not valid UTF-8")
                })?;
                return Ok(());
            } else {
                bytes.push(b);
            }
        }
        Err(WtError::invalid_input(
            "WebTransport-Init: unterminated display string",
        ))
    }

    /// Display String の percent encoding から 16 進数 1 桁を読み取る
    ///
    /// RFC 9651 Section 4.2.10 step 4.3.2: 16 進英字は小文字のみ許容する。
    fn next_hex_digit(&mut self) -> Result<u8, WtError> {
        match self.peek() {
            Some(b @ b'0'..=b'9') => {
                self.advance();
                Ok(b - b'0')
            }
            Some(b @ b'a'..=b'f') => {
                self.advance();
                Ok(b - b'a' + 10)
            }
            None => Err(WtError::invalid_input(
                "WebTransport-Init: percent encoding must contain two lowercase hexadecimal digits",
            )),
            Some(_) => Err(WtError::invalid_input(
                "WebTransport-Init: percent encoding must use lowercase hexadecimal digits",
            )),
        }
    }

    /// RFC 9651 Section 4.2.1.2: Parsing an Inner List
    ///
    /// RFC 9651 Appendix C (非規範) の ABNF:
    /// `inner-list = "(" *SP [ sf-item *( 1*SP sf-item ) *SP ] ")" parameters`。
    /// 未知キーの値として現れた場合のみ呼ばれるため、内側の sf-item は読み飛ばすだけでよい。
    /// Inner List 自体に付くパラメータ列は呼び出し元 `parse_value` 直後の `skip_parameters`
    /// で処理される。
    fn parse_inner_list(&mut self) -> Result<SfValue, WtError> {
        if !self.consume_if_eq(b'(') {
            return Err(WtError::invalid_input(
                "WebTransport-Init: inner list must start with '('",
            ));
        }
        loop {
            // 先頭の SP を破棄
            while self.consume_if_eq(b' ') {}
            if self.consume_if_eq(b')') {
                return Ok(SfValue::InnerList);
            }
            // Inner List 内の項目は bare item のみ (Inner List 入れ子は許容されない)
            let _ = self.parse_bare_item()?;
            // 項目に付くパラメータも消費
            self.skip_parameters()?;
            // 区切りはスペース or 終端
            match self.peek() {
                Some(b' ') => {} // 次ループ先頭で消費される
                Some(b')') => {
                    self.advance();
                    return Ok(SfValue::InnerList);
                }
                _ => {
                    return Err(WtError::invalid_input(
                        "WebTransport-Init: invalid character in inner list",
                    ));
                }
            }
        }
    }

    /// RFC 9651 Section 4.2.3.2: Parsing Parameters
    ///
    /// `*( ";" *SP parameter )` を全て読み飛ばす。`parameter = key [ "=" bare-item ]`。
    fn skip_parameters(&mut self) -> Result<(), WtError> {
        while self.peek() == Some(b';') {
            self.advance();
            while self.consume_if_eq(b' ') {}
            let _ = self.parse_key()?;
            if self.consume_if_eq(b'=') {
                let _ = self.parse_bare_item()?;
            }
        }
        Ok(())
    }
}

use super::is_tchar;
