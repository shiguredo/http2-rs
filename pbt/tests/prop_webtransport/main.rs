//! WebTransport の PBT
//!
//! Capsule Protocol と WtSession の状態遷移をテストする。

use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, CapsuleEncoder, MAX_VALUE, WtConfig, WtSession, WtSessionState,
    WtStream, encoded_len, varint_decode, varint_encode,
};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// RFC 9000 Section 16 Table 4: 1 バイトで表せる最大値
const MAX_1_BYTE: u64 = 63;
/// RFC 9000 Section 16 Table 4: 2 バイトで表せる最大値
const MAX_2_BYTES: u64 = 16_383;
/// RFC 9000 Section 16 Table 4: 4 バイトで表せる最大値
const MAX_4_BYTES: u64 = 1_073_741_823;

/// 0..=max_len の長さを、空・1・上限に 1/5 の確率を付けて引く
fn sample_len(ctx: &mut noprop::TestCaseContext, max_len: usize) -> usize {
    match max_len {
        0 => 0,
        1 => noprop::sample_with_boundaries(ctx, &[0usize, 1], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, 0..=1)
        }),
        _ => noprop::sample_with_boundaries(
            ctx,
            &[0usize, 1, max_len],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, 0..=max_len),
        ),
    }
}

/// varint の有効な値を、エンコード長クラスを等確率で選んで生成する
///
/// 0..=MAX_VALUE の一様では 8 バイト級 (2^30 超) がほぼ全体を占め、
/// 1/2/4 バイト級は N=256 でも未到達になりうる。4 クラス等確率なら
/// 各クラスの未到達確率は (3/4)^256 ≈ 1.3e-32。
fn sample_valid_varint_value(ctx: &mut noprop::TestCaseContext) -> u64 {
    match noprop::sample_weighted_index(ctx, &[1, 1, 1, 1]) {
        0 => noprop::sample_with_boundaries(
            ctx,
            &[0u64, MAX_1_BYTE],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_u64_in(ctx, 0..=MAX_1_BYTE),
        ),
        1 => noprop::sample_with_boundaries(
            ctx,
            &[MAX_1_BYTE + 1, MAX_2_BYTES],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_u64_in(ctx, MAX_1_BYTE + 1..=MAX_2_BYTES),
        ),
        2 => noprop::sample_with_boundaries(
            ctx,
            &[MAX_2_BYTES + 1, MAX_4_BYTES],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_u64_in(ctx, MAX_2_BYTES + 1..=MAX_4_BYTES),
        ),
        _ => noprop::sample_with_boundaries(
            ctx,
            &[MAX_4_BYTES + 1, MAX_VALUE],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_u64_in(ctx, MAX_4_BYTES + 1..=MAX_VALUE),
        ),
    }
}

/// 小さい varint 値 (0..=16383、1-2 バイト) を生成する
fn sample_small_varint_value(ctx: &mut noprop::TestCaseContext) -> u64 {
    noprop::sample_with_boundaries(
        ctx,
        &[0u64, MAX_1_BYTE, MAX_1_BYTE + 1, MAX_2_BYTES],
        noprop::Ratio::one_nth(5),
        |ctx| noprop::sample_u64_in(ctx, 0..=MAX_2_BYTES),
    )
}

/// 任意のバイト列 (0..=max_len) を生成する
fn sample_arbitrary_bytes(ctx: &mut noprop::TestCaseContext, max_len: usize) -> Vec<u8> {
    let len = sample_len(ctx, max_len);
    noprop::sample_bytes_vec(ctx, len)
}

