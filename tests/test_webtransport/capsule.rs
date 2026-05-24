use shiguredo_http2::webtransport::{Capsule, CapsuleDecoder, CapsuleEncoder};

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
