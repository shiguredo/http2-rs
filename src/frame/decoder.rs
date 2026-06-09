//! HTTP/2 フレームデコーダー

use core::num::NonZeroU32;

use crate::decode_error::DecodeError;
use crate::error::{Error, ErrorCode, Result};
use crate::frame::error::{LastStreamId, Weight, WindowIncrement};
use crate::frame::{
    ContinuationFrame, DataFrame, FRAME_HEADER_SIZE, Frame, FrameFlags, FrameHeader, FrameType,
    GoawayFrame, HeadersFrame, NonZeroStreamId, PingFrame, PriorityFields, PriorityFrame,
    PriorityUpdateFrame, RstStreamFrame, SettingsFrame, StreamId, WindowUpdateFrame,
};
use crate::settings::Setting;

/// フレームデコーダー
///
/// ストリーミング方式でフレームをデコードする。
#[derive(Debug)]
pub struct FrameDecoder {
    /// 最大フレームサイズ
    max_frame_size: u32,
    /// 内部バッファ
    buf: Vec<u8>,
    /// 現在パース中のフレームヘッダー
    current_header: Option<FrameHeader>,
    /// 最後にデコード試行したフレームのストリーム ID
    /// ストリームエラー時に RST_STREAM の送信先を特定するために使用する
    last_decoded_stream_id: Option<u32>,
}

impl FrameDecoder {
    /// 新しい `FrameDecoder` を生成する
    #[must_use]
    pub fn new(max_frame_size: u32) -> Self {
        Self {
            max_frame_size,
            buf: Vec::new(),
            current_header: None,
            last_decoded_stream_id: None,
        }
    }

    /// データを入力バッファに追加する
    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// フレームをデコードする
    ///
    /// 完全なフレームがデコードできた場合は `Ok(Some(frame))` を返す。
    /// データが不足している場合は `Ok(None)` を返す。
    /// デコードエラーの場合は `Err` を返す。
    pub fn decode(&mut self) -> Result<Option<Frame>> {
        // ヘッダーをまだ読んでいない場合
        if self.current_header.is_none() {
            if self.buf.len() < FRAME_HEADER_SIZE {
                return Ok(None);
            }

            let header = decode_header(&self.buf[..FRAME_HEADER_SIZE])?;

            // RFC 9113 Section 4.2 (Frame Size): SETTINGS_MAX_FRAME_SIZE を超えるフレームは FRAME_SIZE_ERROR (MUST)
            if header.length > self.max_frame_size {
                return Err(Error::frame_size_error(format!(
                    "frame size {} exceeds max frame size {}",
                    header.length, self.max_frame_size
                )));
            }

            self.current_header = Some(header);
            self.buf.drain(..FRAME_HEADER_SIZE);
        }

        // ここに到達する時点で current_header は必ず Some (上の分岐で設定済み)
        let header = self
            .current_header
            .as_ref()
            .expect("current_header must be set");
        let payload_len = header.length as usize;

        // ペイロードが揃うまで待つ
        if self.buf.len() < payload_len {
            return Ok(None);
        }

        let header = self
            .current_header
            .take()
            .expect("current_header must be set");
        self.last_decoded_stream_id = Some(header.stream_id);
        let payload: Vec<u8> = self.buf.drain(..payload_len).collect();

        decode_frame(header, &payload).map(Some)
    }

    /// 内部バッファの残りデータ長を取得する
    #[must_use]
    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }

    /// 最後にデコード試行したフレームのストリーム ID を返す
    ///
    /// ストリームエラー発生時に RST_STREAM の送信先を特定するために使用する。
    #[must_use]
    pub fn last_decoded_stream_id(&self) -> Option<u32> {
        self.last_decoded_stream_id
    }

    /// 内部バッファをクリアする
    pub fn clear(&mut self) {
        self.buf.clear();
        self.current_header = None;
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new(crate::settings::DEFAULT_MAX_FRAME_SIZE)
    }
}

