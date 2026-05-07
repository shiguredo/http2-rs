//! QUIC 形式の可変長整数エンコーディング (RFC 9000 Section 16)
//!
//! 先頭 2 ビットでエンコード長を示す:
//!
//! | 2MSB | Length | Usable Bits | Range                    |
//! |------|--------|-------------|--------------------------|
//! | 00   | 1      | 6           | 0-63                     |
//! | 01   | 2      | 14          | 0-16383                  |
//! | 10   | 4      | 30          | 0-1073741823             |
//! | 11   | 8      | 62          | 0-4611686018427387903    |

use crate::webtransport::error::{WtError, WtErrorKind, WtResult};

/// 可変長整数の最大値 (2^62 - 1)
pub const MAX_VALUE: u64 = 4_611_686_018_427_387_903;

/// 1 バイトでエンコード可能な最大値
const MAX_1_BYTE: u64 = 63;

/// 2 バイトでエンコード可能な最大値
const MAX_2_BYTES: u64 = 16_383;

/// 4 バイトでエンコード可能な最大値
const MAX_4_BYTES: u64 = 1_073_741_823;

/// 値をエンコードするのに必要なバイト数を返す
///
/// # Panics
///
/// `value` が `MAX_VALUE` を超える場合はパニックする。
#[must_use]
pub const fn encoded_len(value: u64) -> usize {
    if value <= MAX_1_BYTE {
        1
    } else if value <= MAX_2_BYTES {
        2
    } else if value <= MAX_4_BYTES {
        4
    } else {
        8
    }
}

/// 可変長整数をエンコードする
///
/// # 引数
///
/// - `value`: エンコードする値
/// - `buf`: 出力バッファ
///
/// # 戻り値
///
/// 成功時は書き込んだバイト数を返す。
///
/// # エラー
///
/// - `value` が `MAX_VALUE` を超える場合
/// - `buf` のサイズが不足している場合
#[track_caller]
pub fn encode(value: u64, buf: &mut [u8]) -> WtResult<usize> {
    if value > MAX_VALUE {
        return Err(WtError::with_reason(
            WtErrorKind::InvalidInput,
            format!("varint value {value} exceeds maximum {MAX_VALUE}"),
        ));
    }

    let len = encoded_len(value);
    if buf.len() < len {
        return Err(WtError::with_reason(
            WtErrorKind::BufferTooShort,
            format!("need {len} bytes, got {}", buf.len()),
        ));
    }

    match len {
        1 => {
            buf[0] = value as u8;
        }
        2 => {
            buf[0] = 0x40 | ((value >> 8) as u8);
            buf[1] = value as u8;
        }
        4 => {
            buf[0] = 0x80 | ((value >> 24) as u8);
            buf[1] = (value >> 16) as u8;
            buf[2] = (value >> 8) as u8;
            buf[3] = value as u8;
        }
        8 => {
            buf[0] = 0xc0 | ((value >> 56) as u8);
            buf[1] = (value >> 48) as u8;
            buf[2] = (value >> 40) as u8;
            buf[3] = (value >> 32) as u8;
            buf[4] = (value >> 24) as u8;
            buf[5] = (value >> 16) as u8;
            buf[6] = (value >> 8) as u8;
            buf[7] = value as u8;
        }
        _ => unreachable!(),
    }

    Ok(len)
}

