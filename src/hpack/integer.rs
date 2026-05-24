//! HPACK 整数エンコーディング (RFC 7541 Section 5.1)
//!
//! HPACK で使用される可変長整数のエンコード/デコードを提供する。

use crate::error::{Error, Result};

/// 整数をエンコードする
///
/// `prefix_bits` はプレフィックスのビット数（1-8）を指定する。
/// `prefix_value` はプレフィックスバイトの初期値（上位ビットに他の情報がある場合）。
///
/// 成功時はエンコードしたバイト数を返す。
///
/// # Errors
///
/// バッファが不足している場合は `Err` を返す。
pub fn encode(buf: &mut [u8], value: u64, prefix_bits: u8, prefix_value: u8) -> Result<usize> {
    if buf.is_empty() {
        return Err(Error::hpack_error("HPACK integer encode buffer too short"));
    }

    let max_prefix = (1u64 << prefix_bits) - 1;

    if value < max_prefix {
        buf[0] = prefix_value | (value as u8);
        return Ok(1);
    }

    buf[0] = prefix_value | (max_prefix as u8);
    let mut remaining = value - max_prefix;
    let mut offset = 1;

    while remaining >= 128 {
        if offset >= buf.len() {
            return Err(Error::hpack_error("HPACK integer encode buffer too short"));
        }
        buf[offset] = 0x80 | ((remaining & 0x7f) as u8);
        remaining >>= 7;
        offset += 1;
    }

    if offset >= buf.len() {
        return Err(Error::hpack_error("HPACK integer encode buffer too short"));
    }
    buf[offset] = remaining as u8;
    Ok(offset + 1)
}

/// 整数をエンコードした場合のバイト数を計算する
#[must_use]
pub fn encoded_len(value: u64, prefix_bits: u8) -> usize {
    let max_prefix = (1u64 << prefix_bits) - 1;

    if value < max_prefix {
        return 1;
    }

    let mut remaining = value - max_prefix;
    let mut len = 1;

    while remaining >= 128 {
        remaining >>= 7;
        len += 1;
    }

    len + 1
}

/// 整数をデコードする
///
/// `prefix_bits` はプレフィックスのビット数（1-8）を指定する。
///
/// 成功時は `(デコードした値, 消費したバイト数)` を返す。
///
/// # Errors
///
/// - データが不足している場合は `HpackError` を返す。
/// - オーバーフローが発生した場合は `HpackError` を返す。
pub fn decode(buf: &[u8], prefix_bits: u8) -> Result<(u64, usize)> {
    if buf.is_empty() {
        return Err(Error::hpack_error("incomplete HPACK integer"));
    }

    let max_prefix = (1u64 << prefix_bits) - 1;
    let prefix_mask = max_prefix as u8;

    let first_byte = buf[0] & prefix_mask;
    if u64::from(first_byte) < max_prefix {
        return Ok((u64::from(first_byte), 1));
    }

    let mut value = max_prefix;
    let mut shift = 0u32;
    let mut offset = 1;

    loop {
        if offset >= buf.len() {
            return Err(Error::hpack_error("incomplete HPACK integer"));
        }

        let byte = buf[offset];
        offset += 1;

        // RFC 7541 §5.1: HPACK 整数のオーバーフロー
        if shift >= 63 {
            return Err(Error::hpack_error("HPACK integer overflow"));
        }

        let contribution = u64::from(byte & 0x7f);
        value = value
            .checked_add(contribution << shift)
            .ok_or_else(|| Error::hpack_error("HPACK integer overflow"))?;

        if byte & 0x80 == 0 {
            break;
        }

        shift += 7;
    }

    Ok((value, offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_small_value() {
        let mut buf = [0u8; 8];
        let len = encode(&mut buf, 10, 5, 0).unwrap();
        assert_eq!(len, 1);
        assert_eq!(buf[0], 10);
    }

    #[test]
    fn test_encode_max_prefix() {
        let mut buf = [0u8; 8];
        let len = encode(&mut buf, 31, 5, 0).unwrap();
        assert_eq!(len, 2);
        assert_eq!(buf[0], 31);
        assert_eq!(buf[1], 0);
    }

    #[test]
    fn test_encode_large_value() {
        // RFC 7541 Section 5.1 の例: 1337 を 5 ビットプレフィックスでエンコード
        let mut buf = [0u8; 8];
        let len = encode(&mut buf, 1337, 5, 0).unwrap();
        assert_eq!(len, 3);
        assert_eq!(buf[0], 31);
        assert_eq!(buf[1], 154);
        assert_eq!(buf[2], 10);
    }

    #[test]
    fn test_decode_small_value() {
        let buf = [10u8];
        let (value, len) = decode(&buf, 5).unwrap();
        assert_eq!(value, 10);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_decode_large_value() {
        // RFC 7541 Section 5.1 の例: 1337
        let buf = [31u8, 154, 10];
        let (value, len) = decode(&buf, 5).unwrap();
        assert_eq!(value, 1337);
        assert_eq!(len, 3);
    }

    #[test]
    fn test_roundtrip() {
        for value in [0, 1, 30, 31, 127, 128, 1337, 65535, 1_000_000] {
            for prefix_bits in 1..=8 {
                let mut buf = [0u8; 16];
                let encoded_len = encode(&mut buf, value, prefix_bits, 0).unwrap();
                let (decoded, decoded_len) = decode(&buf, prefix_bits).unwrap();
                assert_eq!(value, decoded);
                assert_eq!(encoded_len, decoded_len);
            }
        }
    }
}