/// フレームヘッダーをデコードする
///
/// # Errors
///
/// バッファが 9 バイト未満の場合は `Err` を返す。
pub fn decode_header(buf: &[u8]) -> Result<FrameHeader> {
    if buf.len() < FRAME_HEADER_SIZE {
        return Err(DecodeError::Incomplete.into());
    }

    // Length (24 bits)
    let length = (u32::from(buf[0]) << 16) | (u32::from(buf[1]) << 8) | u32::from(buf[2]);

    // Type (8 bits)
    let frame_type = buf[3];

    // Flags (8 bits)
    let flags = FrameFlags::from_bits(buf[4]);

    // Stream ID (31 bits)。RFC 9113 Section 4.1: Reserved ビットは受信時に無視しなければならない (MUST)
    let stream_id = ((u32::from(buf[5]) & 0x7f) << 24)
        | (u32::from(buf[6]) << 16)
        | (u32::from(buf[7]) << 8)
        | u32::from(buf[8]);

    Ok(FrameHeader {
        length,
        frame_type,
        flags,
        stream_id,
    })
}

/// フレームをデコードする
fn decode_frame(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    match FrameType::from_u8(header.frame_type) {
        Some(FrameType::Data) => decode_data(header, payload),
        Some(FrameType::Headers) => decode_headers(header, payload),
        Some(FrameType::Priority) => decode_priority(header, payload),
        Some(FrameType::RstStream) => decode_rst_stream(header, payload),
        Some(FrameType::Settings) => decode_settings(header, payload),
        Some(FrameType::PushPromise) => decode_push_promise(header, payload),
        Some(FrameType::Ping) => decode_ping(header, payload),
        Some(FrameType::Goaway) => decode_goaway(header, payload),
        Some(FrameType::WindowUpdate) => decode_window_update(header, payload),
        Some(FrameType::Continuation) => decode_continuation(header, payload),
        Some(FrameType::PriorityUpdate) => decode_priority_update(header, payload),
        None => Ok(Frame::Unknown {
            header,
            payload: payload.to_vec(),
        }),
    }
}

/// DATA フレームをデコードする
fn decode_data(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9113 §6.1: DATA フレームはストリーム ID が 0 であってはならない
    let stream_id = require_non_zero_stream_id(header.stream_id, "DATA")?;

    let end_stream = header.flags.is_end_stream();
    let padded = header.flags.is_padded();

    let (data, pad_length) = if padded {
        if payload.is_empty() {
            return Err(Error::frame_size_error(
                "PADDED flag set but no padding length",
            ));
        }
        let pad_len = payload[0] as usize;
        if payload.len() < 1 + pad_len {
            return Err(Error::protocol_error(
                "padding length exceeds frame payload",
            ));
        }
        (
            payload[1..payload.len() - pad_len].to_vec(),
            Some(payload[0]),
        )
    } else {
        (payload.to_vec(), None)
    };

    Ok(Frame::Data(DataFrame {
        stream_id,
        end_stream,
        data,
        pad_length,
    }))
}

