use shiguredo_http2::webtransport::{Capsule, CapsuleDecoder, CapsuleEncoder};

// draft-ietf-webtrans-http2-16 Section 6.4:
// 非終端 WT_STREAM (FIN=0) の capsule type は 0x190B4D3C、
// 終端 WT_STREAM (FIN=1) の capsule type は 0x190B4D3B。
// LSB が FIN bit であり、0x3B の LSB=1 → FIN=1、0x3C の LSB=0 → FIN=0。

/// fin=true で encode した capsule type が varint 表現で 0x190B4D3B であることを直接検証する
#[test]
fn test_wt_stream_fin_wire_type() {
    let mut encoder = CapsuleEncoder::new();
    let capsule = Capsule::WtStream {
        stream_id: 0,
        data: vec![],
        fin: true,
    };
    encoder.encode(&capsule);

    // capsule type の varint 表現は先頭 4 バイト (0x190B4D3B は 4 バイト varint)
    // 0x190B4D3B = 0b10_011001_00001011_01001101_00111011 → [0x99, 0x0B, 0x4D, 0x3B]
    let buf = encoder.buffer();
    assert!(buf.len() >= 4, "encoded buffer too short");
    assert_eq!(
        &buf[0..4],
        &[0x99, 0x0B, 0x4D, 0x3B],
        "fin=true の capsule type は 0x190B4D3B でなければならない"
    );
}

/// fin=false で encode した capsule type が varint 表現で 0x190B4D3C であることを直接検証する
#[test]
fn test_wt_stream_non_fin_wire_type() {
    let mut encoder = CapsuleEncoder::new();
    let capsule = Capsule::WtStream {
        stream_id: 0,
        data: vec![],
        fin: false,
    };
    encoder.encode(&capsule);

    // 0x190B4D3C = 0b10_011001_00001011_01001101_00111100 → [0x99, 0x0B, 0x4D, 0x3C]
    let buf = encoder.buffer();
    assert!(buf.len() >= 4, "encoded buffer too short");
    assert_eq!(
        &buf[0..4],
        &[0x99, 0x0B, 0x4D, 0x3C],
        "fin=false の capsule type は 0x190B4D3C でなければならない"
    );
}

/// 空データ + fin=true の capsule が正しく encode/decode される
#[test]
fn test_wt_stream_empty_data_with_fin() {
    let mut encoder = CapsuleEncoder::new();
    let mut decoder = CapsuleDecoder::new();

    let capsule = Capsule::WtStream {
        stream_id: 4,
        data: vec![],
        fin: true,
    };
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("decode should succeed")
        .expect("capsule should exist");
    assert_eq!(capsule, decoded);
}

/// fin=false を複数送った後に fin=true を送るシーケンスが正しく encode/decode される
#[test]
fn test_wt_stream_sequence_non_fin_then_fin() {
    let mut encoder = CapsuleEncoder::new();
    let mut decoder = CapsuleDecoder::new();

    // 非終端 capsule を 3 個送った後に終端 capsule を送る
    let capsules = vec![
        Capsule::WtStream {
            stream_id: 4,
            data: b"chunk1".to_vec(),
            fin: false,
        },
        Capsule::WtStream {
            stream_id: 4,
            data: b"chunk2".to_vec(),
            fin: false,
        },
        Capsule::WtStream {
            stream_id: 4,
            data: b"chunk3".to_vec(),
            fin: false,
        },
        Capsule::WtStream {
            stream_id: 4,
            data: b"final".to_vec(),
            fin: true,
        },
    ];

    for capsule in &capsules {
        encoder.encode(capsule);
    }

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    for expected in &capsules {
        let decoded = decoder
            .decode()
            .expect("decode should succeed")
            .expect("capsule should exist");
        assert_eq!(expected, &decoded);
    }
    assert!(decoder.decode().expect("decode should succeed").is_none());
}

