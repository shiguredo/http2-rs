//! Capsule Protocol (RFC 9297 + draft-ietf-webtrans-http2-13)
//!
//! # Capsule フォーマット (RFC 9297 Section 3.2)
//!
//! ```text
//! Capsule {
//!   Capsule Type (i),    // 可変長整数
//!   Capsule Length (i),  // 可変長整数 (Payload のバイト数)
//!   Capsule Value (..),  // Payload
//! }
//! ```

use crate::webtransport::error::{WtError, WtErrorKind, WtResult};
use crate::webtransport::varint;

/// Capsule タイプ定数 (RFC 9297 + draft-ietf-webtrans-http2-13)
pub mod capsule_type {
    /// DATAGRAM (RFC 9297 Section 3.5)
    pub const DATAGRAM: u64 = 0x00;

    /// PADDING (draft-ietf-webtrans-http2-13 Section 6.1)
    pub const PADDING: u64 = 0x190B4D38;

    /// WT_RESET_STREAM (draft-ietf-webtrans-http2-13 Section 6.2)
    pub const WT_RESET_STREAM: u64 = 0x190B4D39;

    /// WT_STOP_SENDING (draft-ietf-webtrans-http2-13 Section 6.3)
    pub const WT_STOP_SENDING: u64 = 0x190B4D3A;

    /// WT_STREAM (FIN=0) (draft-ietf-webtrans-http2-13 Section 6.4)
    pub const WT_STREAM: u64 = 0x190B4D3B;

    /// WT_STREAM (FIN=1) (draft-ietf-webtrans-http2-13 Section 6.4)
    pub const WT_STREAM_FIN: u64 = 0x190B4D3C;

    /// WT_MAX_DATA (draft-ietf-webtrans-http2-13 Section 6.5)
    pub const WT_MAX_DATA: u64 = 0x190B4D3D;

    /// WT_MAX_STREAM_DATA (draft-ietf-webtrans-http2-13 Section 6.6)
    pub const WT_MAX_STREAM_DATA: u64 = 0x190B4D3E;

    /// WT_MAX_STREAMS (bidirectional) (draft-ietf-webtrans-http2-13 Section 6.7)
    pub const WT_MAX_STREAMS_BIDI: u64 = 0x190B4D3F;

    /// WT_MAX_STREAMS (unidirectional) (draft-ietf-webtrans-http2-13 Section 6.7)
    pub const WT_MAX_STREAMS_UNI: u64 = 0x190B4D40;

    /// WT_DATA_BLOCKED (draft-ietf-webtrans-http2-13 Section 6.8)
    pub const WT_DATA_BLOCKED: u64 = 0x190B4D41;

    /// WT_STREAM_DATA_BLOCKED (draft-ietf-webtrans-http2-13 Section 6.9)
    pub const WT_STREAM_DATA_BLOCKED: u64 = 0x190B4D42;

    /// WT_STREAMS_BLOCKED (bidirectional) (draft-ietf-webtrans-http2-13 Section 6.10)
    pub const WT_STREAMS_BLOCKED_BIDI: u64 = 0x190B4D43;

    /// WT_STREAMS_BLOCKED (unidirectional) (draft-ietf-webtrans-http2-13 Section 6.10)
    pub const WT_STREAMS_BLOCKED_UNI: u64 = 0x190B4D44;

    /// WT_CLOSE_SESSION (draft-ietf-webtrans-http2-13 Section 6.12)
    pub const WT_CLOSE_SESSION: u64 = 0x2843;

    /// WT_DRAIN_SESSION (draft-ietf-webtrans-http2-13 Section 6.13)
    pub const WT_DRAIN_SESSION: u64 = 0x78AE;
}

/// WT_CLOSE_SESSION のメッセージ最大長
const MAX_CLOSE_REASON_LEN: usize = 1024;

