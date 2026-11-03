//! WebTransport フロー制御の PBT
//!
//! `WT_MAX_DATA` / `WT_MAX_STREAMS` の送信上限・ストリーム数上限の更新と広告の
//! 厳密増加規則を検証する。

use std::cell::Cell;

use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, MAX_VALUE, WtConfig, WtErrorKind, WtFlowControl, WtSession,
};

use super::{CASES, SEED_ENV, sample_valid_varint_value};

/// Maximum Streams の上限 (2^60)
const MAX_STREAMS_LIMIT: u64 = 1 << 60;

/// 現在値を基準に、同値・±1・2^60 境界を含む値を返すサンプラー
fn sample_max_streams_value_around(ctx: &mut noprop::TestCaseContext, current: u64) -> u64 {
    noprop::sample_with_boundaries(
        ctx,
        &[
            current,
            current.saturating_add(1),
            current.saturating_sub(1),
            MAX_STREAMS_LIMIT,
            MAX_STREAMS_LIMIT + 1,
        ],
        noprop::Ratio::one_nth(7),
        |ctx| noprop::sample_u64_in(ctx, 0..=MAX_VALUE),
    )
}

/// ストリーム数上限の初期値に使う小さめの値を返すサンプラー
fn sample_small_max_streams_value(ctx: &mut noprop::TestCaseContext) -> u64 {
    noprop::sample_with_boundaries(ctx, &[0u64, 1, 8], noprop::Ratio::one_nth(5), |ctx| {
        noprop::sample_u64_in(ctx, 0..=8)
    })
}

/// `poll_output` から WT_MAX_STREAMS を 1 つ取り出し、maximum と方向を返す
fn decode_single_wt_max_streams(session: &mut WtSession) -> (u64, bool) {
    let out = session.poll_output().expect("WT_MAX_STREAMS の出力が必要");
    let mut decoder = CapsuleDecoder::new();
    decoder
        .feed(&out)
        .expect("受信データをデコーダーに渡せなければならない");
    let capsule = decoder
        .decode()
        .expect("capsule をデコードできなければならない")
        .expect("capsule が必要");
    let result = match capsule {
        Capsule::WtMaxStreams {
            maximum,
            bidirectional,
        } => (maximum, bidirectional),
        other => panic!("WtMaxStreams を期待したが {other:?} だった"),
    };
    assert!(
        decoder
            .decode()
            .expect("余剰 capsule をデコードできなければならない")
            .is_none(),
        "余剰の capsule を出力してはいけない"
    );
    result
}

