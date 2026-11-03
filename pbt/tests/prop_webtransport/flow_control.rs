//! WebTransport フロー制御の PBT
//!
//! `WT_MAX_DATA` の送信上限更新と広告の厳密増加規則を検証する。

use shiguredo_http2::webtransport::{
    Capsule, CapsuleDecoder, MAX_VALUE, WtConfig, WtErrorKind, WtFlowControl, WtSession,
};

use super::{CASES, SEED_ENV, sample_valid_varint_value};

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