/// Capsule 構造体 (draft-ietf-webtrans-http2-13 Section 6)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capsule {
    /// DATAGRAM (RFC 9297 Section 3.5)
    Datagram { data: Vec<u8> },

    /// PADDING (Section 6.1)
    Padding { length: usize },

    /// WT_RESET_STREAM (Section 6.2)
    WtResetStream {
        stream_id: u64,
        error_code: u64,
        reliable_size: u64,
    },

    /// WT_STOP_SENDING (Section 6.3)
    WtStopSending { stream_id: u64, error_code: u64 },

    /// WT_STREAM (Section 6.4)
    WtStream {
        stream_id: u64,
        data: Vec<u8>,
        fin: bool,
    },

    /// WT_MAX_DATA (Section 6.5)
    WtMaxData { maximum: u64 },

    /// WT_MAX_STREAM_DATA (Section 6.6)
    WtMaxStreamData { stream_id: u64, maximum: u64 },

    /// WT_MAX_STREAMS (Section 6.7)
    WtMaxStreams { maximum: u64, bidirectional: bool },

    /// WT_DATA_BLOCKED (Section 6.8)
    WtDataBlocked { maximum: u64 },

    /// WT_STREAM_DATA_BLOCKED (Section 6.9)
    WtStreamDataBlocked { stream_id: u64, maximum: u64 },

    /// WT_STREAMS_BLOCKED (Section 6.10)
    WtStreamsBlocked { maximum: u64, bidirectional: bool },

    /// WT_CLOSE_SESSION (Section 6.12)
    /// Application Error Code: 32-bit, Message: UTF-8, max 1024 bytes
    WtCloseSession { error_code: u32, reason: String },

    /// WT_DRAIN_SESSION (Section 6.13, Length=0)
    WtDrainSession,

    /// 未知の Capsule タイプ (RFC 9297: MUST silently drop)
    Unknown { capsule_type: u64, data: Vec<u8> },
}

/// Capsule エンコーダー
#[derive(Debug, Default)]
pub struct CapsuleEncoder {
    buffer: Vec<u8>,
}

impl CapsuleEncoder {
    /// 新しいエンコーダーを生成する
    #[must_use]
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Capsule をエンコードする
    pub fn encode(&mut self, capsule: &Capsule) {
        match capsule {
            Capsule::Datagram { data } => {
                self.encode_header(capsule_type::DATAGRAM, data.len());
                self.buffer.extend_from_slice(data);
            }
            Capsule::Padding { length } => {
                self.encode_header(capsule_type::PADDING, *length);
                self.buffer.resize(self.buffer.len() + length, 0);
            }
            Capsule::WtResetStream {
                stream_id,
                error_code,
                reliable_size,
            } => {
                let payload_len = varint::encoded_len(*stream_id)
                    + varint::encoded_len(*error_code)
                    + varint::encoded_len(*reliable_size);
                self.encode_header(capsule_type::WT_RESET_STREAM, payload_len);
                self.encode_varint(*stream_id);
                self.encode_varint(*error_code);
                self.encode_varint(*reliable_size);
            }
            Capsule::WtStopSending {
                stream_id,
                error_code,
            } => {
                let payload_len =
                    varint::encoded_len(*stream_id) + varint::encoded_len(*error_code);
                self.encode_header(capsule_type::WT_STOP_SENDING, payload_len);
                self.encode_varint(*stream_id);
                self.encode_varint(*error_code);
            }
            Capsule::WtStream {
                stream_id,
                data,
                fin,
            } => {
                let capsule_type = if *fin {
                    capsule_type::WT_STREAM_FIN
                } else {
                    capsule_type::WT_STREAM
                };
                let payload_len = varint::encoded_len(*stream_id) + data.len();
                self.encode_header(capsule_type, payload_len);
                self.encode_varint(*stream_id);
                self.buffer.extend_from_slice(data);
            }
            Capsule::WtMaxData { maximum } => {
                let payload_len = varint::encoded_len(*maximum);
                self.encode_header(capsule_type::WT_MAX_DATA, payload_len);
                self.encode_varint(*maximum);
            }
            Capsule::WtMaxStreamData { stream_id, maximum } => {
                let payload_len = varint::encoded_len(*stream_id) + varint::encoded_len(*maximum);
                self.encode_header(capsule_type::WT_MAX_STREAM_DATA, payload_len);
                self.encode_varint(*stream_id);
                self.encode_varint(*maximum);
            }
            Capsule::WtMaxStreams {
                maximum,
                bidirectional,
            } => {
                let capsule_type = if *bidirectional {
                    capsule_type::WT_MAX_STREAMS_BIDI
                } else {
                    capsule_type::WT_MAX_STREAMS_UNI
                };
                let payload_len = varint::encoded_len(*maximum);
                self.encode_header(capsule_type, payload_len);
                self.encode_varint(*maximum);
            }
            Capsule::WtDataBlocked { maximum } => {
                let payload_len = varint::encoded_len(*maximum);
                self.encode_header(capsule_type::WT_DATA_BLOCKED, payload_len);
                self.encode_varint(*maximum);
            }
            Capsule::WtStreamDataBlocked { stream_id, maximum } => {
                let payload_len = varint::encoded_len(*stream_id) + varint::encoded_len(*maximum);
                self.encode_header(capsule_type::WT_STREAM_DATA_BLOCKED, payload_len);
                self.encode_varint(*stream_id);
                self.encode_varint(*maximum);
            }
            Capsule::WtStreamsBlocked {
                maximum,
                bidirectional,
            } => {
                let capsule_type = if *bidirectional {
                    capsule_type::WT_STREAMS_BLOCKED_BIDI
                } else {
                    capsule_type::WT_STREAMS_BLOCKED_UNI
                };
                let payload_len = varint::encoded_len(*maximum);
                self.encode_header(capsule_type, payload_len);
                self.encode_varint(*maximum);
            }
            Capsule::WtCloseSession { error_code, reason } => {
                // reason は最大 1024 バイト
                let reason_bytes = reason.as_bytes();
                let reason_len = reason_bytes.len().min(MAX_CLOSE_REASON_LEN);
                let payload_len = 4 + reason_len; // 32-bit error code + reason
                self.encode_header(capsule_type::WT_CLOSE_SESSION, payload_len);
                self.buffer.extend_from_slice(&error_code.to_be_bytes());
                self.buffer.extend_from_slice(&reason_bytes[..reason_len]);
            }
            Capsule::WtDrainSession => {
                self.encode_header(capsule_type::WT_DRAIN_SESSION, 0);
            }
            Capsule::Unknown { capsule_type, data } => {
                self.encode_header(*capsule_type, data.len());
                self.buffer.extend_from_slice(data);
            }
        }
    }