/// 英数字とスペースのみの reason (0..=max_len) を生成する
fn sample_reason(ctx: &mut noprop::TestCaseContext, max_len: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";
    let len = sample_len(ctx, max_len);
    (0..len)
        .map(|_| noprop::sample_choice(ctx, CHARSET) as char)
        .collect()
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

/// セッション操作を 1 つ生成する
///
/// Initiate は Active 以降の状態への前提操作なので重みを 3 にする。
fn sample_session_op(ctx: &mut noprop::TestCaseContext) -> SessionOp {
    match noprop::sample_weighted_index(ctx, &[3, 1, 1, 1, 1, 2, 1, 1]) {
        0 => SessionOp::Initiate,
        1 => SessionOp::Drain,
        2 => SessionOp::Close,
        3 => SessionOp::OpenBidiStream,
        4 => SessionOp::OpenUniStream,
        5 => SessionOp::SendDatagram(sample_arbitrary_bytes(ctx, 50)),
        6 => SessionOp::RecvCloseSession,
        _ => SessionOp::RecvDrainSession,
    }
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

/// varint エンコード/デコード往復テスト
#[test]
fn prop_varint_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = sample_valid_varint_value(ctx);
        let mut buf = [0u8; 8];
        let encoded_len_result = varint_encode(value, &mut buf).expect("should succeed");
        assert_eq!(encoded_len_result, encoded_len(value));

        let (decoded, consumed) =
            varint_decode(&buf[..encoded_len_result]).expect("should succeed");
        assert_eq!(decoded, value);
        assert_eq!(consumed, encoded_len_result);
        Ok(())
    })?;
    Ok(())
}

