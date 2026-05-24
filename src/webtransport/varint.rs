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

/// 可変長整数を `Vec<u8>` にエンコードする
///
/// # 引数
///
/// - `value`: エンコードする値
///
/// # 戻り値
///
/// エンコードされたバイト列を返す。
///
/// # エラー
///
/// - `value` が `MAX_VALUE` を超える場合
#[track_caller]
pub fn encode_to_vec(value: u64) -> WtResult<Vec<u8>> {
    let mut buf = vec![0u8; encoded_len(value)];
    encode(value, &mut buf)?;
    Ok(buf)
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