    /// ヘッダー (Type + Length) をエンコードする
    fn encode_header(&mut self, capsule_type: u64, length: usize) {
        self.encode_varint(capsule_type);
        self.encode_varint(length as u64);
    }

    /// 可変長整数をエンコードする
    fn encode_varint(&mut self, value: u64) {
        let len = varint::encoded_len(value);
        let start = self.buffer.len();
        self.buffer.resize(start + len, 0);
        varint::encode(value, &mut self.buffer[start..])
            .expect("buffer is pre-sized to encoded_len");
    }

    /// バッファを取得してクリアする
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }

    /// バッファの参照を取得する
    #[must_use]
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// Capsule デコーダー (ストリーミング対応)
#[derive(Debug, Default)]
pub struct CapsuleDecoder {
    buffer: Vec<u8>,
}

impl CapsuleDecoder {
    /// 新しいデコーダーを生成する
    #[must_use]
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// データを追加する
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Capsule をデコードする
    ///
    /// # 戻り値
    ///
    /// - `Ok(Some(capsule))`: Capsule をデコードした
    /// - `Ok(None)`: データが不足している
    /// - `Err(_)`: デコードエラー
    pub fn decode(&mut self) -> WtResult<Option<Capsule>> {
        if self.buffer.is_empty() {
            return Ok(None);
        }

        let mut offset = 0;

        // Type をデコード
        let (capsule_type, type_len) = match varint::decode(&self.buffer[offset..]) {
            Ok(v) => v,
            Err(e) if e.kind == WtErrorKind::Incomplete => return Ok(None),
            Err(e) => return Err(e),
        };
        offset += type_len;

        // Length をデコード
        if offset >= self.buffer.len() {
            return Ok(None);
        }
        let (payload_len, len_len) = match varint::decode(&self.buffer[offset..]) {
            Ok(v) => v,
            Err(e) if e.kind == WtErrorKind::Incomplete => return Ok(None),
            Err(e) => return Err(e),
        };
        offset += len_len;

        // Payload が揃っているか確認
        let payload_len = payload_len as usize;
        if self.buffer.len() < offset + payload_len {
            return Ok(None);
        }

        let payload = &self.buffer[offset..offset + payload_len];
        let capsule = self.decode_payload(capsule_type, payload)?;

        // 消費したデータを削除
        let total_len = offset + payload_len;
        self.buffer.drain(..total_len);

        Ok(Some(capsule))
    }

