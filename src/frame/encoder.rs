//! HTTP/2 フレームエンコーダー
//!
//! `Frame::encode(&mut BytesMut)` で呼び出し側のバッファに直接書き込む。
//! 中間バッファを持たないため、`Connection` の `output_buffer` への memcpy が
//! 設計上発生しない。

use bytes::{BufMut, Bytes, BytesMut};

use crate::error::{Error, Result};
use crate::frame::{
    ContinuationFrame, DataFrame, FRAME_HEADER_SIZE, Frame, FrameFlags, FrameHeader, FrameType,
    GoawayFrame, HeadersFrame, PingFrame, PriorityFrame, PriorityUpdateFrame, RstStreamFrame,
    SettingsFrame, WindowUpdateFrame,
};

impl Frame {
    /// フレームを呼び出し側の `BytesMut` に直接エンコードする
    ///
    /// 中間バッファを介さないため、`output_buffer` への memcpy は発生しない。
    ///
    /// # Errors
    ///
    /// `Frame::PushPromise` (サーバープッシュ非サポート) または
    /// `Frame::Priority` (RFC 9113 で非推奨) は送信不可のため `Err` を返す。
    pub fn encode(&self, buf: &mut BytesMut) -> Result<()> {
        match self {
            Self::Data(f) => encode_data(buf, f),
            Self::Headers(f) => encode_headers(buf, f),
            Self::Priority(f) => encode_priority(buf, f),
            Self::RstStream(f) => encode_rst_stream(buf, f),
            Self::Settings(f) => encode_settings(buf, f),
            Self::PushPromise { .. } => Err(Error::protocol_error(
                "PUSH_PROMISE is not supported and should not be sent",
            )),
            Self::Ping(f) => encode_ping(buf, f),
            Self::Goaway(f) => encode_goaway(buf, f),
            Self::WindowUpdate(f) => encode_window_update(buf, f),
            Self::Continuation(f) => encode_continuation(buf, f),
            Self::PriorityUpdate(f) => encode_priority_update(buf, f),
            Self::Unknown { header, payload } => encode_unknown(buf, header, payload),
        }
    }
}

/// フレームヘッダーを `BytesMut` に書き込む
fn put_header(buf: &mut BytesMut, header: &FrameHeader) {
    // Length (24 bits)
    buf.put_u8(((header.length >> 16) & 0xff) as u8);
    buf.put_u8(((header.length >> 8) & 0xff) as u8);
    buf.put_u8((header.length & 0xff) as u8);
    // Type (8 bits)
    buf.put_u8(header.frame_type);
    // Flags (8 bits)
    buf.put_u8(header.flags.bits());
    // Stream ID (31 bits, R bit is reserved)
    buf.put_u8(((header.stream_id >> 24) & 0x7f) as u8);
    buf.put_u8(((header.stream_id >> 16) & 0xff) as u8);
    buf.put_u8(((header.stream_id >> 8) & 0xff) as u8);
    buf.put_u8((header.stream_id & 0xff) as u8);
}

/// DATA フレームをエンコードする
fn encode_data(buf: &mut BytesMut, frame: &DataFrame) -> Result<()> {
    let mut flags = FrameFlags::empty();
    if frame.end_stream {
        flags = flags.set(FrameFlags::END_STREAM);
    }

    // パディング処理
    let (length, pad_length) = if let Some(pad_len) = frame.pad_length {
        flags = flags.set(FrameFlags::PADDED);
        // 1 (pad length field) + data length + pad_length (padding)
        let total = 1 + frame.data.len() as u32 + u32::from(pad_len);
        (total, Some(pad_len))
    } else {
        (frame.data.len() as u32, None)
    };

    let header = FrameHeader::new(FrameType::Data, flags, frame.stream_id).with_length(length);
    put_header(buf, &header);

    // パディング長フィールド
    if let Some(pad_len) = pad_length {
        buf.put_u8(pad_len);
    }

    // データ
    buf.extend_from_slice(&frame.data);

    // パディングバイト
    if let Some(pad_len) = pad_length {
        buf.put_bytes(0, pad_len as usize);
    }

    Ok(())
}

