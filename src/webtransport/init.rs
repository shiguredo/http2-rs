//! WebTransport-Init ヘッダーフィールドのパーサー
//!
//! draft-ietf-webtrans-http2-15 Section 4.3.2 (L519-L541) に定義されている
//! `WebTransport-Init` HTTP ヘッダーフィールドをパースする。
//! 実体は RFC 8941 (Structured Field Values for HTTP) Dictionary。
//!
//! 本モジュールは known キー (`u` / `bl` / `br`) のみ抽出する必要最小限の
//! Dictionary パーサーを提供する。仕様の MUST 要件:
//! - 未知キーとパラメータは MUST 無視する (Section 4.3.2 L541)
//! - パース不能・型不一致・値範囲外は MUST 4xx で拒否する (Section 4.3.2 L525-L526, L539-L540)
//! - 重複キーは RFC 8941 Section 4.2.2 の規則で last-wins
//!
//! 本実装が参照する仕様は IETF draft (`-14`) であり、draft の改訂や RFC 化に
//! 伴って章番号・行番号・要求項目が変わりうる。コメント内の `L<番号>` 参照は
//! 現行 draft 時点のもの。

use crate::webtransport::error::WtError;

/// WebTransport-Init ヘッダーフィールドの値
///
/// draft-ietf-webtrans-http2-15 Section 4.3.2 (L527-L537) で定義された
/// 3 つの Integer キーを optional に保持する。
///
/// `None` はキーがヘッダーに含まれていなかったことを示し、
/// SETTINGS 由来の値とマージする際に上書き対象外として扱う。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WtInit {
    /// `u`: ピア (=送信者から見た recipient) が開く単方向ストリームの初期最大データ量
    pub u: Option<u64>,
    /// `bl`: 送信者が開く双方向ストリームの初期最大データ量
    pub bl: Option<u64>,
    /// `br`: ピア (=送信者から見た recipient) が開く双方向ストリームの初期最大データ量
    pub br: Option<u64>,
}

/// RFC 8941 Section 3.3.1: Integer の絶対値上限 (15 桁の十進数)
const SF_INTEGER_MAX: u64 = 999_999_999_999_999;