    /// Payload をデコードする
    fn decode_payload(&self, capsule_type: u64, payload: &[u8]) -> WtResult<Capsule> {
        match capsule_type {
            capsule_type::DATAGRAM => Ok(Capsule::Datagram {
                data: payload.to_vec(),
            }),

            capsule_type::PADDING => Ok(Capsule::Padding {
                length: payload.len(),
            }),

            capsule_type::WT_RESET_STREAM => {
                let mut offset = 0;
                let (stream_id, len) = varint::decode(&payload[offset..])?;
                offset += len;
                let (error_code, len) = varint::decode(&payload[offset..])?;
                offset += len;
                let (reliable_size, len) = varint::decode(&payload[offset..])?;
                offset += len;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if offset != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_RESET_STREAM payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtResetStream {
                    stream_id,
                    error_code,
                    reliable_size,
                })
            }

            capsule_type::WT_STOP_SENDING => {
                let mut offset = 0;
                let (stream_id, len) = varint::decode(&payload[offset..])?;
                offset += len;
                let (error_code, len) = varint::decode(&payload[offset..])?;
                offset += len;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if offset != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_STOP_SENDING payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtStopSending {
                    stream_id,
                    error_code,
                })
            }

            capsule_type::WT_STREAM | capsule_type::WT_STREAM_FIN => {
                let (stream_id, len) = varint::decode(payload)?;
                let data = payload[len..].to_vec();
                let fin = capsule_type == capsule_type::WT_STREAM_FIN;
                Ok(Capsule::WtStream {
                    stream_id,
                    data,
                    fin,
                })
            }