/// decode 側で 0x190B4D3B 受信時に fin=true になることを直接検証する
#[test]
fn test_decode_wt_stream_fin_from_raw_bytes() {
    let mut decoder = CapsuleDecoder::new();

    // capsule type = 0x190B4D3B (varint: [0x99, 0x0B, 0x4D, 0x3B])
    // capsule length = 1 (stream_id=0 の varint 1 バイト)
    // payload = stream_id=0 (varint: [0x00])
    let raw: &[u8] = &[0x99, 0x0B, 0x4D, 0x3B, 0x01, 0x00];
    decoder.feed(raw).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("decode should succeed")
        .expect("capsule should exist");
    assert_eq!(
        decoded,
        Capsule::WtStream {
            stream_id: 0,
            data: vec![],
            fin: true,
        },
        "0x190B4D3B は fin=true でデコードされなければならない"
    );
}

/// decode 側で 0x190B4D3C 受信時に fin=false になることを直接検証する
#[test]
fn test_decode_wt_stream_non_fin_from_raw_bytes() {
    let mut decoder = CapsuleDecoder::new();

    // capsule type = 0x190B4D3C (varint: [0x99, 0x0B, 0x4D, 0x3C])
    // capsule length = 1 (stream_id=0 の varint 1 バイト)
    // payload = stream_id=0 (varint: [0x00])
    let raw: &[u8] = &[0x99, 0x0B, 0x4D, 0x3C, 0x01, 0x00];
    decoder.feed(raw).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("decode should succeed")
        .expect("capsule should exist");
    assert_eq!(
        decoded,
        Capsule::WtStream {
            stream_id: 0,
            data: vec![],
            fin: false,
        },
        "0x190B4D3C は fin=false でデコードされなければならない"
    );
}

#[test]
fn test_encode_decode_datagram() {
    let mut encoder = CapsuleEncoder::new();
    let mut decoder = CapsuleDecoder::new();

    let capsule = Capsule::Datagram {
        data: b"hello".to_vec(),
    };
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule, decoded);
}

#[test]
fn test_encode_decode_wt_max_data() {
    let mut encoder = CapsuleEncoder::new();
    let mut decoder = CapsuleDecoder::new();

    let capsule = Capsule::WtMaxData { maximum: 1_000_000 };
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule, decoded);

    // Unidirectional
    encoder.clear();
    decoder.clear();

    let capsule = Capsule::WtMaxStreams {
        maximum: 50,
        bidirectional: false,
    };
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule, decoded);
}

#[test]
fn test_encode_decode_wt_drain_session() {
    let mut encoder = CapsuleEncoder::new();
    let mut decoder = CapsuleDecoder::new();

    let capsule = Capsule::WtDrainSession;
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule, decoded);
}

#[test]
fn test_encode_decode_padding() {
    let mut encoder = CapsuleEncoder::new();
    let mut decoder = CapsuleDecoder::new();

    let capsule = Capsule::Padding { length: 100 };
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule, decoded);
}

#[test]
fn test_decode_incomplete() {
    let mut decoder = CapsuleDecoder::new();

    // 不完全なデータ
    decoder.feed(&[0x00]).expect("feed should succeed"); // DATAGRAM type only
    assert!(decoder.decode().expect("feed should succeed").is_none());

    decoder.clear();

    // Type + Length のみ
    decoder.feed(&[0x00, 0x05]).expect("feed should succeed"); // DATAGRAM, length=5
    assert!(decoder.decode().expect("feed should succeed").is_none());
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");

    let decoded1 = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule1, decoded1);

    let decoded2 = decoder
        .decode()
        .expect("decode should succeed")
        .expect("decode should succeed");
    assert_eq!(capsule2, decoded2);

    assert!(decoder.decode().expect("decode should succeed").is_none());
}

