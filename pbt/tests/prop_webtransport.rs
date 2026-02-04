//! WebTransport の PBT

use proptest::prelude::*;
use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, CapsuleEncoder, MAX_VALUE, encoded_len, varint_decode, varint_encode,
};

/// varint の有効な値を生成する
fn valid_varint_value() -> impl Strategy<Value = u64> {
    0..=MAX_VALUE
}

/// 小さい varint 値 (1-2 バイト)
fn small_varint_value() -> impl Strategy<Value = u64> {
    0..=16383u64
}

proptest! {
    /// varint エンコード/デコード往復テスト
    #[test]
    fn prop_varint_roundtrip(value in valid_varint_value()) {
        let mut buf = [0u8; 8];
        let encoded_len_result = varint_encode(value, &mut buf).unwrap();
        prop_assert_eq!(encoded_len_result, encoded_len(value));

        let (decoded, consumed) = varint_decode(&buf[..encoded_len_result]).unwrap();
        prop_assert_eq!(decoded, value);
        prop_assert_eq!(consumed, encoded_len_result);
    }

    /// varint エンコード長テスト
    #[test]
    fn prop_varint_encoded_len(value in valid_varint_value()) {
        let len = encoded_len(value);

        // 値の範囲に応じたエンコード長
        if value <= 63 {
            prop_assert_eq!(len, 1);
        } else if value <= 16383 {
            prop_assert_eq!(len, 2);
        } else if value <= 1073741823 {
            prop_assert_eq!(len, 4);
        } else {
            prop_assert_eq!(len, 8);
        }
    }

    /// DATAGRAM Capsule 往復テスト
    #[test]
    fn prop_capsule_datagram_roundtrip(data in prop::collection::vec(any::<u8>(), 0..1000)) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Datagram { data: data.clone() };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_STREAM Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_stream_roundtrip(
        stream_id in small_varint_value(),
        data in prop::collection::vec(any::<u8>(), 0..500),
        fin in any::<bool>(),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStream {
            stream_id,
            data: data.clone(),
            fin,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_RESET_STREAM Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_reset_stream_roundtrip(
        stream_id in small_varint_value(),
        error_code in small_varint_value(),
        reliable_size in small_varint_value(),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtResetStream {
            stream_id,
            error_code,
            reliable_size,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_STOP_SENDING Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_stop_sending_roundtrip(
        stream_id in small_varint_value(),
        error_code in small_varint_value(),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStopSending {
            stream_id,
            error_code,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_MAX_DATA Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_max_data_roundtrip(maximum in small_varint_value()) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxData { maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_MAX_STREAM_DATA Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_max_stream_data_roundtrip(
        stream_id in small_varint_value(),
        maximum in small_varint_value(),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxStreamData { stream_id, maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_MAX_STREAMS Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_max_streams_roundtrip(
        maximum in small_varint_value(),
        bidirectional in any::<bool>(),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxStreams { maximum, bidirectional };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_CLOSE_SESSION Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_close_session_roundtrip(
        error_code in any::<u32>(),
        reason in "[a-zA-Z0-9 ]{0,100}",
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtCloseSession {
            error_code,
            reason: reason.clone(),
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// 複数 Capsule の連続デコードテスト
    #[test]
    fn prop_multiple_capsules_roundtrip(
        count in 1..10usize,
        data_sizes in prop::collection::vec(0..100usize, 1..10),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let mut capsules: Vec<Capsule> = Vec::new();
        for (i, &size) in data_sizes.iter().enumerate().take(count) {
            let data = vec![i as u8; size];
            let capsule = Capsule::Datagram { data };
            encoder.encode(&capsule);
            capsules.push(capsule);
        }

        decoder.feed(encoder.buffer());

        for expected in &capsules {
            let decoded = decoder.decode().unwrap().unwrap();
            prop_assert_eq!(expected, &decoded);
        }

        // これ以上 Capsule はない
        prop_assert!(decoder.decode().unwrap().is_none());
    }

    /// 不完全データでのデコード安全性テスト
    #[test]
    fn prop_decode_incomplete_safe(
        data in prop::collection::vec(any::<u8>(), 0..50),
    ) {
        let mut decoder = CapsuleDecoder::new();
        decoder.feed(&data);

        // デコードを試みる - panic しないことを確認
        // 成功するかもしれないし、失敗するかもしれないが、panic はしない
        let _ = decoder.decode();
    }

    /// Unknown Capsule タイプの往復テスト
    #[test]
    fn prop_unknown_capsule_roundtrip(
        capsule_type_value in 0x1000000u64..0x2000000u64, // 未知のタイプ
        data in prop::collection::vec(any::<u8>(), 0..100),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Unknown {
            capsule_type: capsule_type_value,
            data: data.clone(),
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// PADDING Capsule 往復テスト
    #[test]
    fn prop_capsule_padding_roundtrip(length in 0..1000usize) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Padding { length };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_DATA_BLOCKED Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_data_blocked_roundtrip(maximum in small_varint_value()) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtDataBlocked { maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_STREAM_DATA_BLOCKED Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_stream_data_blocked_roundtrip(
        stream_id in small_varint_value(),
        maximum in small_varint_value(),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStreamDataBlocked { stream_id, maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_STREAMS_BLOCKED Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_streams_blocked_roundtrip(
        maximum in small_varint_value(),
        bidirectional in any::<bool>(),
    ) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStreamsBlocked { maximum, bidirectional };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer());
        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(capsule, decoded);
    }
}