/// HEADERS フレームをデコードする
fn decode_headers(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9113 §6.2: HEADERS フレームはストリーム ID が 0 であってはならない
    let stream_id = require_non_zero_stream_id(header.stream_id, "HEADERS")?;

    let end_stream = header.flags.is_end_stream();
    let end_headers = header.flags.is_end_headers();
    let padded = header.flags.is_padded();
    let priority = header.flags.is_priority();

    let mut offset = 0;
    let mut data_end = payload.len();
    let mut pad_length = None;

    if padded {
        if payload.is_empty() {
            return Err(Error::frame_size_error(
                "PADDED flag set but no padding length",
            ));
        }
        let pad_len = payload[0] as usize;
        pad_length = Some(payload[0]);
        offset = 1;
        if payload.len() < 1 + pad_len {
            return Err(Error::protocol_error(
                "padding length exceeds frame payload",
            ));
        }
        data_end = payload.len() - pad_len;
    }

    // PRIORITY フラグが設定されている場合、5 バイトの優先度フィールドをパース
    // RFC 9113 §6.2: 非推奨だが相互運用性のため処理する
    let priority_fields = if priority {
        if data_end - offset < 5 {
            return Err(Error::frame_size_error(
                "PRIORITY flag set but insufficient data for priority fields",
            ));
        }
        let exclusive = (payload[offset] & 0x80) != 0;
        let stream_dependency = ((u32::from(payload[offset]) & 0x7f) << 24)
            | (u32::from(payload[offset + 1]) << 16)
            | (u32::from(payload[offset + 2]) << 8)
            | u32::from(payload[offset + 3]);
        // u8 は Weight の有効範囲 (0..=255) に常に収まる
        let weight = Weight::from_validated_parts(payload[offset + 4]);
        offset += 5;
        Some(PriorityFields {
            exclusive,
            stream_dependency: StreamId::from_wire(stream_dependency),
            weight,
        })
    } else {
        None
    };

    let header_block_fragment = payload[offset..data_end].to_vec();

    Ok(Frame::Headers(HeadersFrame {
        stream_id,
        end_stream,
        end_headers,
        priority_fields,
        header_block_fragment,
        pad_length,
    }))
}

/// PRIORITY フレームをデコードする
///
/// # 非推奨 (Deprecated)
///
/// RFC 9113 で優先度シグナリングは非推奨となった。
/// 相互運用性のため受信は処理するが、優先度制御には使用しない。
fn decode_priority(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9113 §6.3: PRIORITY フレームはストリーム ID が 0 であってはならない
    let stream_id = require_non_zero_stream_id(header.stream_id, "PRIORITY")?;

    // RFC 9113 §6.3: PRIORITY フレームは常に 5 バイト。
    // 長さが異なる場合はストリームエラーの FRAME_SIZE_ERROR として扱う。
    if payload.len() != 5 {
        return Err(Error::stream_error(
            crate::error::ErrorCode::FrameSizeError,
            "PRIORITY frame must be 5 bytes",
        ));
    }

    let exclusive = (payload[0] & 0x80) != 0;
    let stream_dependency = ((u32::from(payload[0]) & 0x7f) << 24)
        | (u32::from(payload[1]) << 16)
        | (u32::from(payload[2]) << 8)
        | u32::from(payload[3]);
    // u8 は Weight の有効範囲 (0..=255) に常に収まる
    let weight = Weight::from_validated_parts(payload[4]);

    Ok(Frame::Priority(PriorityFrame {
        stream_id,
        exclusive,
        stream_dependency: StreamId::from_wire(stream_dependency),
        weight,
    }))
}

/// RST_STREAM フレームをデコードする
fn decode_rst_stream(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9113 §6.4: RST_STREAM フレームはストリーム ID が 0 であってはならない
    let stream_id = require_non_zero_stream_id(header.stream_id, "RST_STREAM")?;

    // RST_STREAM フレームは常に 4 バイト
    if payload.len() != 4 {
        return Err(Error::frame_size_error("RST_STREAM frame must be 4 bytes"));
    }

    let error_code = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);

    Ok(Frame::RstStream(RstStreamFrame {
        stream_id,
        error_code,
    }))
}