/// 各バイトで feed を分割し、ヘッダーとペイロードをまたいだ入力を確認する。
#[test]
fn test_feed_split_capsule_after_consuming_prefix() {
    let prefix = Capsule::Datagram { data: vec![1; 128] };
    let capsule_type = 1_u64 << 32;
    let data: Vec<u8> = (0..64).collect();
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&prefix);
    let encoded_prefix = encoder.take();

    // RFC 9297 Section 1.1 は非最小の varint を許容するため、短いデータで
    // Type と Length の両方が 8 バイトになるヘッダーを検証できる。
    const VARINT_8_BYTE_TAG: u64 = 0b11 << 62;
    let mut encoded = (VARINT_8_BYTE_TAG | capsule_type).to_be_bytes().to_vec();
    encoded.extend_from_slice(&(VARINT_8_BYTE_TAG | data.len() as u64).to_be_bytes());
    encoded.extend_from_slice(&data);
    let capsule = Capsule::Unknown { capsule_type, data };

    for split in 1..encoded.len() {
        // 上限まで入力して先頭だけを消費し、次の Capsule の一部を残す。
        // 後半の feed では、消費済みのバイトを上限に数えないことを確認する。
        let mut input = encoded_prefix.clone();
        input.extend_from_slice(&encoded[..split]);
        let mut decoder = CapsuleDecoder::with_max_buffer_size(input.len());
        decoder.feed(&input).expect("初回の feed に失敗した");
        assert_eq!(
            decoder.decode().expect("先頭のデコードに失敗した"),
            Some(prefix.clone())
        );
        assert_eq!(decoder.remaining(), split);
        assert_eq!(
            decoder.decode().expect("分割した入力のデコードに失敗した"),
            None
        );
        assert_eq!(decoder.remaining(), split);

        decoder
            .feed(&encoded[split..])
            .expect("消費済みの領域に後半を追加できなかった");
        assert_eq!(decoder.remaining(), encoded.len());
        assert_eq!(
            decoder.decode().expect("全体のデコードに失敗した"),
            Some(capsule.clone())
        );
        assert_eq!(decoder.remaining(), 0);

        decoder
            .feed(&encoded[..split])
            .expect("次の feed に失敗した");
        decoder.clear();
        assert_eq!(decoder.remaining(), 0);
        assert_eq!(
            decoder.decode().expect("クリア後のデコードに失敗した"),
            None
        );
    }
}

/// 入力サイズを増やしても、既にデコードした Capsule の削除が二乗時間にならないことを確認する。
#[test]
fn test_decode_batch_scales_with_input_size() {
    use std::time::Instant;

    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&Capsule::Datagram { data: Vec::new() });
    let capsule = encoder.buffer();
    assert_eq!(capsule.len(), 2);

    let [small, medium, large] = [16, 64, 256].map(|kib| {
        let capsule_count = kib * 1024 / capsule.len();
        let input = capsule.repeat(capsule_count);
        // feed を計測から外し、スケジューリングによる一時的な遅延の影響を抑える。
        (0..5)
            .map(|_| {
                let mut decoder = CapsuleDecoder::new();
                decoder.feed(&input).expect("計測用の feed に失敗した");
                let start = Instant::now();
                let mut decoded = 0;
                while decoder
                    .decode()
                    .expect("バッチのデコードに失敗した")
                    .is_some()
                {
                    decoded += 1;
                }
                let elapsed = start.elapsed();
                assert_eq!(decoded, capsule_count);
                elapsed
            })
            .min()
            .expect("デコード時間を計測できなかった")
    });
    assert!(
        large <= small * 64,
        "デコード時間が入力サイズに対して大きすぎる (16 KiB: {small:?}, 64 KiB: {medium:?}, 256 KiB: {large:?})"
    );
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule, decoded);
}

#[test]
fn test_decode_wt_data_blocked() {
    let mut encoder = CapsuleEncoder::new();
    let mut decoder = CapsuleDecoder::new();

    let capsule = Capsule::WtDataBlocked { maximum: 65536 };
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
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

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule, decoded);

    // Unidirectional
    encoder.clear();
    decoder.clear();

    let capsule = Capsule::WtStreamsBlocked {
        maximum: 5,
        bidirectional: false,
    };
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer()).expect("feed should succeed");
    let decoded = decoder
        .decode()
        .expect("feed should succeed")
        .expect("feed should succeed");
    assert_eq!(capsule, decoded);
}