/// 可変長整数をデコードする
///
/// # 引数
///
/// - `buf`: 入力バッファ
///
/// # 戻り値
///
/// 成功時は `(値, 消費バイト数)` を返す。
///
/// # エラー
///
/// - 入力データが不足している場合 (`Incomplete`)
/// - 非最小エンコーディングの場合 (`InvalidInput`)
///   - RFC 9000 Section 16: 値は最小バイト数でエンコードされなければならない
#[track_caller]
pub fn decode(buf: &[u8]) -> WtResult<(u64, usize)> {
    if buf.is_empty() {
        return Err(WtError::new(WtErrorKind::Incomplete));
    }

    let first = buf[0];
    let prefix = first >> 6;

    let (value, len) = match prefix {
        0 => {
            // 1 バイト
            (u64::from(first & 0x3f), 1)
        }
        1 => {
            // 2 バイト
            if buf.len() < 2 {
                return Err(WtError::new(WtErrorKind::Incomplete));
            }
            let value = (u64::from(first & 0x3f) << 8) | u64::from(buf[1]);
            (value, 2)
        }
        2 => {
            // 4 バイト
            if buf.len() < 4 {
                return Err(WtError::new(WtErrorKind::Incomplete));
            }
            let value = (u64::from(first & 0x3f) << 24)
                | (u64::from(buf[1]) << 16)
                | (u64::from(buf[2]) << 8)
                | u64::from(buf[3]);
            (value, 4)
        }
        3 => {
            // 8 バイト
            if buf.len() < 8 {
                return Err(WtError::new(WtErrorKind::Incomplete));
            }
            let value = (u64::from(first & 0x3f) << 56)
                | (u64::from(buf[1]) << 48)
                | (u64::from(buf[2]) << 40)
                | (u64::from(buf[3]) << 32)
                | (u64::from(buf[4]) << 24)
                | (u64::from(buf[5]) << 16)
                | (u64::from(buf[6]) << 8)
                | u64::from(buf[7]);
            (value, 8)
        }
        _ => unreachable!(),
    };

    // RFC 9000 Section 16: 非最小エンコーディングを拒否する
    // "A variable-length integer MUST use the minimum number of bytes required to encode the value."
    if encoded_len(value) != len {
        return Err(WtError::with_reason(
            WtErrorKind::InvalidInput,
            format!(
                "non-minimal varint encoding: value {value} encoded in {len} bytes, minimum is {}",
                encoded_len(value)
            ),
        ));
    }

    Ok((value, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoded_len() {
        assert_eq!(encoded_len(0), 1);
        assert_eq!(encoded_len(63), 1);
        assert_eq!(encoded_len(64), 2);
        assert_eq!(encoded_len(16383), 2);
        assert_eq!(encoded_len(16384), 4);
        assert_eq!(encoded_len(1073741823), 4);
        assert_eq!(encoded_len(1073741824), 8);
        assert_eq!(encoded_len(MAX_VALUE), 8);
    }

    #[test]
    fn test_encode_decode_1_byte() {
        let mut buf = [0u8; 8];

        for value in [0, 1, 37, 63] {
            let len = encode(value, &mut buf).unwrap();
            assert_eq!(len, 1);

            let (decoded, consumed) = decode(&buf[..len]).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(consumed, 1);
        }
    }

    #[test]
    fn test_encode_decode_2_bytes() {
        let mut buf = [0u8; 8];

        for value in [64, 100, 494, 16383] {
            let len = encode(value, &mut buf).unwrap();
            assert_eq!(len, 2);

            let (decoded, consumed) = decode(&buf[..len]).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(consumed, 2);
        }
    }

    #[test]
    fn test_encode_decode_4_bytes() {
        let mut buf = [0u8; 8];

        for value in [16384, 65535, 494878333, 1073741823] {
            let len = encode(value, &mut buf).unwrap();
            assert_eq!(len, 4);

            let (decoded, consumed) = decode(&buf[..len]).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(consumed, 4);
        }
    }

    #[test]
    fn test_encode_decode_8_bytes() {
        let mut buf = [0u8; 8];

        for value in [1073741824, 151288809941952652, MAX_VALUE] {
            let len = encode(value, &mut buf).unwrap();
            assert_eq!(len, 8);

            let (decoded, consumed) = decode(&buf[..len]).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(consumed, 8);
        }
    }

    #[test]
    fn test_encode_overflow() {
        let mut buf = [0u8; 8];
        let result = encode(MAX_VALUE + 1, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_buffer_too_short() {
        let mut buf = [0u8; 1];
        let result = encode(16384, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_incomplete() {
        // 空バッファ
        assert!(decode(&[]).is_err());

        // 2 バイトが必要だが 1 バイトしかない
        assert!(decode(&[0x40]).is_err());

        // 4 バイトが必要だが 3 バイトしかない
        assert!(decode(&[0x80, 0x00, 0x00]).is_err());

        // 8 バイトが必要だが 7 バイトしかない
        assert!(decode(&[0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).is_err());
    }

    #[test]
    fn test_decode_non_minimal_encoding() {
        // RFC 9000 Section 16: 非最小エンコーディングは拒否される

        // 値 10 を 2 バイトでエンコード (最小は 1 バイト)
        // 0x40 | (10 >> 8) = 0x40, buf[1] = 10
        assert!(decode(&[0x40, 0x0a]).is_err());

        // 値 0 を 2 バイトでエンコード (最小は 1 バイト)
        assert!(decode(&[0x40, 0x00]).is_err());

        // 値 63 を 2 バイトでエンコード (最小は 1 バイト)
        assert!(decode(&[0x40, 0x3f]).is_err());

        // 値 64 を 4 バイトでエンコード (最小は 2 バイト)
        assert!(decode(&[0x80, 0x00, 0x00, 0x40]).is_err());

        // 値 16383 を 4 バイトでエンコード (最小は 2 バイト)
        assert!(decode(&[0x80, 0x00, 0x3f, 0xff]).is_err());

        // 値 16384 を 8 バイトでエンコード (最小は 4 バイト)
        assert!(decode(&[0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00]).is_err());
    }

    #[test]
    fn test_rfc_examples() {
        // RFC 9000 Appendix A.1 のサンプル値
        let mut buf = [0u8; 8];

        // 37 -> 0x25 (1 byte)
        let len = encode(37, &mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x25]);

        // 15293 -> 0x7bbd (2 bytes)
        let len = encode(15293, &mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x7b, 0xbd]);

        // 494878333 -> 0x9d7f3e7d (4 bytes)
        let len = encode(494878333, &mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x9d, 0x7f, 0x3e, 0x7d]);

        // 151288809941952652 -> 0xc2197c5eff14e88c (8 bytes)
        let len = encode(151288809941952652, &mut buf).unwrap();
        assert_eq!(
            &buf[..len],
            &[0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c]
        );
    }
}