/// HEADERS フレームをエンコードする
///
/// # 注意
///
/// `priority_fields` は無視される（RFC 9113 で非推奨のため送信しない）。
fn encode_headers(buf: &mut BytesMut, frame: &HeadersFrame) -> Result<()> {
    let mut flags = FrameFlags::empty();
    if frame.end_stream {
        flags = flags.set(FrameFlags::END_STREAM);
    }
    if frame.end_headers {
        flags = flags.set(FrameFlags::END_HEADERS);
    }
    // priority_fields は無視（RFC 9113 で非推奨のため PRIORITY フラグは設定しない）

    // パディング処理
    let (length, pad_length) = if let Some(pad_len) = frame.pad_length {
        flags = flags.set(FrameFlags::PADDED);
        // 1 (pad length field) + header block fragment length + pad_length (padding)
        let total = 1 + frame.header_block_fragment.len() as u32 + u32::from(pad_len);
        (total, Some(pad_len))
    } else {
        (frame.header_block_fragment.len() as u32, None)
    };

    let header = FrameHeader::new(FrameType::Headers, flags, frame.stream_id).with_length(length);
    put_header(buf, &header);

    // パディング長フィールド
    if let Some(pad_len) = pad_length {
        buf.put_u8(pad_len);
    }

    // ヘッダーブロックフラグメント
    buf.extend_from_slice(&frame.header_block_fragment);

    // パディングバイト
    if let Some(pad_len) = pad_length {
        buf.put_bytes(0, pad_len as usize);
    }

    Ok(())
}

/// PRIORITY フレームをエンコードする
///
/// # 非推奨 (Deprecated)
///
/// RFC 9113 で優先度シグナリングは非推奨となった。
/// 送信は推奨されないため、エラーを返す。
fn encode_priority(_buf: &mut BytesMut, _frame: &PriorityFrame) -> Result<()> {
    Err(Error::protocol_error(
        "PRIORITY frame is deprecated in RFC 9113 and should not be sent",
    ))
}

/// RST_STREAM フレームをエンコードする
fn encode_rst_stream(buf: &mut BytesMut, frame: &RstStreamFrame) -> Result<()> {
    let header =
        FrameHeader::new(FrameType::RstStream, FrameFlags::empty(), frame.stream_id).with_length(4);

    put_header(buf, &header);
    buf.extend_from_slice(&frame.error_code.to_be_bytes());
    Ok(())
}

/// SETTINGS フレームをエンコードする
fn encode_settings(buf: &mut BytesMut, frame: &SettingsFrame) -> Result<()> {
    let mut flags = FrameFlags::empty();
    if frame.ack {
        flags = flags.set(FrameFlags::ACK);
    }

    let length = if frame.ack {
        0
    } else {
        (frame.settings.len() * 6) as u32
    };

    let header = FrameHeader::new(FrameType::Settings, flags, 0).with_length(length);
    put_header(buf, &header);

    if !frame.ack {
        for setting in &frame.settings {
            buf.extend_from_slice(&setting.id.to_be_bytes());
            buf.extend_from_slice(&setting.value.to_be_bytes());
        }
    }
    Ok(())
}

/// PING フレームをエンコードする
fn encode_ping(buf: &mut BytesMut, frame: &PingFrame) -> Result<()> {
    let mut flags = FrameFlags::empty();
    if frame.ack {
        flags = flags.set(FrameFlags::ACK);
    }

    let header = FrameHeader::new(FrameType::Ping, flags, 0).with_length(8);
    put_header(buf, &header);
    buf.extend_from_slice(&frame.opaque_data);
    Ok(())
}

/// GOAWAY フレームをエンコードする
fn encode_goaway(buf: &mut BytesMut, frame: &GoawayFrame) -> Result<()> {
    let length = (8 + frame.debug_data.len()) as u32;
    let header = FrameHeader::new(FrameType::Goaway, FrameFlags::empty(), 0).with_length(length);

    put_header(buf, &header);
    // Last-Stream-ID (31 bits, R bit is reserved)
    buf.put_u8(((frame.last_stream_id >> 24) & 0x7f) as u8);
    buf.put_u8(((frame.last_stream_id >> 16) & 0xff) as u8);
    buf.put_u8(((frame.last_stream_id >> 8) & 0xff) as u8);
    buf.put_u8((frame.last_stream_id & 0xff) as u8);
    // Error Code
    buf.extend_from_slice(&frame.error_code.to_be_bytes());
    // Debug Data
    buf.extend_from_slice(&frame.debug_data);
    Ok(())
}

/// WINDOW_UPDATE フレームをエンコードする
fn encode_window_update(buf: &mut BytesMut, frame: &WindowUpdateFrame) -> Result<()> {
    let header = FrameHeader::new(
        FrameType::WindowUpdate,
        FrameFlags::empty(),
        frame.stream_id,
    )
    .with_length(4);

    put_header(buf, &header);
    // Window Size Increment (31 bits, R bit is reserved)
    buf.put_u8(((frame.window_size_increment >> 24) & 0x7f) as u8);
    buf.put_u8(((frame.window_size_increment >> 16) & 0xff) as u8);
    buf.put_u8(((frame.window_size_increment >> 8) & 0xff) as u8);
    buf.put_u8((frame.window_size_increment & 0xff) as u8);
    Ok(())
}

