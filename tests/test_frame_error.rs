//! フレーム構築時検査エラーと関連型の単体テスト

use shiguredo_http2::frame::{FrameError, FrameType, LastStreamId, Weight, WindowIncrement};

#[test]
fn window_increment_new_ok() {
    let w = WindowIncrement::new(1).expect("increment 1 は有効");
    assert_eq!(w.as_u32(), 1);
    let w = WindowIncrement::new(WindowIncrement::MAX).expect("MAX increment は有効");
    assert_eq!(w.as_u32(), WindowIncrement::MAX);
}

#[test]
fn window_increment_new_zero() {
    assert_eq!(
        WindowIncrement::new(0),
        Err(FrameError::ZeroWindowIncrement)
    );
}

#[test]
fn window_increment_new_overflow() {
    assert_eq!(
        WindowIncrement::new(WindowIncrement::MAX + 1),
        Err(FrameError::WindowIncrementOutOfRange {
            value: WindowIncrement::MAX + 1
        })
    );
}

#[test]
fn window_increment_from_static_ok() {
    const W: WindowIncrement = WindowIncrement::from_static(1024);
    assert_eq!(W.as_u32(), 1024);
}

#[test]
#[should_panic(expected = "increment must not be 0")]
fn window_increment_from_static_zero_panics() {
    let _ = WindowIncrement::from_static(0);
}

#[test]
#[should_panic(expected = "must be <= 2^31-1")]
fn window_increment_from_static_overflow_panics() {
    let _ = WindowIncrement::from_static(WindowIncrement::MAX + 1);
}

#[test]
fn weight_new_ok() {
    let w = Weight::new(0).expect("wire value 0 は有効");
    assert_eq!(w.as_wire(), 0);
    assert_eq!(w.weight_value(), 1);

    let w = Weight::new(255).expect("wire value 255 は有効");
    assert_eq!(w.as_wire(), 255);
    assert_eq!(w.weight_value(), 256);
}

#[test]
fn weight_new_out_of_range() {
    assert_eq!(
        Weight::new(256),
        Err(FrameError::InvalidWeight { value: 256 })
    );
}

#[test]
fn weight_from_static_ok() {
    const W: Weight = Weight::from_static(15);
    assert_eq!(W.as_wire(), 15);
    assert_eq!(W.weight_value(), 16);
}

#[test]
fn last_stream_id_new_ok() {
    let id = LastStreamId::new(0).expect("last stream id 0 は有効");
    assert_eq!(id.get(), 0);
    let id = LastStreamId::new(LastStreamId::MAX).expect("MAX last stream id は有効");
    assert_eq!(id.get(), LastStreamId::MAX);
}

#[test]
fn last_stream_id_new_out_of_range() {
    assert_eq!(
        LastStreamId::new(LastStreamId::MAX + 1),
        Err(FrameError::LastStreamIdOutOfRange {
            value: LastStreamId::MAX + 1
        })
    );
}

#[test]
fn last_stream_id_from_static_ok() {
    const ID: LastStreamId = LastStreamId::from_static(42);
    assert_eq!(ID.get(), 42);
}

#[test]
fn frame_error_display_zero_stream_id() {
    let err = FrameError::ZeroStreamIdNotAllowed {
        frame_type: FrameType::Data,
    };
    assert_eq!(err.to_string(), "DATA frame must not use stream ID 0");
}

#[test]
fn frame_error_display_non_zero_stream_id() {
    let err = FrameError::NonZeroStreamIdNotAllowed {
        frame_type: FrameType::Settings,
        stream_id: 5,
    };
    assert_eq!(
        err.to_string(),
        "SETTINGS frame requires stream ID 0 but got 5"
    );
}

#[test]
fn frame_error_display_zero_window_increment() {
    assert_eq!(
        FrameError::ZeroWindowIncrement.to_string(),
        "WINDOW_UPDATE increment must not be 0"
    );
}

#[test]
fn frame_error_display_window_overflow() {
    let err = FrameError::WindowIncrementOutOfRange { value: u32::MAX };
    assert!(err.to_string().contains("exceeds maximum"));
}

#[test]
fn frame_error_display_invalid_weight() {
    let err = FrameError::InvalidWeight { value: 1024 };
    assert_eq!(err.to_string(), "PRIORITY weight 1024 out of range 0..=255");
}

#[test]
fn frame_error_display_padding_exceeds_payload() {
    let err = FrameError::PaddingExceedsPayload {
        padding: 100,
        payload_len: 50,
    };
    assert_eq!(
        err.to_string(),
        "padding length 100 exceeds payload length 50"
    );
}

#[test]
fn frame_error_display_last_stream_id_out_of_range() {
    let err = FrameError::LastStreamIdOutOfRange { value: u32::MAX };
    assert!(err.to_string().contains("exceeds maximum"));
}
