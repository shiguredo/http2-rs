#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::{
    ContinuationFrame, DataFrame, Frame, FrameEncoder, FrameFlags, FrameHeader, GoawayFrame,
    HeadersFrame, LastStreamId, NonZeroStreamId, PingFrame, PriorityUpdateFrame, RstStreamFrame,
    Setting, SettingsFrame, StreamId, Weight, WindowIncrement, WindowUpdateFrame,
};

#[derive(Debug, Arbitrary)]
enum FuzzFrame {
    Data {
        stream_id: u32,
        data: Vec<u8>,
        end_stream: bool,
        pad_length: Option<u8>,
    },
    Headers {
        stream_id: u32,
        header_block_fragment: Vec<u8>,
        end_stream: bool,
        end_headers: bool,
        pad_length: Option<u8>,
    },
    RstStream {
        stream_id: u32,
        error_code: u32,
    },
    Settings {
        ack: bool,
        settings: Vec<(u16, u32)>,
    },
    Ping {
        opaque_data: [u8; 8],
        ack: bool,
    },
    Goaway {
        last_stream_id: u32,
        error_code: u32,
        debug_data: Vec<u8>,
    },
    WindowUpdate {
        stream_id: u32,
        increment: u32,
    },
    Continuation {
        stream_id: u32,
        header_block_fragment: Vec<u8>,
        end_headers: bool,
    },
    PriorityUpdate {
        element_id: u32,
        priority_field_value: Vec<u8>,
    },
    Priority {
        stream_id: u32,
        dependency: u32,
        weight: u16,
        exclusive: bool,
    },
    PushPromise {
        stream_id: u32,
    },
    Unknown {
        frame_type: u8,
        flags: u8,
        stream_id: u32,
        payload: Vec<u8>,
    },
}

fn try_build_frame(input: &FuzzFrame) -> Option<Frame> {
    match input {
        FuzzFrame::Data {
            stream_id,
            data,
            end_stream,
            pad_length,
        } => {
            let sid = NonZeroStreamId::new(*stream_id).ok()?;
            let mut frame = DataFrame::new(sid, data.clone()).with_end_stream(*end_stream);
            if let Some(pad) = pad_length {
                frame = frame.with_padding(*pad);
            }
            Some(Frame::Data(frame))
        }
        FuzzFrame::Headers {
            stream_id,
            header_block_fragment,
            end_stream,
            end_headers,
            pad_length,
        } => {
            let sid = NonZeroStreamId::new(*stream_id).ok()?;
            let mut frame = HeadersFrame::new(sid, header_block_fragment.clone())
                .with_end_stream(*end_stream)
                .with_end_headers(*end_headers);
            if let Some(pad) = pad_length {
                frame = frame.with_padding(*pad);
            }
            Some(Frame::Headers(frame))
        }
        FuzzFrame::RstStream {
            stream_id,
            error_code,
        } => {
            let sid = NonZeroStreamId::new(*stream_id).ok()?;
            Some(Frame::RstStream(RstStreamFrame::new(sid, *error_code)))
        }
        FuzzFrame::Settings { ack, settings } => {
            if *ack {
                Some(Frame::Settings(SettingsFrame::ack()))
            } else {
                let mut frame = SettingsFrame::new();
                for (id, value) in settings {
                    if let Ok(setting) = Setting::from_wire(*id, *value) {
                        frame.add(setting);
                    }
                }
                Some(Frame::Settings(frame))
            }
        }
        FuzzFrame::Ping { opaque_data, ack } => {
            if *ack {
                Some(Frame::Ping(PingFrame::ack(*opaque_data)))
            } else {
                Some(Frame::Ping(PingFrame::new(*opaque_data)))
            }
        }
        FuzzFrame::Goaway {
            last_stream_id,
            error_code,
            debug_data,
        } => {
            let lsid = LastStreamId::new(*last_stream_id).ok()?;
            Some(Frame::Goaway(
                GoawayFrame::new(lsid, *error_code).with_debug_data(debug_data.clone()),
            ))
        }
        FuzzFrame::WindowUpdate {
            stream_id,
            increment,
        } => {
            let inc = WindowIncrement::new(*increment).ok()?;
            if *stream_id == 0 {
                Some(Frame::WindowUpdate(WindowUpdateFrame::for_connection(inc)))
            } else {
                let sid = NonZeroStreamId::new(*stream_id).ok()?;
                Some(Frame::WindowUpdate(WindowUpdateFrame::for_stream(sid, inc)))
            }
        }
        FuzzFrame::Continuation {
            stream_id,
            header_block_fragment,
            end_headers,
        } => {
            let sid = NonZeroStreamId::new(*stream_id).ok()?;
            Some(Frame::Continuation(
                ContinuationFrame::new(sid, header_block_fragment.clone())
                    .with_end_headers(*end_headers),
            ))
        }
        FuzzFrame::PriorityUpdate {
            element_id,
            priority_field_value,
        } => {
            let eid = NonZeroStreamId::new(*element_id).ok()?;
            Some(Frame::PriorityUpdate(PriorityUpdateFrame::new(
                eid,
                priority_field_value.clone(),
            )))
        }
        FuzzFrame::Priority {
            stream_id,
            dependency,
            weight,
            exclusive,
        } => {
            let sid = NonZeroStreamId::new(*stream_id).ok()?;
            let dep = StreamId::from_wire(*dependency & 0x7FFF_FFFF);
            let w = Weight::new(*weight).ok()?;
            Some(Frame::Priority(shiguredo_http2::frame::PriorityFrame {
                stream_id: sid,
                exclusive: *exclusive,
                stream_dependency: dep,
                weight: w,
            }))
        }
        FuzzFrame::PushPromise { stream_id } => Some(Frame::PushPromise {
            stream_id: StreamId::from_wire(*stream_id & 0x7FFF_FFFF),
        }),
        FuzzFrame::Unknown {
            frame_type,
            flags,
            stream_id,
            payload,
        } => {
            let header = FrameHeader {
                length: 0,
                frame_type: *frame_type,
                flags: FrameFlags::from_bits(*flags),
                stream_id: *stream_id & 0x7FFF_FFFF,
            };
            Some(Frame::Unknown {
                header,
                payload: payload.clone(),
            })
        }
    }
}

fuzz_target!(|input: FuzzFrame| {
    let mut encoder = FrameEncoder::new();
    if let Some(frame) = try_build_frame(&input) {
        let _ = encoder.encode(&frame);
    }
});