            capsule_type::WT_MAX_DATA => {
                let (maximum, len) = varint::decode(payload)?;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if len != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_MAX_DATA payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtMaxData { maximum })
            }

            capsule_type::WT_MAX_STREAM_DATA => {
                let mut offset = 0;
                let (stream_id, len) = varint::decode(&payload[offset..])?;
                offset += len;
                let (maximum, len) = varint::decode(&payload[offset..])?;
                offset += len;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if offset != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_MAX_STREAM_DATA payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtMaxStreamData { stream_id, maximum })
            }

            capsule_type::WT_MAX_STREAMS_BIDI => {
                let (maximum, len) = varint::decode(payload)?;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if len != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_MAX_STREAMS_BIDI payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtMaxStreams {
                    maximum,
                    bidirectional: true,
                })
            }

            capsule_type::WT_MAX_STREAMS_UNI => {
                let (maximum, len) = varint::decode(payload)?;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if len != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_MAX_STREAMS_UNI payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtMaxStreams {
                    maximum,
                    bidirectional: false,
                })
            }

            capsule_type::WT_DATA_BLOCKED => {
                let (maximum, len) = varint::decode(payload)?;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if len != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_DATA_BLOCKED payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtDataBlocked { maximum })
            }

            capsule_type::WT_STREAM_DATA_BLOCKED => {
                let mut offset = 0;
                let (stream_id, len) = varint::decode(&payload[offset..])?;
                offset += len;
                let (maximum, len) = varint::decode(&payload[offset..])?;
                offset += len;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if offset != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_STREAM_DATA_BLOCKED payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtStreamDataBlocked { stream_id, maximum })
            }

            capsule_type::WT_STREAMS_BLOCKED_BIDI => {
                let (maximum, len) = varint::decode(payload)?;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if len != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_STREAMS_BLOCKED_BIDI payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtStreamsBlocked {
                    maximum,
                    bidirectional: true,
                })
            }

            capsule_type::WT_STREAMS_BLOCKED_UNI => {
                let (maximum, len) = varint::decode(payload)?;
                // RFC 9297 Section 3.3: 余剰バイトは不正
                if len != payload.len() {
                    return Err(WtError::capsule_decode(
                        "WT_STREAMS_BLOCKED_UNI payload has trailing bytes",
                    ));
                }
                Ok(Capsule::WtStreamsBlocked {
                    maximum,
                    bidirectional: false,
                })
            }

            capsule_type::WT_CLOSE_SESSION => {
                if payload.len() < 4 {
                    return Err(WtError::capsule_decode(
                        "WT_CLOSE_SESSION payload too short",
                    ));
                }
                // draft-ietf-webtrans-http2 Section 6.1:
                // reason の長さは 1024 バイト以下でなければならない (MUST NOT)
                let reason_len = payload.len() - 4;
                if reason_len > MAX_CLOSE_REASON_LEN {
                    return Err(WtError::capsule_decode(
                        "WT_CLOSE_SESSION reason exceeds 1024 bytes",
                    ));
                }
                let error_code =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let reason = if payload.len() > 4 {
                    String::from_utf8(payload[4..].to_vec())
                        .map_err(|_| WtError::capsule_decode("invalid UTF-8 in close reason"))?
                } else {
                    String::new()
                };
                Ok(Capsule::WtCloseSession { error_code, reason })
            }

            capsule_type::WT_DRAIN_SESSION => {
                if !payload.is_empty() {
                    return Err(WtError::capsule_decode(
                        "WT_DRAIN_SESSION must have empty payload",
                    ));
                }
                Ok(Capsule::WtDrainSession)
            }

            _ => {
                // 未知の Capsule タイプは Unknown として返す
                Ok(Capsule::Unknown {
                    capsule_type,
                    data: payload.to_vec(),
                })
            }
        }
    }

    /// バッファの残りバイト数を取得する
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.buffer.len()
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_datagram() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Datagram {
            data: b"hello".to_vec(),
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_stream() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStream {
            stream_id: 4,
            data: b"test data".to_vec(),
            fin: false,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_stream_fin() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStream {
            stream_id: 8,
            data: b"final data".to_vec(),
            fin: true,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_reset_stream() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtResetStream {
            stream_id: 4,
            error_code: 42,
            reliable_size: 1000,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_stop_sending() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStopSending {
            stream_id: 8,
            error_code: 99,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_max_data() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxData { maximum: 1_000_000 };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_max_stream_data() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxStreamData {
            stream_id: 12,
            maximum: 500_000,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_max_streams() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        // Bidirectional
        let capsule = Capsule::WtMaxStreams {
            maximum: 100,
            bidirectional: true,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);

        // Unidirectional
        encoder.clear();
        decoder.clear();

        let capsule = Capsule::WtMaxStreams {
            maximum: 50,
            bidirectional: false,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_close_session() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtCloseSession {
            error_code: 0,
            reason: "normal close".to_string(),
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_wt_drain_session() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtDrainSession;
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_encode_decode_padding() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Padding { length: 100 };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_decode_incomplete() {
        let mut decoder = CapsuleDecoder::new();

        // 不完全なデータ
        decoder.feed(&[0x00]); // DATAGRAM type only
        assert!(decoder.decode().unwrap().is_none());

        decoder.clear();

        // Type + Length のみ
        decoder.feed(&[0x00, 0x05]); // DATAGRAM, length=5
        assert!(decoder.decode().unwrap().is_none());
    }

    #[test]
    fn test_decode_multiple_capsules() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule1 = Capsule::Datagram {
            data: b"first".to_vec(),
        };
        let capsule2 = Capsule::Datagram {
            data: b"second".to_vec(),
        };

        encoder.encode(&capsule1);
        encoder.encode(&capsule2);

        decoder.feed(encoder.buffer());

        let decoded1 = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule1, decoded1);

        let decoded2 = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule2, decoded2);

        assert!(decoder.decode().unwrap().is_none());
    }

    #[test]
    fn test_decode_unknown_capsule_type() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        // 未知の Capsule タイプ
        let capsule = Capsule::Unknown {
            capsule_type: 0xFFFF,
            data: b"unknown data".to_vec(),
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_decode_wt_data_blocked() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtDataBlocked { maximum: 65536 };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_decode_wt_stream_data_blocked() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStreamDataBlocked {
            stream_id: 4,
            maximum: 32768,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }

    #[test]
    fn test_decode_wt_streams_blocked() {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        // Bidirectional
        let capsule = Capsule::WtStreamsBlocked {
            maximum: 10,
            bidirectional: true,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);

        // Unidirectional
        encoder.clear();
        decoder.clear();

        let capsule = Capsule::WtStreamsBlocked {
            maximum: 5,
            bidirectional: false,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(capsule, decoded);
    }
}
