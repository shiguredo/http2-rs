//! HTTP/2 フレームエンコーダー

use crate::decode_error::DecodeError;
use crate::error::{Error, Result};
use crate::frame::{
    ContinuationFrame, DataFrame, FRAME_HEADER_SIZE, Frame, FrameFlags, FrameHeader, FrameType,
    GoawayFrame, HeadersFrame, PingFrame, PriorityFrame, PriorityUpdateFrame, RstStreamFrame,
    SettingsFrame, WindowUpdateFrame,
};

/// フレームエンコーダー
#[derive(Debug)]
pub struct FrameEncoder {
    /// 出力バッファ
    buf: Vec<u8>,
}

impl FrameEncoder {
    /// 新しい `FrameEncoder` を生成する
    #[must_use]
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// 内部バッファへの参照を取得する
    #[must_use]
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// 内部バッファをクリアする
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// 内部バッファの内容を取得してクリアする
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }

    /// フレームをエンコードする
    pub fn encode(&mut self, frame: &Frame) -> Result<()> {
        match frame {
            Frame::Data(f) => self.encode_data(f),
            Frame::Headers(f) => self.encode_headers(f),
            Frame::Priority(f) => self.encode_priority(f),
            Frame::RstStream(f) => self.encode_rst_stream(f),
            Frame::Settings(f) => self.encode_settings(f),
            Frame::PushPromise { .. } => {
                // PUSH_PROMISE は送信しない (サーバープッシュ非サポート)
                Err(Error::protocol_error(
                    "PUSH_PROMISE is not supported and should not be sent",
                ))
            }
            Frame::Ping(f) => self.encode_ping(f),
            Frame::Goaway(f) => self.encode_goaway(f),
            Frame::WindowUpdate(f) => self.encode_window_update(f),
            Frame::Continuation(f) => self.encode_continuation(f),
            Frame::PriorityUpdate(f) => self.encode_priority_update(f),
            Frame::Unknown { header, payload } => self.encode_unknown(header, payload),
        }
    }

    /// フレームヘッダーをエンコードする
    fn encode_header(&mut self, header: &FrameHeader) {
        // Length (24 bits)
        self.buf.push(((header.length >> 16) & 0xff) as u8);
        self.buf.push(((header.length >> 8) & 0xff) as u8);
        self.buf.push((header.length & 0xff) as u8);
        // Type (8 bits)
        self.buf.push(header.frame_type);
        // Flags (8 bits)
        self.buf.push(header.flags.bits());
        // Stream ID (31 bits, R bit is reserved)
        self.buf.push(((header.stream_id >> 24) & 0x7f) as u8);
        self.buf.push(((header.stream_id >> 16) & 0xff) as u8);
        self.buf.push(((header.stream_id >> 8) & 0xff) as u8);
        self.buf.push((header.stream_id & 0xff) as u8);
    }

    /// DATA フレームをエンコードする
    fn encode_data(&mut self, frame: &DataFrame) -> Result<()> {
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

        self.encode_header(&header);

        // パディング長フィールド
        if let Some(pad_len) = pad_length {
            self.buf.push(pad_len);
        }

        // データ
        self.buf.extend_from_slice(&frame.data);

        // パディングバイト
        if let Some(pad_len) = pad_length {
            self.buf.extend(std::iter::repeat_n(0u8, pad_len as usize));
        }

        Ok(())
    }

    /// HEADERS フレームをエンコードする
    ///
    /// # 注意
    ///
    /// `priority_fields` は無視される（RFC 9113 で非推奨のため送信しない）。
    fn encode_headers(&mut self, frame: &HeadersFrame) -> Result<()> {
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

        let header =
            FrameHeader::new(FrameType::Headers, flags, frame.stream_id).with_length(length);

        self.encode_header(&header);

        // パディング長フィールド
        if let Some(pad_len) = pad_length {
            self.buf.push(pad_len);
        }

        // ヘッダーブロックフラグメント
        self.buf.extend_from_slice(&frame.header_block_fragment);

        // パディングバイト
        if let Some(pad_len) = pad_length {
            self.buf.extend(std::iter::repeat_n(0u8, pad_len as usize));
        }

        Ok(())
    }

    /// PRIORITY フレームをエンコードする
    ///
    /// # 非推奨 (Deprecated)
    ///
    /// RFC 9113 で優先度シグナリングは非推奨となった。
    /// 送信は推奨されないため、エラーを返す。
    fn encode_priority(&mut self, _frame: &PriorityFrame) -> Result<()> {
        Err(Error::protocol_error(
            "PRIORITY frame is deprecated in RFC 9113 and should not be sent",
        ))
    }

    /// RST_STREAM フレームをエンコードする
    fn encode_rst_stream(&mut self, frame: &RstStreamFrame) -> Result<()> {
        let header = FrameHeader::new(FrameType::RstStream, FrameFlags::empty(), frame.stream_id)
            .with_length(4);

        self.encode_header(&header);
        self.buf.extend_from_slice(&frame.error_code.to_be_bytes());
        Ok(())
    }

    /// SETTINGS フレームをエンコードする
    fn encode_settings(&mut self, frame: &SettingsFrame) -> Result<()> {
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

        self.encode_header(&header);

        if !frame.ack {
            for setting in &frame.settings {
                self.buf.extend_from_slice(&setting.id.to_be_bytes());
                self.buf.extend_from_slice(&setting.value.to_be_bytes());
            }
        }
        Ok(())
    }

    /// PING フレームをエンコードする
    fn encode_ping(&mut self, frame: &PingFrame) -> Result<()> {
        let mut flags = FrameFlags::empty();
        if frame.ack {
            flags = flags.set(FrameFlags::ACK);
        }

        let header = FrameHeader::new(FrameType::Ping, flags, 0).with_length(8);

        self.encode_header(&header);
        self.buf.extend_from_slice(&frame.opaque_data);
        Ok(())
    }

    /// GOAWAY フレームをエンコードする
    fn encode_goaway(&mut self, frame: &GoawayFrame) -> Result<()> {
        let length = (8 + frame.debug_data.len()) as u32;
        let header =
            FrameHeader::new(FrameType::Goaway, FrameFlags::empty(), 0).with_length(length);

        self.encode_header(&header);
        // Last-Stream-ID (31 bits, R bit is reserved)
        self.buf.push(((frame.last_stream_id >> 24) & 0x7f) as u8);
        self.buf.push(((frame.last_stream_id >> 16) & 0xff) as u8);
        self.buf.push(((frame.last_stream_id >> 8) & 0xff) as u8);
        self.buf.push((frame.last_stream_id & 0xff) as u8);
        // Error Code
        self.buf.extend_from_slice(&frame.error_code.to_be_bytes());
        // Debug Data
        self.buf.extend_from_slice(&frame.debug_data);
        Ok(())
    }

    /// WINDOW_UPDATE フレームをエンコードする
    fn encode_window_update(&mut self, frame: &WindowUpdateFrame) -> Result<()> {
        let header = FrameHeader::new(
            FrameType::WindowUpdate,
            FrameFlags::empty(),
            frame.stream_id,
        )
        .with_length(4);

        self.encode_header(&header);
        // Window Size Increment (31 bits, R bit is reserved)
        self.buf
            .push(((frame.window_size_increment >> 24) & 0x7f) as u8);
        self.buf
            .push(((frame.window_size_increment >> 16) & 0xff) as u8);
        self.buf
            .push(((frame.window_size_increment >> 8) & 0xff) as u8);
        self.buf.push((frame.window_size_increment & 0xff) as u8);
        Ok(())
    }

    /// CONTINUATION フレームをエンコードする
    fn encode_continuation(&mut self, frame: &ContinuationFrame) -> Result<()> {
        let mut flags = FrameFlags::empty();
        if frame.end_headers {
            flags = flags.set(FrameFlags::END_HEADERS);
        }

        let length = frame.header_block_fragment.len() as u32;
        let header =
            FrameHeader::new(FrameType::Continuation, flags, frame.stream_id).with_length(length);

        self.encode_header(&header);
        self.buf.extend_from_slice(&frame.header_block_fragment);
        Ok(())
    }

    /// PRIORITY_UPDATE フレームをエンコードする (RFC 9218 Section 7.1)
    fn encode_priority_update(&mut self, frame: &PriorityUpdateFrame) -> Result<()> {
        let length = (4 + frame.priority_field_value.len()) as u32;
        // RFC 9218 Section 7.1: stream identifier 0 で送信
        let header =
            FrameHeader::new(FrameType::PriorityUpdate, FrameFlags::empty(), 0).with_length(length);

        self.encode_header(&header);
        // Prioritized Element ID (31 bits, R bit is reserved)
        self.buf
            .push(((frame.prioritized_element_id >> 24) & 0x7f) as u8);
        self.buf
            .push(((frame.prioritized_element_id >> 16) & 0xff) as u8);
        self.buf
            .push(((frame.prioritized_element_id >> 8) & 0xff) as u8);
        self.buf.push((frame.prioritized_element_id & 0xff) as u8);
        // Priority Field Value
        self.buf.extend_from_slice(&frame.priority_field_value);
        Ok(())
    }

    /// 未知のフレームをエンコードする
    fn encode_unknown(&mut self, header: &FrameHeader, payload: &[u8]) -> Result<()> {
        let header = header.with_length(payload.len() as u32);
        self.encode_header(&header);
        self.buf.extend_from_slice(payload);
        Ok(())
    }
}

impl Default for FrameEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// フレームヘッダーをバッファにエンコードする
///
/// # Errors
///
/// バッファが 9 バイト未満の場合は `Err` を返す。
pub fn encode_header(buf: &mut [u8], header: &FrameHeader) -> Result<()> {
    DecodeError::check_buffer_size(FRAME_HEADER_SIZE, buf)?;

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

/// フレームをバッファにエンコードする
///
/// 成功時はエンコードしたバイト数を返す。
///
/// # Errors
///
/// バッファが不足している場合は `Err` を返す。
pub fn encode_frame(buf: &mut [u8], frame: &Frame) -> Result<usize> {
    let mut encoder = FrameEncoder::new();
    encoder.encode(frame)?;
    let encoded = encoder.buffer();
    DecodeError::check_buffer_size(encoded.len(), buf)?;
    buf[..encoded.len()].copy_from_slice(encoded);
    Ok(encoded.len())
}

/// フレームをエンコードして Vec<u8> として返す
pub fn encode_frame_to_vec(frame: &Frame) -> Result<Vec<u8>> {
    let mut encoder = FrameEncoder::new();
    encoder.encode(frame)?;
    Ok(encoder.take())
}