/// WT_MAX_DATA の受信で更新する送信上限が、増加する値だけを受理することを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.5: ピアの初期値または最後に受理した上限を
/// 増加させない値は WT_FLOW_CONTROL_ERROR になる。
#[test]
fn prop_wt_max_data_update_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial = sample_valid_varint_value(ctx);
        let mut flow_control = WtFlowControl::new(initial, 0, 0, 0, 0, 0);
        let mut accepted = initial;

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            let value = sample_valid_varint_value(ctx);
            let result = flow_control.update_send_max(value);
            if value > accepted {
                result.expect("増加する上限は受理されなければならない");
                accepted = value;
            } else {
                let err = result.expect_err("増加しない上限は拒否されなければならない");
                assert_eq!(
                    err.kind(),
                    WtErrorKind::FlowControlError,
                    "拒否時はフロー制御エラーでなければならない"
                );
            }
            assert_eq!(
                flow_control.send_available(),
                accepted,
                "送信上限は最後に受理した値でなければならない"
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// WT_MAX_DATA の広告が増加する値だけを出力し、受信上限と一致することを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.5: 初期値または最後に広告した値を
/// 増加させない値は拒否し、受信上限と出力を変更しない。
#[test]
fn prop_wt_max_data_advertisement_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial = sample_valid_varint_value(ctx);
        let config = WtConfig {
            initial_max_data: initial,
            ..WtConfig::default()
        };
        let mut session = WtSession::server(config, WtConfig::default());
        session
            .initiate()
            .expect("セッションを開始できなければならない");
        let mut advertised = initial;

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            let value = sample_valid_varint_value(ctx);
            let result = session.send_max_data(value);
            if value > advertised {
                result.expect("増加する広告は受理されなければならない");
                advertised = value;
                assert_eq!(
                    session.flow_control().recv_max(),
                    advertised,
                    "広告値と受信上限が一致しなければならない"
                );
                let out = session
                    .poll_output()
                    .expect("受理時は WT_MAX_DATA を出力しなければならない");
                let mut decoder = CapsuleDecoder::new();
                decoder
                    .feed(&out)
                    .expect("受信データをデコーダーに渡せなければならない");
                match decoder
                    .decode()
                    .expect("capsule をデコードできなければならない")
                {
                    Some(Capsule::WtMaxData { maximum }) => {
                        assert_eq!(
                            maximum, advertised,
                            "出力された広告値が一致しなければならない"
                        );
                    }
                    other => panic!("WT_MAX_DATA を期待したが {other:?} だった"),
                }
                assert!(
                    decoder
                        .decode()
                        .expect("余剰 capsule をデコードできなければならない")
                        .is_none(),
                    "受理時に余剰の capsule を出力してはいけない"
                );
            } else {
                let err = result.expect_err("増加しない広告は拒否されなければならない");
                assert_eq!(
                    err.kind(),
                    WtErrorKind::FlowControlError,
                    "拒否時はフロー制御エラーでなければならない"
                );
                assert_eq!(
                    session.flow_control().recv_max(),
                    advertised,
                    "拒否時に受信上限が変わってはいけない"
                );
                assert!(
                    session.poll_output().is_none(),
                    "拒否時に WT_MAX_DATA を出力してはいけない"
                );
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// `grow_recv_window` の増分列が広告値と受信上限を正しく進めることを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.5: 増分 0 と varint 上限を超える拡張は拒否し、
/// 成功時は広告値と受信上限を一致させ、最後に出力した capsule の maximum と一致させる。
#[test]
fn prop_grow_recv_window_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // varint 上限に到達しない範囲の初期値から始める
        let initial = noprop::sample_with_boundaries(
            ctx,
            &[0u64, 1, 1 << 20],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_u64_in(ctx, 0..=(1 << 20)),
        );
        let config = WtConfig {
            initial_max_data: initial,
            ..WtConfig::default()
        };
        let mut session = WtSession::server(config, WtConfig::default());
        session
            .initiate()
            .expect("セッションを開始できなければならない");
        let mut advertised = initial;

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            // 0 / 小さい増分 / 中くらいの増分 / u64::MAX を混ぜて分岐を網羅する
            let increment = match noprop::sample_weighted_index(ctx, &[1, 1, 1, 1]) {
                0 => 0,
                1 => noprop::sample_u64_in(ctx, 1..=(1 << 20)),
                2 => (1u64 << 20) + noprop::sample_u64_in(ctx, 0..=(1 << 20)),
                _ => u64::MAX,
            };
            let expected_ok = increment > 0
                && advertised
                    .checked_add(increment)
                    .is_some_and(|value| value <= MAX_VALUE);
            let result = session.grow_recv_window(increment);

            if expected_ok {
                result.expect("増加する拡張は受理されなければならない");
                advertised += increment;
                assert_eq!(
                    session.flow_control().recv_max(),
                    advertised,
                    "成功時に受信上限と広告値が一致しなければならない"
                );
                let out = session
                    .poll_output()
                    .expect("成功時は WT_MAX_DATA を出力しなければならない");
                let mut decoder = CapsuleDecoder::new();
                decoder
                    .feed(&out)
                    .expect("受信データをデコーダーに渡せなければならない");
                match decoder
                    .decode()
                    .expect("capsule をデコードできなければならない")
                {
                    Some(Capsule::WtMaxData { maximum }) => {
                        assert_eq!(
                            maximum, advertised,
                            "出力された広告値が一致しなければならない"
                        );
                    }
                    other => panic!("WT_MAX_DATA を期待したが {other:?} だった"),
                }
                assert!(
                    decoder
                        .decode()
                        .expect("余剰 capsule をデコードできなければならない")
                        .is_none(),
                    "受理時に余剰の capsule を出力してはいけない"
                );
            } else {
                let err = result.expect_err("増加しない拡張は拒否されなければならない");
                assert_eq!(
                    err.kind(),
                    WtErrorKind::FlowControlError,
                    "拒否時はフロー制御エラーでなければならない"
                );
                assert_eq!(
                    session.flow_control().recv_max(),
                    advertised,
                    "拒否時に受信上限が変わってはいけない"
                );
                assert!(
                    session.poll_output().is_none(),
                    "拒否時に WT_MAX_DATA を出力してはいけない"
                );
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// `WtFlowControl::update_max_streams` が方向別に増加する値だけを受理することを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.7: 対象方向の初期値または最後に受理した
/// 上限を増加させない値は WT_FLOW_CONTROL_ERROR になる。他方の方向には影響しない。
#[test]
fn prop_wt_max_streams_update_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    let accepted_bidi_gate = Cell::new(0u32);
    let accepted_uni_gate = Cell::new(0u32);
    let rejected_gate = Cell::new(0u32);
    runner.run(CASES, |ctx| {
        let bidi = sample_small_max_streams_value(ctx);
        let uni = sample_small_max_streams_value(ctx);
        let mut flow_control = WtFlowControl::new(0, 0, bidi, bidi, uni, uni);
        let mut accepted_bidi = bidi;
        let mut accepted_uni = uni;

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            let bidirectional = noprop::sample_bool(ctx);
            let current = if bidirectional {
                accepted_bidi
            } else {
                accepted_uni
            };
            let value = sample_max_streams_value_around(ctx, current);
            let expected_ok = value <= MAX_STREAMS_LIMIT && value > current;
            let result = flow_control.update_max_streams(value, bidirectional);

            if expected_ok {
                result.expect("増加する上限は受理されなければならない");
                if bidirectional {
                    accepted_bidi = value;
                    accepted_bidi_gate.set(accepted_bidi_gate.get() + 1);
                } else {
                    accepted_uni = value;
                    accepted_uni_gate.set(accepted_uni_gate.get() + 1);
                }
            } else {
                let err = result.expect_err("増加しない上限は拒否されなければならない");
                assert_eq!(err.kind(), WtErrorKind::FlowControlError);
                rejected_gate.set(rejected_gate.get() + 1);
            }

            assert_eq!(
                flow_control.max_streams_bidi_remote(),
                accepted_bidi,
                "bidi のピア上限は最後に受理した値でなければならない"
            );
            assert_eq!(
                flow_control.max_streams_uni_remote(),
                accepted_uni,
                "uni のピア上限は最後に受理した値でなければならない"
            );
            assert_eq!(
                flow_control.max_streams_bidi_local(),
                bidi,
                "ローカル上限は不変"
            );
            assert_eq!(
                flow_control.max_streams_uni_local(),
                uni,
                "ローカル上限は不変"
            );
        }
        Ok(())
    })?;
    assert!(accepted_bidi_gate.get() > 0, "bidi の受理分岐が必要");
    assert!(accepted_uni_gate.get() > 0, "uni の受理分岐が必要");
    assert!(rejected_gate.get() > 0, "拒否分岐が必要");
    Ok(())
}

/// `WtSession::send_max_streams` の広告が方向別に増加する値だけを出力することを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.7: 対象方向の初期値または最後に広告した
/// 上限を増加させない値は拒否し、ローカル上限と出力を変更しない。
#[test]
fn prop_wt_max_streams_advertisement_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    let accepted_bidi_gate = Cell::new(0u32);
    let accepted_uni_gate = Cell::new(0u32);
    let rejected_gate = Cell::new(0u32);
    runner.run(CASES, |ctx| {
        let bidi = sample_small_max_streams_value(ctx);
        let uni = sample_small_max_streams_value(ctx);
        let config = WtConfig {
            initial_max_streams_bidi: bidi,
            initial_max_streams_uni: uni,
            ..WtConfig::default()
        };
        let mut session = WtSession::server(config, WtConfig::peer_default());
        session
            .initiate()
            .expect("セッションを開始できなければならない");
        let mut advertised_bidi = bidi;
        let mut advertised_uni = uni;

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            let bidirectional = noprop::sample_bool(ctx);
            let current = if bidirectional {
                advertised_bidi
            } else {
                advertised_uni
            };
            let value = sample_max_streams_value_around(ctx, current);
            let expected_ok = value > current && value <= MAX_STREAMS_LIMIT;
            let result = session.send_max_streams(value, bidirectional);

            if expected_ok {
                result.expect("増加する広告は受理されなければならない");
                if bidirectional {
                    advertised_bidi = value;
                    accepted_bidi_gate.set(accepted_bidi_gate.get() + 1);
                } else {
                    advertised_uni = value;
                    accepted_uni_gate.set(accepted_uni_gate.get() + 1);
                }
                let (streams, is_bidi) = decode_single_wt_max_streams(&mut session);
                assert_eq!(streams, value);
                assert_eq!(is_bidi, bidirectional);
            } else {
                let err = result.expect_err("増加しない広告は拒否されなければならない");
                assert_eq!(err.kind(), WtErrorKind::FlowControlError);
                rejected_gate.set(rejected_gate.get() + 1);
                assert!(
                    session.poll_output().is_none(),
                    "拒否時に WT_MAX_STREAMS を出力してはいけない"
                );
            }

            assert_eq!(
                session.flow_control().max_streams_bidi_local(),
                advertised_bidi,
                "bidi のローカル上限は最後に広告した値でなければならない"
            );
            assert_eq!(
                session.flow_control().max_streams_uni_local(),
                advertised_uni,
                "uni のローカル上限は最後に広告した値でなければならない"
            );
        }
        Ok(())
    })?;
    assert!(accepted_bidi_gate.get() > 0, "bidi の受理分岐が必要");
    assert!(accepted_uni_gate.get() > 0, "uni の受理分岐が必要");
    assert!(rejected_gate.get() > 0, "拒否分岐が必要");
    Ok(())
}