/// SETTINGS フレームをデコードする
fn decode_settings(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9113 Section 6.5 (SETTINGS): SETTINGS フレームはストリーム ID が 0 でなければならない
    if header.stream_id != 0 {
        return Err(Error::protocol_error(
            "SETTINGS frame with non-zero stream ID",
        ));
    }

    let ack = header.flags.is_ack();

    // RFC 9113 Section 6.5 (SETTINGS): ACK フラグが設定されている場合、ペイロードは空でなければならない
    if ack {
        if !payload.is_empty() {
            return Err(Error::frame_size_error(
                "SETTINGS ACK frame with non-empty payload",
            ));
        }
        return Ok(Frame::Settings(SettingsFrame::ack()));
    }

    // RFC 9113 Section 6.5 (SETTINGS): ペイロードは 6 バイトの倍数でなければならない
    if !payload.len().is_multiple_of(6) {
        return Err(Error::frame_size_error(
            "SETTINGS frame payload must be a multiple of 6 bytes",
        ));
    }

    let mut frame = SettingsFrame::new();
    for chunk in payload.chunks(6) {
        let id = u16::from_be_bytes([chunk[0], chunk[1]]);
        let value = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
        let setting = Setting::from_wire(id, value).map_err(|e| {
            let error_code = match e {
                crate::settings::SettingError::InitialWindowSizeOutOfRange { .. } => {
                    ErrorCode::FlowControlError
                }
                _ => ErrorCode::ProtocolError,
            };
            Error::connection_error(error_code, e.to_string())
        })?;
        frame.add(setting);
    }

    Ok(Frame::Settings(frame))
}

/// PING フレームをデコードする
fn decode_ping(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9113 Section 6.7 (PING): PING フレームはストリーム ID が 0 でなければならない
    if header.stream_id != 0 {
        return Err(Error::protocol_error("PING frame with non-zero stream ID"));
    }

    // RFC 9113 Section 6.7 (PING): PING フレームは常に 8 バイト。それ以外は FRAME_SIZE_ERROR (MUST)
    if payload.len() != 8 {
        return Err(Error::frame_size_error("PING frame must be 8 bytes"));
    }

    let ack = header.flags.is_ack();
    let mut opaque_data = [0u8; 8];
    opaque_data.copy_from_slice(payload);

    Ok(Frame::Ping(PingFrame { ack, opaque_data }))
}

/// GOAWAY フレームをデコードする
fn decode_goaway(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9113 §6.8: GOAWAY フレームはストリーム ID が 0 でなければならない
    if header.stream_id != 0 {
        return Err(Error::protocol_error(
            "GOAWAY frame with non-zero stream ID",
        ));
    }

    // RFC 9113 Section 4.2 (Frame Size): 必須フィールドを収められないフレームは FRAME_SIZE_ERROR (MUST)。GOAWAY は最低 8 バイト
    if payload.len() < 8 {
        return Err(Error::frame_size_error(
            "GOAWAY frame must be at least 8 bytes",
        ));
    }

    let last_stream_id_raw = ((u32::from(payload[0]) & 0x7f) << 24)
        | (u32::from(payload[1]) << 16)
        | (u32::from(payload[2]) << 8)
        | u32::from(payload[3]);
    // wire 上は 31-bit マスク済みなので LastStreamId の範囲 (0..=2^31-1) に必ず収まる
    let last_stream_id = LastStreamId::from_validated_parts(last_stream_id_raw);

    let error_code = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);

    let debug_data = if payload.len() > 8 {
        payload[8..].to_vec()
    } else {
        Vec::new()
    };

    Ok(Frame::Goaway(GoawayFrame {
        last_stream_id,
        error_code,
        debug_data,
    }))
}

/// WINDOW_UPDATE フレームをデコードする
fn decode_window_update(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // WINDOW_UPDATE フレームは常に 4 バイト
    if payload.len() != 4 {
        return Err(Error::frame_size_error(
            "WINDOW_UPDATE frame must be 4 bytes",
        ));
    }

    let raw_increment = ((u32::from(payload[0]) & 0x7f) << 24)
        | (u32::from(payload[1]) << 16)
        | (u32::from(payload[2]) << 8)
        | u32::from(payload[3]);

    // RFC 9113 §6.9: increment = 0 は接続 / ストリームでエラー種別が異なる
    if raw_increment == 0 {
        if header.stream_id == 0 {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "WINDOW_UPDATE with zero increment on connection",
            ));
        }
        return Err(Error::stream_error(
            ErrorCode::ProtocolError,
            "WINDOW_UPDATE with zero increment on stream",
        ));
    }

    // 非ゼロかつ 31-bit マスク済みなので WindowIncrement の範囲に必ず収まる
    let window_size_increment = WindowIncrement::from_validated_parts(
        NonZeroU32::new(raw_increment).expect("zero increment already rejected above"),
    );

    Ok(Frame::WindowUpdate(WindowUpdateFrame {
        stream_id: StreamId::from_wire(header.stream_id),
        window_size_increment,
    }))
}