/// CONTINUATION フレームをエンコードする
fn encode_continuation(buf: &mut BytesMut, frame: &ContinuationFrame) -> Result<()> {
    let mut flags = FrameFlags::empty();
    if frame.end_headers {
        flags = flags.set(FrameFlags::END_HEADERS);
    }

    let length = frame.header_block_fragment.len() as u32;
    let header =
        FrameHeader::new(FrameType::Continuation, flags, frame.stream_id).with_length(length);

    put_header(buf, &header);
    buf.extend_from_slice(&frame.header_block_fragment);
    Ok(())
}

/// PRIORITY_UPDATE フレームをエンコードする (RFC 9218 Section 4)
fn encode_priority_update(buf: &mut BytesMut, frame: &PriorityUpdateFrame) -> Result<()> {
    let length = (4 + frame.priority_field_value.len()) as u32;
    // RFC 9218 Section 4: stream identifier 0 で送信
    let header =
        FrameHeader::new(FrameType::PriorityUpdate, FrameFlags::empty(), 0).with_length(length);

    put_header(buf, &header);
    // Prioritized Element ID (31 bits, R bit is reserved)
    buf.put_u8(((frame.prioritized_element_id >> 24) & 0x7f) as u8);
    buf.put_u8(((frame.prioritized_element_id >> 16) & 0xff) as u8);
    buf.put_u8(((frame.prioritized_element_id >> 8) & 0xff) as u8);
    buf.put_u8((frame.prioritized_element_id & 0xff) as u8);
    // Priority Field Value
    buf.extend_from_slice(&frame.priority_field_value);
    Ok(())
}

/// 未知のフレームをエンコードする
fn encode_unknown(buf: &mut BytesMut, header: &FrameHeader, payload: &[u8]) -> Result<()> {
    let header = header.with_length(payload.len() as u32);
    put_header(buf, &header);
    buf.extend_from_slice(payload);
    Ok(())
}

/// フレームヘッダーをスライスにエンコードする
///
/// 呼び出し側がバッファを管理するスタイル。`BytesMut` を扱わない場面のための低レベル API。
///
/// # Errors
///
/// バッファが 9 バイト未満の場合は `Err` を返す。
pub fn encode_header(buf: &mut [u8], header: &FrameHeader) -> Result<()> {
    Error::check_buffer_size(FRAME_HEADER_SIZE, buf)?;

    // Length (24 bits)
    buf[0] = ((header.length >> 16) & 0xff) as u8;
    buf[1] = ((header.length >> 8) & 0xff) as u8;
    buf[2] = (header.length & 0xff) as u8;
    // Type (8 bits)
    buf[3] = header.frame_type;
    // Flags (8 bits)
    buf[4] = header.flags.bits();
    // Stream ID (31 bits, R bit is reserved)
    buf[5] = ((header.stream_id >> 24) & 0x7f) as u8;
    buf[6] = ((header.stream_id >> 16) & 0xff) as u8;
    buf[7] = ((header.stream_id >> 8) & 0xff) as u8;
    buf[8] = (header.stream_id & 0xff) as u8;

    Ok(())
}

/// フレームをスライスにエンコードする
///
/// 成功時はエンコードしたバイト数を返す。`BytesMut` を扱わない場面のための低レベル API。
///
/// # Errors
///
/// バッファが不足している場合は `Err` を返す。
pub fn encode_frame(buf: &mut [u8], frame: &Frame) -> Result<usize> {
    let mut tmp = BytesMut::new();
    frame.encode(&mut tmp)?;
    Error::check_buffer_size(tmp.len(), buf)?;
    buf[..tmp.len()].copy_from_slice(&tmp);
    Ok(tmp.len())
}

/// フレームをエンコードして `Bytes` として返す
///
/// 一回限りのエンコード用 helper。`Connection` 等のホットパスでは
/// `frame.encode(&mut output_buffer)` を直接呼び、中間バッファを介さない。
///
/// # Errors
///
/// `Frame::encode` のエラーを伝播する。
pub fn encode_frame_to_bytes(frame: &Frame) -> Result<Bytes> {
    let mut buf = BytesMut::new();
    frame.encode(&mut buf)?;
    Ok(buf.freeze())
}