impl WtInit {
    /// HTTP ヘッダー値バイト列をパースする
    ///
    /// 入力は HPACK でデコードされたヘッダー値そのまま (RFC 8941 Section 4.2 step 1:
    /// ASCII 変換に失敗した入力はパース失敗)。RFC 8941 のパース手順を必要最小限で実装する。
    pub fn parse(value: &[u8]) -> Result<Self, WtError> {
        // RFC 8941 Section 4.2 step 1: 入力は ASCII であること
        if value.iter().any(|&b| b >= 0x80) {
            return Err(WtError::invalid_input(
                "WebTransport-Init: non-ASCII byte in header value",
            ));
        }

        let mut parser = DictionaryParser::new(value);
        let mut out = WtInit::default();

        // RFC 8941 Section 4.2 step 2 は先頭の SP のみ破棄を規定する。
        // 本実装は OWS (SP / HTAB) まで許容する独自緩和
        parser.skip_ows();

        while !parser.is_empty() {
            // RFC 8941 Section 4.2.2 step 2.1: Parse a Key
            let key = parser.parse_key()?;

            // RFC 8941 Section 4.2.2 step 2.2/2.3: "=" の有無で値を決める
            let value = if parser.consume_if_eq(b'=') {
                // Parse a Bare Item or Inner List
                parser.parse_value()?
            } else {
                // "=" 省略時は Boolean true (本実装は値そのものを使わないのでタグだけ立てる)
                SfValue::Boolean
            };

            // RFC 8941 Section 3.1.2: Parameters を消費 (本実装ではすべて無視)
            parser.skip_parameters()?;

            // known キーのみ WtInit に格納する
            // (RFC 8941 §4.2.2 step 2.4 は last-wins、未知キーは Section 4.3.2 L541 で無視)
            match key.as_slice() {
                b"u" => out.u = Some(extract_nonneg_integer(&value, "u")?),
                b"bl" => out.bl = Some(extract_nonneg_integer(&value, "bl")?),
                b"br" => out.br = Some(extract_nonneg_integer(&value, "br")?),
                _ => {} // 未知キーは値の型に関わらず無視する
            }

            // RFC 8941 Section 4.2.2 step 2.6: trailing OWS を破棄
            parser.skip_ows();

            // RFC 8941 Section 4.2.2 step 2.7: 入力末尾なら成功
            if parser.is_empty() {
                return Ok(out);
            }

            // RFC 8941 Section 4.2.2 step 2.8-2.10: "," を必須消費、trailing comma は不可
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
/// 仕様 (Section 4.3.2 L527-L537) は「Integer」とのみ規定。フロー制御の初期値として
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

/// RFC 8941 の Bare Item (`u`/`bl`/`br` で実際に意味を持つのは Integer のみ)
///
/// 未知キーの値読み飛ばしでも型を識別する必要があるため、サポート対象型を
/// 列挙して保持する。Inner List は RFC 8941 §3.1 で Member Value として
/// 許容されるが、本実装では `_` で受けて読み飛ばす。
enum SfValue {
    Integer(i64),
    Decimal,
    String,
    Token,
    ByteSequence,
    Boolean,
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

    /// RFC 8941 Section 4.2.3.3: Parsing a Key
    ///
    /// RFC 8941 §3.1.2 の ABNF: `key = ( lcalpha / "*" ) *( lcalpha / DIGIT / "_" / "-" / "." / "*" )`
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

    /// RFC 8941 Section 4.2.1.1: Parsing an Item or Inner List
    fn parse_value(&mut self) -> Result<SfValue, WtError> {
        match self.peek() {
            Some(b'(') => self.parse_inner_list(),
            _ => self.parse_bare_item(),
        }
    }

    /// RFC 8941 Section 4.2.3.1: Parsing a Bare Item
    ///
    /// 先頭バイトで型を識別して bare item 末尾までを消費する。
    /// known キーへの値判定 (Integer かどうか) と、未知キーの値読み飛ばしの両方で使われる。
    fn parse_bare_item(&mut self) -> Result<SfValue, WtError> {
        match self.peek() {
            Some(b) if b == b'-' || b.is_ascii_digit() => self.parse_number(),
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
            Some(b) if b == b'*' || b.is_ascii_alphabetic() => {
                self.skip_token()?;
                Ok(SfValue::Token)
            }
            _ => Err(WtError::invalid_input(
                "WebTransport-Init: unexpected character at start of bare item",
            )),
        }
    }

    /// RFC 8941 Section 4.2.4: Parsing an Integer or Decimal
    ///
    /// `[ "-" ] 1*15DIGIT [ "." 1*3DIGIT ]`。Integer は最大 15 桁、Decimal は整数部 12 桁 +
    /// 小数部 1-3 桁。Decimal は WtInit のキーには使われないが、未知キーの値として現れた
    /// 場合に正しく読み飛ばすため両方をハンドリングする。
    fn parse_number(&mut self) -> Result<SfValue, WtError> {
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
        // RFC 8941 Section 4.2.4 step 7: Decimal なら "." の前で 12 桁まで
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
            return Ok(SfValue::Decimal);
        }
        // Integer: 15 桁制限 (RFC 8941 §3.3.1)
        if int_digits > 15 {
            return Err(WtError::invalid_input(
                "WebTransport-Init: integer exceeds 15 digits",
            ));
        }
        // 数値文字列を i64 に変換する
        // 範囲は ±SF_INTEGER_MAX (= 10^15 - 1) で i64::MAX より十分小さい
        let text = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| WtError::invalid_input("WebTransport-Init: integer is not valid UTF-8"))?;
        let n: i64 = text
            .parse()
            .map_err(|_| WtError::invalid_input("WebTransport-Init: failed to parse integer"))?;
        if n.unsigned_abs() > SF_INTEGER_MAX {
            return Err(WtError::invalid_input(
                "WebTransport-Init: integer out of range",
            ));
        }
        Ok(SfValue::Integer(n))
    }

    /// RFC 8941 Section 4.2.5: Parsing a String
    fn skip_string(&mut self) -> Result<(), WtError> {
        // 先頭の `"` を消費
        if !self.consume_if_eq(b'"') {
            return Err(WtError::invalid_input(
                "WebTransport-Init: string must start with '\"'",
            ));
        }
        while let Some(b) = self.peek() {
            if b == b'\\' {
                // RFC 8941 Section 4.2.5: エスケープは `\\` または `\"`
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

    /// RFC 8941 Section 4.2.6: Parsing a Token
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

    /// RFC 8941 Section 4.2.7: Parsing a Byte Sequence
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

    /// RFC 8941 Section 4.2.8: Parsing a Boolean
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

    /// RFC 8941 Section 4.2.1.2: Parsing an Inner List
    ///
    /// RFC 8941 §3.1.1 の ABNF: `inner-list = "(" *SP [ sf-item *( 1*SP sf-item ) *SP ] ")" parameters`。
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

    /// RFC 8941 Section 4.2.3.2: Parsing Parameters
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

/// RFC 9110 Section 5.6.2: tchar (RFC 7230 §3.2.6 から定義は変わらず移管されている)
///
/// `"!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA`
fn is_tchar(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
    ) || b.is_ascii_alphanumeric()
}