/// CONTINUATION フレームをデコードする
fn decode_continuation(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9113 §6.10: CONTINUATION フレームはストリーム ID が 0 であってはならない
    let stream_id = require_non_zero_stream_id(header.stream_id, "CONTINUATION")?;

    let end_headers = header.flags.is_end_headers();
    let header_block_fragment = payload.to_vec();

    Ok(Frame::Continuation(ContinuationFrame {
        stream_id,
        end_headers,
        header_block_fragment,
    }))
}

/// PUSH_PROMISE フレームをデコードする (RFC 9113 §6.6)
///
/// # 非サポート
///
/// サーバープッシュは主要ブラウザでサポートが削除されているため、
/// このライブラリでは送信機能を提供しない。
/// 受信時はストリーム ID のみ抽出し、connection モジュールでエラー処理する。
fn decode_push_promise(header: FrameHeader, _payload: &[u8]) -> Result<Frame> {
    // RFC 9113 §6.6: PUSH_PROMISE フレームはストリーム ID が 0 であってはならない
    if header.stream_id == 0 {
        return Err(Error::protocol_error("PUSH_PROMISE frame with stream ID 0"));
    }

    // ペイロードの詳細なデコードは不要 (エラーを返すため)
    Ok(Frame::PushPromise {
        stream_id: StreamId::from_wire(header.stream_id),
    })
}

/// PRIORITY_UPDATE フレームをデコードする (RFC 9218 §7.1)
fn decode_priority_update(header: FrameHeader, payload: &[u8]) -> Result<Frame> {
    // RFC 9218 §7.1: PRIORITY_UPDATE フレームはストリーム ID が 0 でなければならない
    if header.stream_id != 0 {
        return Err(Error::protocol_error(
            "PRIORITY_UPDATE frame with non-zero stream ID",
        ));
    }

    // RFC 9218 §7.1: ペイロードは最低 4 バイト (Prioritized Stream ID)
    if payload.len() < 4 {
        return Err(Error::frame_size_error(
            "PRIORITY_UPDATE frame must be at least 4 bytes",
        ));
    }

    // Prioritized Element ID (31 bits)
    let raw_id = ((u32::from(payload[0]) & 0x7f) << 24)
        | (u32::from(payload[1]) << 16)
        | (u32::from(payload[2]) << 8)
        | u32::from(payload[3]);

    let nz_id = NonZeroU32::new(raw_id).ok_or_else(|| {
        Error::protocol_error("PRIORITY_UPDATE prioritized element ID must not be 0")
    })?;
    let prioritized_element_id = NonZeroStreamId::from_validated_parts(nz_id);

    // Priority Field Value (残りのバイト)
    let priority_field_value = if payload.len() > 4 {
        payload[4..].to_vec()
    } else {
        Vec::new()
    };

    Ok(Frame::PriorityUpdate(PriorityUpdateFrame {
        prioritized_element_id,
        priority_field_value,
    }))
}

/// stream_id が 0 の場合に PROTOCOL_ERROR を返すヘルパー
fn require_non_zero_stream_id(raw: u32, frame_name: &str) -> Result<NonZeroStreamId> {
    let nz = NonZeroU32::new(raw)
        .ok_or_else(|| Error::protocol_error(format!("{frame_name} frame with stream ID 0")))?;
    Ok(NonZeroStreamId::from_validated_parts(nz))
}