/// varint エンコード長テスト (RFC 9000 Section 16 Table 4 の境界値)
///
/// 4 クラス等確率 (各 1/4)。N=256 で各クラス未到達は (3/4)^256 ≈ 1.3e-32。
#[test]
fn prop_varint_encoded_len() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let len1_gate = std::cell::Cell::new(0usize);
    let len2_gate = std::cell::Cell::new(0usize);
    let len4_gate = std::cell::Cell::new(0usize);
    let len8_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = sample_valid_varint_value(ctx);
        let len = encoded_len(value);

        // 値の範囲に応じたエンコード長
        if value <= MAX_1_BYTE {
            assert_eq!(len, 1);
            len1_gate.set(len1_gate.get() + 1);
        } else if value <= MAX_2_BYTES {
            assert_eq!(len, 2);
            len2_gate.set(len2_gate.get() + 1);
        } else if value <= MAX_4_BYTES {
            assert_eq!(len, 4);
            len4_gate.set(len4_gate.get() + 1);
        } else {
            assert_eq!(len, 8);
            len8_gate.set(len8_gate.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        len1_gate.get() > 0,
        "1 バイト varint が一度も実行されなかった\n{runner}"
    );
    assert!(
        len2_gate.get() > 0,
        "2 バイト varint が一度も実行されなかった\n{runner}"
    );
    assert!(
        len4_gate.get() > 0,
        "4 バイト varint が一度も実行されなかった\n{runner}"
    );
    assert!(
        len8_gate.get() > 0,
        "8 バイト varint が一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// DATAGRAM Capsule 往復テスト
#[test]
fn prop_capsule_datagram_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_arbitrary_bytes(ctx, 1000);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Datagram { data: data.clone() };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_STREAM Capsule 往復テスト
#[test]
fn prop_capsule_wt_stream_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_small_varint_value(ctx);
        let data = sample_arbitrary_bytes(ctx, 500);
        let fin = noprop::sample_bool(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStream {
            stream_id,
            data: data.clone(),
            fin,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_RESET_STREAM Capsule 往復テスト
#[test]
fn prop_capsule_wt_reset_stream_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_small_varint_value(ctx);
        let error_code = sample_small_varint_value(ctx);
        let reliable_size = sample_small_varint_value(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtResetStream {
            stream_id,
            error_code,
            reliable_size,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_STOP_SENDING Capsule 往復テスト
#[test]
fn prop_capsule_wt_stop_sending_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_small_varint_value(ctx);
        let error_code = sample_small_varint_value(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStopSending {
            stream_id,
            error_code,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_MAX_DATA Capsule 往復テスト
#[test]
fn prop_capsule_wt_max_data_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let maximum = sample_small_varint_value(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxData { maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_MAX_STREAM_DATA Capsule 往復テスト
#[test]
fn prop_capsule_wt_max_stream_data_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_small_varint_value(ctx);
        let maximum = sample_small_varint_value(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxStreamData { stream_id, maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_MAX_STREAMS Capsule 往復テスト
#[test]
fn prop_capsule_wt_max_streams_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let maximum = sample_small_varint_value(ctx);
        let bidirectional = noprop::sample_bool(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtMaxStreams {
            maximum,
            bidirectional,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_CLOSE_SESSION Capsule 往復テスト
#[test]
fn prop_capsule_wt_close_session_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let error_code = noprop::sample_u32(ctx);
        let reason = sample_reason(ctx, 100);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtCloseSession {
            error_code,
            reason: reason.clone(),
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// 複数 Capsule の連続デコードテスト
#[test]
fn prop_multiple_capsules_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let count =
            noprop::sample_with_boundaries(ctx, &[1usize, 9], noprop::Ratio::one_nth(5), |ctx| {
                noprop::sample_usize_in(ctx, 1..=9)
            });
        let data_sizes: Vec<usize> = (0..count).map(|_| sample_len(ctx, 100)).collect();
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let mut capsules: Vec<Capsule> = Vec::new();
        for (i, &size) in data_sizes.iter().enumerate() {
            let data = vec![i as u8; size];
            let capsule = Capsule::Datagram { data };
            encoder.encode(&capsule);
            capsules.push(capsule);
        }

        decoder.feed(encoder.buffer()).expect("feed should succeed");

        for expected in &capsules {
            let decoded = decoder
                .decode()
                .expect("decode should succeed")
                .expect("decode should succeed");
            assert_eq!(expected, &decoded);
        }

        // これ以上 Capsule はない
        assert!(decoder.decode().expect("decode should succeed").is_none());
        Ok(())
    })?;
    Ok(())
}

/// Unknown Capsule タイプの往復テスト
#[test]
fn prop_unknown_capsule_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 未知のタイプ (0x1000000..0x2000000)
        let capsule_type_value = noprop::sample_u64_in(ctx, 0x1000000..0x200_0000);
        let data = sample_arbitrary_bytes(ctx, 100);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Unknown {
            capsule_type: capsule_type_value,
            data: data.clone(),
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// PADDING Capsule 往復テスト
#[test]
fn prop_capsule_padding_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let length = sample_len(ctx, 1000);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::Padding { length };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_DATA_BLOCKED Capsule 往復テスト
#[test]
fn prop_capsule_wt_data_blocked_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let maximum = sample_small_varint_value(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtDataBlocked { maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_STREAM_DATA_BLOCKED Capsule 往復テスト
#[test]
fn prop_capsule_wt_stream_data_blocked_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_small_varint_value(ctx);
        let maximum = sample_small_varint_value(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStreamDataBlocked { stream_id, maximum };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// WT_STREAMS_BLOCKED Capsule 往復テスト
#[test]
fn prop_capsule_wt_streams_blocked_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let maximum = sample_small_varint_value(ctx);
        let bidirectional = noprop::sample_bool(ctx);
        let mut encoder = CapsuleEncoder::new();
        let mut decoder = CapsuleDecoder::new();

        let capsule = Capsule::WtStreamsBlocked {
            maximum,
            bidirectional,
        };
        encoder.encode(&capsule);

        decoder.feed(encoder.buffer()).expect("feed should succeed");
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");
        assert_eq!(capsule, decoded);
        Ok(())
    })?;
    Ok(())
}

/// 余剰バイト付き Capsule はデコードエラーになる (RFC 9297 Section 3.3)
#[test]
fn prop_capsule_trailing_bytes_rejected() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let maximum = sample_small_varint_value(ctx);
        let trailing_len =
            noprop::sample_with_boundaries(ctx, &[1usize, 8], noprop::Ratio::one_nth(5), |ctx| {
                1 + noprop::sample_usize_in(ctx, 0..=7)
            });
        let trailing: Vec<u8> = noprop::sample_bytes_vec(ctx, trailing_len);

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
        assert!(decoder.decode().is_err());
        Ok(())
    })?;
    Ok(())
}

/// WT ストリーム送信フロー制御: 上限超過はエラー
///
/// 成功/失敗を独立サンプリングの重なりに頼らず first-class 分岐にする。
/// 等確率 1/2、N=256 で未到達は (1/2)^256。
#[test]
fn prop_wt_stream_send_flow_control() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let ok_gate = std::cell::Cell::new(0usize);
    let err_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_data = noprop::sample_with_boundaries(
            ctx,
            &[1u64, 10_000],
            noprop::Ratio::one_nth(5),
            |ctx| 1 + noprop::sample_u64_in(ctx, 0..=10_000),
        );
        let mut stream = WtStream::new(0, max_data, max_data, true, true);

        match noprop::sample_weighted_index(ctx, &[1, 1]) {
            0 => {
                let send_size = noprop::sample_u64_in(ctx, 1..=max_data);
                stream.send_data(send_size, false).expect("within window");
                ok_gate.set(ok_gate.get() + 1);
            }
            _ => {
                let send_size = max_data + 1 + noprop::sample_u64_in(ctx, 0..=10_000);
                assert!(stream.send_data(send_size, false).is_err());
                err_gate.set(err_gate.get() + 1);
            }
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "送信成功パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        err_gate.get() > 0,
        "送信超過エラーパスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// WT ストリーム受信フロー制御: 上限超過はエラー
///
/// 成功/失敗を独立サンプリングの重なりに頼らず first-class 分岐にする。
/// 等確率 1/2、N=256 で未到達は (1/2)^256。
#[test]
fn prop_wt_stream_recv_flow_control() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let ok_gate = std::cell::Cell::new(0usize);
    let err_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_data = noprop::sample_with_boundaries(
            ctx,
            &[1u64, 10_000],
            noprop::Ratio::one_nth(5),
            |ctx| 1 + noprop::sample_u64_in(ctx, 0..=10_000),
        );
        let mut stream = WtStream::new(0, max_data, max_data, true, true);

        match noprop::sample_weighted_index(ctx, &[1, 1]) {
            0 => {
                let recv_size = noprop::sample_u64_in(ctx, 1..=max_data);
                stream.recv_data(recv_size, false).expect("within window");
                ok_gate.set(ok_gate.get() + 1);
            }
            _ => {
                let recv_size = max_data + 1 + noprop::sample_u64_in(ctx, 0..=10_000);
                assert!(stream.recv_data(recv_size, false).is_err());
                err_gate.set(err_gate.get() + 1);
            }
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "受信成功パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        err_gate.get() > 0,
        "受信超過エラーパスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// WT ストリームフロー制御: 複数回の送信で累積が上限を超えるとエラー
///
/// 1 回目は必ず成功する量、2 回目は成功/超過を first-class 分岐で選ぶ。
/// 等確率 1/2、N=256 で未到達は (1/2)^256。
#[test]
fn prop_wt_stream_cumulative_send_flow_control() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let ok_gate = std::cell::Cell::new(0usize);
    let overflow_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_data =
            noprop::sample_with_boundaries(ctx, &[2u64, 1_000], noprop::Ratio::one_nth(5), |ctx| {
                2 + noprop::sample_u64_in(ctx, 0..=998)
            });
        // 1 回目は窓を残す (1..=max_data-1)
        let chunk1 = noprop::sample_u64_in(ctx, 1..=max_data - 1);
        let remaining = max_data - chunk1;
        let mut stream = WtStream::new(0, max_data, max_data, true, true);
        stream
            .send_data(chunk1, false)
            .expect("first chunk fits by construction");

        match noprop::sample_weighted_index(ctx, &[1, 1]) {
            0 => {
                let chunk2 = noprop::sample_u64_in(ctx, 1..=remaining);
                stream
                    .send_data(chunk2, false)
                    .expect("second chunk fits by construction");
                ok_gate.set(ok_gate.get() + 1);
            }
            _ => {
                let chunk2 = remaining + 1 + noprop::sample_u64_in(ctx, 0..=100);
                assert!(stream.send_data(chunk2, false).is_err());
                overflow_gate.set(overflow_gate.get() + 1);
            }
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "累積送信の成功パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        overflow_gate.get() > 0,
        "累積フロー制御超過のエラーパスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// セッション状態の不変条件: 状態は常に有効な WtSessionState の 1 つ
///
/// 数学的意義: 状態空間の閉包性
#[test]
fn prop_session_state_always_valid() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let executed_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let is_client = noprop::sample_bool(ctx);
        let steps = noprop::sample_with_boundaries(
            ctx,
            &[0usize, 1, 29],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, 0..=29),
        );
        let mut session = if is_client {
            WtSession::client(WtConfig::default(), WtConfig::default())
        } else {
            WtSession::server(WtConfig::default(), WtConfig::default())
        };

        // 初期状態 (空列) でも不変条件を検査する
        {
            let state = session.state();
            assert_eq!(session.is_active(), state == WtSessionState::Active);
            assert_eq!(session.is_closed(), state == WtSessionState::Closed);
        }

        for _ in 0..steps {
            let op = sample_session_op(ctx);
            let _ = apply_session_op(&mut session, &op);

            // 不変条件: 状態は常に有効な値
            let state = session.state();
            let is_valid_state = matches!(
                state,
                WtSessionState::Initial
                    | WtSessionState::Active
                    | WtSessionState::Draining
                    | WtSessionState::Closed
            );
            assert!(is_valid_state, "Invalid session state after {op:?}");

            // 不変条件: is_active() と is_closed() は状態と整合する
            assert_eq!(session.is_active(), state == WtSessionState::Active);
            assert_eq!(session.is_closed(), state == WtSessionState::Closed);
            executed_gate.set(executed_gate.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        executed_gate.get() > 0,
        "操作後の不変条件が一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// Closed 状態は吸収状態
///
/// 数学的意義: 終状態からは遷移しない
#[test]
fn prop_session_closed_is_absorbing() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let is_client = noprop::sample_bool(ctx);
        let before_steps = noprop::sample_with_boundaries(
            ctx,
            &[0usize, 1, 9],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, 0..=9),
        );
        let after_steps =
            noprop::sample_with_boundaries(ctx, &[1usize, 10], noprop::Ratio::one_nth(5), |ctx| {
                1 + noprop::sample_usize_in(ctx, 0..=9)
            });
        let mut session = if is_client {
            WtSession::client(WtConfig::default(), WtConfig::default())
        } else {
            WtSession::server(WtConfig::default(), WtConfig::default())
        };

        // 操作を適用
        for _ in 0..before_steps {
            let _ = apply_session_op(&mut session, &sample_session_op(ctx));
        }

        // Closed でなければ強制的に Closed にする
        if !session.is_closed() {
            let _ = session.initiate();
            let _ = session.close(0, "force close");
        }
        assert!(
            session.is_closed(),
            "force close のあと Closed になっていなければならない"
        );

        // Closed 後の操作は状態を変えない
        for _ in 0..after_steps {
            let op = sample_session_op(ctx);
            let _ = apply_session_op(&mut session, &op);
            assert!(
                session.is_closed(),
                "Closed state should be absorbing, but changed after {op:?}",
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// 状態遷移の単調性: Initial -> Active -> Draining -> Closed
///
/// 数学的意義: 状態遷移グラフの有向非巡回性
#[test]
fn prop_session_state_monotonicity() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let executed_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let is_client = noprop::sample_bool(ctx);
        let steps = noprop::sample_with_boundaries(
            ctx,
            &[0usize, 1, 29],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, 0..=29),
        );
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

        for _ in 0..steps {
            let op = sample_session_op(ctx);
            let _ = apply_session_op(&mut session, &op);

            let current_order = state_order(session.state());

            // 不変条件: 状態は前に戻らない (Draining から Active には戻らない等)
            // ただし、Active/Draining -> Closed は許可
            // また、同じ状態にとどまることも許可
            if current_order < max_order
                && session.state() != WtSessionState::Closed
                && !(current_order == 1 && max_order == 1)
            {
                // Active に留まることは許可 (Draining にならない限り)
                panic!(
                    "State went backwards from order {} to {} after {op:?}",
                    max_order, current_order,
                );
            }
            max_order = max_order.max(current_order);
            executed_gate.set(executed_gate.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        executed_gate.get() > 0,
        "操作後の単調性が一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// ストリーム ID の単調増加の不変条件
///
/// 数学的意義: ストリーム ID 生成の単射性
#[test]
fn prop_stream_id_monotonic_invariant() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 少なくとも 1 回はストリーム ID が発番されたことをゲートする
    let id_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let is_client = noprop::sample_bool(ctx);
        let steps =
            noprop::sample_with_boundaries(ctx, &[1usize, 19], noprop::Ratio::one_nth(5), |ctx| {
                1 + noprop::sample_usize_in(ctx, 0..=18)
            });
        let mut session = if is_client {
            WtSession::client(WtConfig::default(), WtConfig::default())
        } else {
            WtSession::server(WtConfig::default(), WtConfig::default())
        };
        session.initiate().expect("initiate should succeed");

        let mut bidi_ids: Vec<u64> = Vec::new();
        let mut uni_ids: Vec<u64> = Vec::new();

        for _ in 0..steps {
            let is_bidi = noprop::sample_bool(ctx);
            let result = if is_bidi {
                session.open_bidi_stream()
            } else {
                session.open_uni_stream()
            };

            if let Ok(id) = result {
                if is_bidi {
                    // 不変条件: すべての bidi ID は異なり、単調増加
                    if let Some(&last) = bidi_ids.last() {
                        assert!(id > last, "bidi stream ID not monotonically increasing");
                    }
                    assert!(!bidi_ids.contains(&id), "duplicate bidi stream ID");
                    bidi_ids.push(id);
                } else {
                    // 不変条件: すべての uni ID は異なり、単調増加
                    if let Some(&last) = uni_ids.last() {
                        assert!(id > last, "uni stream ID not monotonically increasing");
                    }
                    assert!(!uni_ids.contains(&id), "duplicate uni stream ID");
                    uni_ids.push(id);
                }
                id_gate.set(id_gate.get() + 1);
            }
        }
        Ok(())
    })?;
    assert!(
        id_gate.get() > 0,
        "ストリーム ID が一度も発番されなかった\n{runner}"
    );
    Ok(())
}

/// Capsule のラウンドトリップ: encode -> decode -> encode == encode
///
/// 数学的意義: エンコード・デコードの同型性
#[test]
fn prop_capsule_encode_decode_isomorphism() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let stream_id = sample_small_varint_value(ctx);
        let data = sample_arbitrary_bytes(ctx, 100);
        let fin = noprop::sample_bool(ctx);
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
        let decoded = decoder
            .decode()
            .expect("feed should succeed")
            .expect("feed should succeed");

        // 再エンコード
        let mut encoder2 = CapsuleEncoder::new();
        encoder2.encode(&decoded);
        let encoded2 = encoder2.buffer().to_vec();

        // 不変条件: encode(decode(encode(x))) == encode(x)
        assert_eq!(encoded1, encoded2);
        assert_eq!(original, decoded);
        Ok(())
    })?;
    Ok(())
}
