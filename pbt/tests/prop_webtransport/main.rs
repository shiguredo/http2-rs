//! WebTransport の PBT
//!
//! Capsule Protocol と WtSession の状態遷移をテストする。

use proptest::prelude::*;
use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, CapsuleEncoder, MAX_VALUE, WtConfig, WtSession, WtSessionState,
    WtStream, encoded_len, varint_decode, varint_encode,
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
        let encoded_len_result = varint_encode(value, &mut buf).expect("should succeed");
        prop_assert_eq!(encoded_len_result, encoded_len(value));

        let (decoded, consumed) = varint_decode(&buf[..encoded_len_result]).expect("should succeed");
        prop_assert_eq!(decoded, value);
        prop_assert_eq!(consumed, encoded_len_result);
    }

    /// varint エンコード長テスト (RFC 9000 Section 16 Table 4 の境界値)
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_MAX_DATA Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_max_data_roundtrip(maximum in small_varint_value()) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxData { maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");

        for expected in &capsules {
            let decoded = decoder.decode().expect("decode should succeed").expect("decode should succeed");
            prop_assert_eq!(expected, &decoded);
        }

        // これ以上 Capsule はない
        prop_assert!(decoder.decode().expect("decode should succeed").is_none());
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
        prop_assert_eq!(capsule, decoded);
    }

    /// PADDING Capsule 往復テスト
    #[test]
    fn prop_capsule_padding_roundtrip(length in 0..1000usize) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Padding { length };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
        prop_assert_eq!(capsule, decoded);
    }

    /// WT_DATA_BLOCKED Capsule 往復テスト
    #[test]
    fn prop_capsule_wt_data_blocked_roundtrip(maximum in small_varint_value()) {
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtDataBlocked { maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
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

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");
        prop_assert_eq!(capsule, decoded);
    }

    /// 余剰バイト付き Capsule はデコードエラーになる (RFC 9297 Section 3.3)
    #[test]
    fn prop_capsule_trailing_bytes_rejected(
        maximum in small_varint_value(),
        trailing in prop::collection::vec(any::<u8>(), 1..=8),
    ) {
        // WT_MAX_DATA を低レベルで構築し、余剰バイトを付加

        // capsule header (type + length) の後の payload に余剰バイトを追加
        // length フィールドを手動で修正する必要があるので、低レベルで構築する
        let mut buf = Vec::new();
        // Type: WT_MAX_DATA
        let type_len = encoded_len(0x190B4D3D);
        buf.resize(type_len, 0);
        varint_encode(0x190B4D3D, &mut buf).expect("should succeed");

        // payload: varint(maximum) + trailing bytes
        let max_len = encoded_len(maximum);
        let payload_total = max_len + trailing.len();
        let len_start = buf.len();
        let len_len = encoded_len(payload_total as u64);
        buf.resize(len_start + len_len, 0);
        varint_encode(payload_total as u64, &mut buf[len_start..]).expect("should succeed");

        // maximum の varint
        let val_start = buf.len();
        buf.resize(val_start + max_len, 0);
        varint_encode(maximum, &mut buf[val_start..]).expect("should succeed");

        // trailing bytes
        buf.extend_from_slice(&trailing);

        // WT_MAX_DATA は varint 1 つだけなので余剰バイトがあればエラー
        let mut decoder = CapsuleDecoder::new();
        decoder.feed(&buf).expect("feed should succeed");
        prop_assert!(decoder.decode().is_err());
    }

    /// WT ストリーム送信フロー制御: 上限超過はエラー
    #[test]
    fn prop_wt_stream_send_flow_control(
        max_data in 1u64..=10000,
        send_size in 1u64..=20000,
    ) {
        let mut stream = WtStream::new(0, max_data, max_data, true, true);

        let result = stream.send_data(send_size, false);
        if send_size <= max_data {
            prop_assert!(result.is_ok());
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// WT ストリーム受信フロー制御: 上限超過はエラー
    #[test]
    fn prop_wt_stream_recv_flow_control(
        max_data in 1u64..=10000,
        recv_size in 1u64..=20000,
    ) {
        let mut stream = WtStream::new(0, max_data, max_data, true, true);

        let result = stream.recv_data(recv_size, false);
        if recv_size <= max_data {
            prop_assert!(result.is_ok());
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// WT ストリームフロー制御: 複数回の送信で累積が上限を超えるとエラー
    #[test]
    fn prop_wt_stream_cumulative_send_flow_control(
        max_data in 100u64..=1000,
        chunk1 in 1u64..=500,
        chunk2 in 1u64..=500,
    ) {
        let mut stream = WtStream::new(0, max_data, max_data, true, true);

        if chunk1 <= max_data {
            let r1 = stream.send_data(chunk1, false);
            prop_assert!(r1.is_ok());

            let r2 = stream.send_data(chunk2, false);
            if chunk1 + chunk2 <= max_data {
                prop_assert!(r2.is_ok());
            } else {
                prop_assert!(r2.is_err());
            }
        }
    }

    // ========================================================================
    // WtSession 状態遷移の真の PBT
    // ========================================================================
}

/// セッション操作
#[derive(Debug, Clone)]
enum SessionOp {
    /// セッション初期化
    Initiate,
    /// セッションドレイン
    Drain,
    /// セッションクローズ
    Close,
    /// 双方向ストリームを開く
    OpenBidiStream,
    /// 単方向ストリームを開く
    OpenUniStream,
    /// データグラム送信
    SendDatagram(Vec<u8>),
    /// WT_CLOSE_SESSION Capsule 受信
    RecvCloseSession,
    /// WT_DRAIN_SESSION Capsule 受信
    RecvDrainSession,
}

/// セッション操作の Strategy
fn session_op() -> impl Strategy<Value = SessionOp> {
    prop_oneof![
        Just(SessionOp::Initiate),
        Just(SessionOp::Drain),
        Just(SessionOp::Close),
        Just(SessionOp::OpenBidiStream),
        Just(SessionOp::OpenUniStream),
        prop::collection::vec(any::<u8>(), 0..50).prop_map(SessionOp::SendDatagram),
        Just(SessionOp::RecvCloseSession),
        Just(SessionOp::RecvDrainSession),
    ]
}

/// 操作を適用する (エラーは無視)
fn apply_session_op(session: &mut WtSession, op: &SessionOp) -> Result<(), ()> {
    match op {
        SessionOp::Initiate => session.initiate().map_err(|_| ()),
        SessionOp::Drain => session.drain().map_err(|_| ()),
        SessionOp::Close => session.close(0, "close").map_err(|_| ()),
        SessionOp::OpenBidiStream => session.open_bidi_stream().map(|_| ()).map_err(|_| ()),
        SessionOp::OpenUniStream => session.open_uni_stream().map(|_| ()).map_err(|_| ()),
        SessionOp::SendDatagram(data) => session.send_datagram(data).map_err(|_| ()),
        SessionOp::RecvCloseSession => {
            let mut encoder = CapsuleEncoder::new();
            encoder.encode(&Capsule::WtCloseSession {
                error_code: 0,
                reason: "test".to_string(),
            });
            session.feed(encoder.buffer()).map_err(|_| ())?;
            session.process().map_err(|_| ())
        }
        SessionOp::RecvDrainSession => {
            let mut encoder = CapsuleEncoder::new();
            encoder.encode(&Capsule::WtDrainSession);
            session.feed(encoder.buffer()).map_err(|_| ())?;
            session.process().map_err(|_| ())
        }
    }
}

proptest! {
    /// セッション状態の不変条件: 状態は常に有効な WtSessionState の 1 つ
    ///
    /// 数学的意義: 状態空間の閉包性
    #[test]
    fn prop_session_state_always_valid(
        is_client in any::<bool>(),
        ops in prop::collection::vec(session_op(), 0..30),
    ) {
        let mut session = if is_client {
            WtSession::client(WtConfig::default(), WtConfig::default())
        } else {
            WtSession::server(WtConfig::default(), WtConfig::default())
        };

        for op in &ops {
            let _ = apply_session_op(&mut session, op);

            // 不変条件: 状態は常に有効な値
            let state = session.state();
            let is_valid_state = matches!(
                state,
                WtSessionState::Initial
                    | WtSessionState::Active
                    | WtSessionState::Draining
                    | WtSessionState::Closed
            );
            prop_assert!(is_valid_state, "Invalid session state after {:?}", op);

            // 不変条件: is_active() と is_closed() は状態と整合する
            prop_assert_eq!(session.is_active(), state == WtSessionState::Active);
            prop_assert_eq!(session.is_closed(), state == WtSessionState::Closed);
        }
    }

    /// Closed 状態は吸収状態
    ///
    /// 数学的意義: 終状態からは遷移しない
    #[test]
    fn prop_session_closed_is_absorbing(
        is_client in any::<bool>(),
        ops_before in prop::collection::vec(session_op(), 0..10),
        ops_after in prop::collection::vec(session_op(), 1..10),
    ) {
        let mut session = if is_client {
            WtSession::client(WtConfig::default(), WtConfig::default())
        } else {
            WtSession::server(WtConfig::default(), WtConfig::default())
        };

        // 操作を適用
        for op in &ops_before {
            let _ = apply_session_op(&mut session, op);
        }

        // Closed でなければ強制的に Closed にする
        if !session.is_closed() {
            let _ = session.initiate();
            let _ = session.close(0, "force close");
        }

        if session.is_closed() {
            // Closed 後の操作は状態を変えない
            for op in &ops_after {
                let _ = apply_session_op(&mut session, op);
                prop_assert!(
                    session.is_closed(),
                    "Closed state should be absorbing, but changed after {:?}",
                    op
                );
            }
        }
    }

    /// 状態遷移の単調性: Initial -> Active -> Draining -> Closed
    ///
    /// 数学的意義: 状態遷移グラフの有向非巡回性
    #[test]
    fn prop_session_state_monotonicity(
        is_client in any::<bool>(),
        ops in prop::collection::vec(session_op(), 0..30),
    ) {
        let mut session = if is_client {
            WtSession::client(WtConfig::default(), WtConfig::default())
        } else {
            WtSession::server(WtConfig::default(), WtConfig::default())
        };

        // 状態の順序を定義
        fn state_order(state: WtSessionState) -> u8 {
            match state {
                WtSessionState::Initial => 0,
                WtSessionState::Active => 1,
                WtSessionState::Draining => 2,
                WtSessionState::Closed => 3,
            }
        }

        let mut max_order = state_order(session.state());

        for op in &ops {
            let _ = apply_session_op(&mut session, op);

            let current_order = state_order(session.state());

            // 不変条件: 状態は前に戻らない (Draining から Active には戻らない等)
            // ただし、Active/Draining -> Closed は許可
            // また、同じ状態にとどまることも許可
            if current_order < max_order && session.state() != WtSessionState::Closed {
                // Active に留まることは許可 (Draining にならない限り)
                if !(current_order == 1 && max_order == 1) {
                    prop_assert!(
                        false,
                        "State went backwards from order {} to {} after {:?}",
                        max_order,
                        current_order,
                        op
                    );
                }
            }
            max_order = max_order.max(current_order);
        }
    }

    /// ストリーム ID の単調増加の不変条件
    ///
    /// 数学的意義: ストリーム ID 生成の単射性
    #[test]
    fn prop_stream_id_monotonic_invariant(
        is_client in any::<bool>(),
        ops in prop::collection::vec(
            prop_oneof![
                Just(true),  // bidi
                Just(false), // uni
            ],
            1..20
        ),
    ) {
        let mut session = if is_client {
            WtSession::client(WtConfig::default(), WtConfig::default())
        } else {
            WtSession::server(WtConfig::default(), WtConfig::default())
        };
        session.initiate().expect("initiate should succeed");

        let mut bidi_ids: Vec<u64> = Vec::new();
        let mut uni_ids: Vec<u64> = Vec::new();

        for is_bidi in ops {
            let result = if is_bidi {
                session.open_bidi_stream()
            } else {
                session.open_uni_stream()
            };

            if let Ok(id) = result {
                if is_bidi {
                    // 不変条件: すべての bidi ID は異なり、単調増加
                    if let Some(&last) = bidi_ids.last() {
                        prop_assert!(id > last, "bidi stream ID not monotonically increasing");
                    }
                    prop_assert!(
                        !bidi_ids.contains(&id),
                        "duplicate bidi stream ID"
                    );
                    bidi_ids.push(id);
                } else {
                    // 不変条件: すべての uni ID は異なり、単調増加
                    if let Some(&last) = uni_ids.last() {
                        prop_assert!(id > last, "uni stream ID not monotonically increasing");
                    }
                    prop_assert!(
                        !uni_ids.contains(&id),
                        "duplicate uni stream ID"
                    );
                    uni_ids.push(id);
                }
            }
        }
    }

    /// Capsule のラウンドトリップ: encode -> decode -> encode == encode
    ///
    /// 数学的意義: エンコード・デコードの同型性
    #[test]
    fn prop_capsule_encode_decode_isomorphism(
        stream_id in small_varint_value(),
        data in prop::collection::vec(any::<u8>(), 0..100),
        fin in any::<bool>(),
    ) {
        let original = Capsule::WtStream {
            stream_id,
            data: data.clone(),
            fin,
        };

        // エンコード
        let mut encoder = CapsuleEncoder::new();
        encoder.encode(&original);
        let encoded1 = encoder.buffer().to_vec();

        // デコード
        let mut decoder = CapsuleDecoder::new();
        decoder.feed(&encoded1).expect("feed should succeed");
        let decoded = decoder.decode().expect("feed should succeed").expect("feed should succeed");

        // 再エンコード
        let mut encoder2 = CapsuleEncoder::new();
        encoder2.encode(&decoded);
        let encoded2 = encoder2.buffer().to_vec();

        // 不変条件: encode(decode(encode(x))) == encode(x)
        prop_assert_eq!(encoded1, encoded2);
        prop_assert_eq!(original, decoded);
    }
}