/// `WtSession::grow_max_streams` の増分列がローカル上限と広告値を進めることを検証する
///
/// draft-ietf-webtrans-http2-16 Section 6.7: 増分 0、2^60 超過、u64 オーバーフローは
/// 拒否し、成功時は広告値とローカル上限を一致させる。
#[test]
fn prop_grow_max_streams_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    let accepted_bidi_gate = Cell::new(0u32);
    let accepted_uni_gate = Cell::new(0u32);
    let rejected_gate = Cell::new(0u32);
    runner.run(CASES, |ctx| {
        let bidi = sample_small_max_streams_value(ctx);
        let uni = sample_small_max_streams_value(ctx);
        let config = WtConfig {
            initial_max_streams_bidi: bidi,
            initial_max_streams_uni: uni,
            ..WtConfig::default()
        };
        let mut session = WtSession::server(config, WtConfig::peer_default());
        session
            .initiate()
            .expect("セッションを開始できなければならない");
        let mut advertised_bidi = bidi;
        let mut advertised_uni = uni;

        for _ in 0..noprop::sample_usize_in(ctx, 1..=8) {
            let bidirectional = noprop::sample_bool(ctx);
            // 0 / 小さい増分 / 2^60 近傍 / u64 オーバーフローを混ぜて分岐を網羅する
            let increment = match noprop::sample_weighted_index(ctx, &[1, 1, 1, 1, 1]) {
                0 => 0,
                1 => noprop::sample_u64_in(ctx, 1..=8),
                2 => noprop::sample_u64_in(ctx, 1..=4096),
                3 => MAX_STREAMS_LIMIT - noprop::sample_u64_in(ctx, 0..=4096),
                _ => u64::MAX,
            };
            let current = if bidirectional {
                advertised_bidi
            } else {
                advertised_uni
            };
            let expected_ok = increment > 0
                && current
                    .checked_add(increment)
                    .is_some_and(|value| value <= MAX_STREAMS_LIMIT);
            let result = session.grow_max_streams(increment, bidirectional);

            if expected_ok {
                result.expect("増加する拡張は受理されなければならない");
                if bidirectional {
                    accepted_bidi_gate.set(accepted_bidi_gate.get() + 1);
                } else {
                    accepted_uni_gate.set(accepted_uni_gate.get() + 1);
                }
                let expected = current + increment;
                if bidirectional {
                    advertised_bidi = expected;
                } else {
                    advertised_uni = expected;
                }
                let (maximum, is_bidi) = decode_single_wt_max_streams(&mut session);
                assert_eq!(maximum, expected);
                assert_eq!(is_bidi, bidirectional);
            } else {
                let err = result.expect_err("増加しない拡張は拒否されなければならない");
                assert_eq!(err.kind(), WtErrorKind::FlowControlError);
                rejected_gate.set(rejected_gate.get() + 1);
                assert!(
                    session.poll_output().is_none(),
                    "拒否時に WT_MAX_STREAMS を出力してはいけない"
                );
            }

            assert_eq!(
                session.flow_control().max_streams_bidi_local(),
                advertised_bidi
            );
            assert_eq!(
                session.flow_control().max_streams_uni_local(),
                advertised_uni
            );
        }
        Ok(())
    })?;
    assert!(accepted_bidi_gate.get() > 0, "bidi の受理分岐が必要");
    assert!(accepted_uni_gate.get() > 0, "uni の受理分岐が必要");
    assert!(rejected_gate.get() > 0, "拒否分岐が必要");
    Ok(())
}