/// Application Protocol Error Code が 0xffffffff を超える WT_RESET_STREAM は
/// セッションエラー (WT_ERROR 相当 = SessionStateError) になること
///
/// draft-ietf-webtrans-http2-16 Section 6.2
#[test]
fn test_wt_reset_stream_error_code_exceeds_u32_is_session_error() {
    use shiguredo_http2::webtransport::{WtErrorKind, capsule_type, varint_encode};

    // Type + Length + Stream ID(0) + Error Code(0x1_0000_0000) + Reliable Size(0)
    let mut payload = Vec::new();
    let mut buf = [0u8; 8];
    let n = varint_encode(0, &mut buf).expect("encode stream_id");
    payload.extend_from_slice(&buf[..n]);
    let n = varint_encode(0x1_0000_0000, &mut buf).expect("encode error_code");
    payload.extend_from_slice(&buf[..n]);
    let n = varint_encode(0, &mut buf).expect("encode reliable_size");
    payload.extend_from_slice(&buf[..n]);

    let mut wire = Vec::new();
    let n = varint_encode(capsule_type::WT_RESET_STREAM, &mut buf).expect("encode type");
    wire.extend_from_slice(&buf[..n]);
    let n = varint_encode(payload.len() as u64, &mut buf).expect("encode length");
    wire.extend_from_slice(&buf[..n]);
    wire.extend_from_slice(&payload);

    let mut decoder = CapsuleDecoder::new();
    decoder.feed(&wire).expect("feed should succeed");
    let err = decoder
        .decode()
        .expect_err("0xffffffff 超過の error_code はエラーになるはず");
    assert_eq!(decoder.remaining(), wire.len());
    assert_eq!(
        err.kind(),
        WtErrorKind::SessionStateError,
        "WT_ERROR 相当として SessionStateError であること、実際: {err}"
    );
}

/// Application Protocol Error Code が 0xffffffff を超える WT_STOP_SENDING は
/// セッションエラー (WT_ERROR 相当 = SessionStateError) になること
///
/// draft-ietf-webtrans-http2-16 Section 6.3
#[test]
fn test_wt_stop_sending_error_code_exceeds_u32_is_session_error() {
    use shiguredo_http2::webtransport::{WtErrorKind, capsule_type, varint_encode};

    let mut payload = Vec::new();
    let mut buf = [0u8; 8];
    let n = varint_encode(0, &mut buf).expect("encode stream_id");
    payload.extend_from_slice(&buf[..n]);
    let n = varint_encode(0x1_0000_0000, &mut buf).expect("encode error_code");
    payload.extend_from_slice(&buf[..n]);

    let mut wire = Vec::new();
    let n = varint_encode(capsule_type::WT_STOP_SENDING, &mut buf).expect("encode type");
    wire.extend_from_slice(&buf[..n]);
    let n = varint_encode(payload.len() as u64, &mut buf).expect("encode length");
    wire.extend_from_slice(&buf[..n]);
    wire.extend_from_slice(&payload);

    let mut decoder = CapsuleDecoder::new();
    decoder.feed(&wire).expect("feed should succeed");
    let err = decoder
        .decode()
        .expect_err("0xffffffff 超過の error_code はエラーになるはず");
    assert_eq!(
        err.kind(),
        WtErrorKind::SessionStateError,
        "WT_ERROR 相当として SessionStateError であること、実際: {err}"
    );
}

/// バッファ上限超過時に feed がエラーを返すことを確認する
#[test]
fn test_feed_buffer_limit_exceeded() {
    use shiguredo_http2::webtransport::WtErrorKind;

    // 小さな上限を指定してデコーダーを生成
    let mut decoder = CapsuleDecoder::with_max_buffer_size(10);

    // 上限以内は成功
    decoder.feed(&[0u8; 10]).expect("feed should succeed");

    // 上限超過はエラー
    let err = decoder.feed(&[0u8; 1]).unwrap_err();
    assert_eq!(err.kind(), WtErrorKind::InvalidInput);
    assert!(err.reason().contains("buffer limit exceeded"));
}

/// デフォルトのバッファ上限 (16 MiB) が適用されることを確認する
#[test]
fn test_feed_default_buffer_limit() {
    use shiguredo_http2::webtransport::WtErrorKind;

    let mut decoder = CapsuleDecoder::new();

    // 16 MiB 以内は成功
    let data = vec![0u8; 16 * 1024 * 1024];
    decoder.feed(&data).expect("feed should succeed");

    // 1 バイトでも超過するとエラー
    let err = decoder.feed(&[0u8; 1]).unwrap_err();
    assert_eq!(err.kind(), WtErrorKind::InvalidInput);
}
