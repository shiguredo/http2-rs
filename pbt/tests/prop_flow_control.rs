//! フロー制御の PBT
//!
//! 本 PBT は `src/flow_control.rs` (接続/ストリームレベル) に対応する。
//! `src/webtransport/flow_control.rs` 用の PBT は将来 `pbt/tests/prop_webtransport/flow_control.rs` に配置する。

use shiguredo_http2::{FlowControl, MAX_WINDOW_SIZE};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// 有効なウィンドウサイズ (1..=2^31-1) を生成する
fn sample_valid_window_size(ctx: &mut noprop::TestCaseContext) -> u32 {
    noprop::sample_with_boundaries(
        ctx,
        &[1u32, MAX_WINDOW_SIZE],
        noprop::Ratio::one_nth(5),
        |ctx| 1 + noprop::sample_u64_in(ctx, 0..MAX_WINDOW_SIZE as u64) as u32,
    )
}

/// フロー制御の初期化テスト
#[test]
fn prop_flow_control_init() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_window = sample_valid_window_size(ctx);
        let fc = FlowControl::new(initial_window);
        assert_eq!(fc.send_window(), i64::from(initial_window));
        assert_eq!(fc.recv_window(), i64::from(initial_window));
        assert_eq!(fc.send_initial(), initial_window);
        assert_eq!(fc.recv_initial(), initial_window);
        Ok(())
    })?;
    Ok(())
}

/// 送信/受信ウィンドウ分離初期化テスト
///
/// RFC 9113 Section 6.9.2: 新規ストリームのフロー制御ウィンドウは
/// SETTINGS_INITIAL_WINDOW_SIZE で初期化される。
/// 送信ウィンドウはリモートの設定値、受信ウィンドウはローカルの設定値で初期化する。
#[test]
fn prop_separate_windows_init() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let send_initial = sample_valid_window_size(ctx);
        let recv_initial = sample_valid_window_size(ctx);
        let fc = FlowControl::with_separate_windows(send_initial, recv_initial);
        assert_eq!(fc.send_window(), i64::from(send_initial));
        assert_eq!(fc.recv_window(), i64::from(recv_initial));
        assert_eq!(fc.send_initial(), send_initial);
        assert_eq!(fc.recv_initial(), recv_initial);
        Ok(())
    })?;
    Ok(())
}

/// 送信ウィンドウ消費テスト
///
/// 消費量がウィンドウ以内なら成功してウィンドウが減り、超過ならエラーになる。
/// エラー分岐を確実に探索するため `consume_size` をウィンドウ範囲全域から引く。
#[test]
fn prop_consume_send() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 成功 (消費量 <= ウィンドウ) / 失敗 (超過) それぞれの到達ゲート
    let ok_gate = std::cell::Cell::new(0usize);
    let err_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_window = noprop::sample_with_boundaries(
            ctx,
            &[100u32, 65_535],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_u64_in(ctx, 100..=65_535) as u32,
        );
        let mut fc = FlowControl::new(initial_window);
        match noprop::sample_weighted_index(ctx, &[1, 1]) {
            0 => {
                let consume_size = noprop::sample_usize_in(ctx, 0..=initial_window as usize);
                fc.consume_send(consume_size).expect("within window");
                assert_eq!(
                    fc.send_window(),
                    i64::from(initial_window) - consume_size as i64
                );
                ok_gate.set(ok_gate.get() + 1);
            }
            _ => {
                let consume_size =
                    initial_window as usize + 1 + noprop::sample_usize_in(ctx, 0..=65_535);
                assert!(fc.consume_send(consume_size).is_err());
                err_gate.set(err_gate.get() + 1);
            }
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "消費成功パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        err_gate.get() > 0,
        "ウィンドウ超過エラーパスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// 送信可能サイズの計算テスト
///
/// `send_available()` は消費後の残りウィンドウと一致する。
#[test]
fn prop_send_available() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_window = noprop::sample_with_boundaries(
            ctx,
            &[1u32, 65_535],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_u64_in(ctx, 1..=65_535) as u32,
        );
        let consume_size = noprop::sample_usize_in(ctx, 0..=initial_window as usize);
        let mut fc = FlowControl::new(initial_window);
        let consume = consume_size.min(initial_window as usize);
        fc.consume_send(consume).expect("should succeed");

        let available = fc.send_available();
        let expected = (initial_window as usize).saturating_sub(consume);
        assert_eq!(available, expected);
        Ok(())
    })?;
    Ok(())
}

/// WINDOW_UPDATE 受信テスト
///
/// RFC 9113 Section 6.9.1: フロー制御ウィンドウは 2^31-1 オクテットを超えてはならない (MUST NOT)。
/// 境界値付きの増分を引くことでウィンドウ超過エラー分岐も確実に探索する。
#[test]
fn prop_window_update() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let ok_gate = std::cell::Cell::new(0usize);
    let err_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_window = noprop::sample_u64_in(ctx, 1..=1_000_000) as u32;
        let consume_size = noprop::sample_usize_in(ctx, 0..=1_000_000);
        // 境界値 (2^31-1 付近) を 1/4 の確率で引く
        let increment = noprop::sample_with_boundaries(
            ctx,
            &[1u32, 1_000_000, MAX_WINDOW_SIZE - 1, MAX_WINDOW_SIZE],
            noprop::Ratio::one_nth(4),
            |ctx| 1 + noprop::sample_u64_in(ctx, 0..1_000_000u64) as u32,
        );
        let mut fc = FlowControl::new(initial_window);
        let consume = consume_size.min(initial_window as usize);
        fc.consume_send(consume).expect("should succeed");

        let before = fc.send_window();
        let result = fc.recv_window_update(increment);

        let new_window = before + i64::from(increment);
        if new_window > i64::from(MAX_WINDOW_SIZE) {
            assert!(result.is_err());
            err_gate.set(err_gate.get() + 1);
        } else {
            assert!(result.is_ok());
            assert_eq!(fc.send_window(), new_window);
            ok_gate.set(ok_gate.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "WINDOW_UPDATE 成功パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        err_gate.get() > 0,
        "ウィンドウオーバーフローエラーパスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// ウィンドウサイズ更新テスト
///
/// SETTINGS_INITIAL_WINDOW_SIZE 変更時の送信ウィンドウ調整を検証する。
/// 境界値付きの新サイズを引くことでオーバーフローエラー分岐も確実に探索する。
#[test]
fn prop_update_initial_window_size() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let ok_gate = std::cell::Cell::new(0usize);
    let err_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_window = noprop::sample_u64_in(ctx, 1..=65535) as u32;
        let consume_size = noprop::sample_usize_in(ctx, 0..=32767);
        // 2^31 を超える境界値で大きい調整幅も引く。
        // オーバーフロー条件は new_window = (initial - consume) + (new_initial - initial)
        // = new_initial - consume > 2^31-1 なので、new_initial >= 2^31 かつ consume が小さい必要がある。
        let new_initial = noprop::sample_with_boundaries(
            ctx,
            &[MAX_WINDOW_SIZE, MAX_WINDOW_SIZE + 1_000_000, u32::MAX],
            noprop::Ratio::one_nth(4),
            |ctx| 1 + noprop::sample_u64_in(ctx, 0..131070u64) as u32,
        );
        let mut fc = FlowControl::new(initial_window);
        let consume = consume_size.min(initial_window as usize);
        fc.consume_send(consume).expect("should succeed");

        let before = fc.send_window();
        let result = fc.update_initial_window_size(new_initial);

        let delta = i64::from(new_initial) - i64::from(initial_window);
        let new_window = before + delta;

        if new_window > i64::from(MAX_WINDOW_SIZE) {
            assert!(result.is_err());
            err_gate.set(err_gate.get() + 1);
        } else {
            assert!(result.is_ok());
            assert_eq!(fc.send_window(), new_window);
            assert_eq!(fc.send_initial(), new_initial);
            ok_gate.set(ok_gate.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        ok_gate.get() > 0,
        "初期ウィンドウ更新の成功パスが一度も実行されなかった\n{runner}"
    );
    assert!(
        err_gate.get() > 0,
        "ウィンドウオーバーフローエラーパスが一度も実行されなかった\n{runner}"
    );
    Ok(())
}

/// should_send_window_update / window_update_increment が send_initial に依存しないことを検証する
///
/// 任意の (send_initial_a, send_initial_b, recv_initial, consume_amount) に対して、
/// with_separate_windows(send_initial_a, recv_initial) と
/// with_separate_windows(send_initial_b, recv_initial) で同じ量を consume_recv した後の
/// 結果が一致することを検証する。
#[test]
fn prop_recv_methods_independent_of_send_initial() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let send_initial_a = sample_valid_window_size(ctx);
        let send_initial_b = sample_valid_window_size(ctx);
        let recv_initial = noprop::sample_u64_in(ctx, 1..=1_000_000) as u32;
        let consume_amount = noprop::sample_usize_in(ctx, 0..=1_000_000);
        let mut fc_a = FlowControl::with_separate_windows(send_initial_a, recv_initial);
        let mut fc_b = FlowControl::with_separate_windows(send_initial_b, recv_initial);

        let consume = consume_amount.min(recv_initial as usize);
        fc_a.consume_recv(consume).expect("should succeed");
        fc_b.consume_recv(consume).expect("should succeed");

        assert_eq!(
            fc_a.should_send_window_update(),
            fc_b.should_send_window_update()
        );
        assert_eq!(
            fc_a.window_update_increment(),
            fc_b.window_update_increment()
        );
        Ok(())
    })?;
    Ok(())
}

/// update_initial_window_size が recv_initial を変更しないことを検証する
#[test]
fn prop_update_initial_preserves_recv_initial() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let send_initial = sample_valid_window_size(ctx);
        let recv_initial = sample_valid_window_size(ctx);
        let new_size = noprop::sample_u64_in(ctx, 1..=131070) as u32;
        let mut fc = FlowControl::with_separate_windows(send_initial, recv_initial);
        let recv_initial_before = fc.recv_initial();
        let _ = fc.update_initial_window_size(new_size);
        assert_eq!(fc.recv_initial(), recv_initial_before);
        Ok(())
    })?;
    Ok(())
}

/// recv_window > recv_initial のケースで window_update_increment が 0 を返し、
/// should_send_window_update が false を返すことを検証する
#[test]
fn prop_recv_window_above_initial() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let send_initial = sample_valid_window_size(ctx);
        let recv_initial = noprop::sample_u64_in(ctx, 1..=1_000_000) as u32;
        let extra = noprop::sample_u64_in(ctx, 1..=1_000_000) as u32;
        let mut fc = FlowControl::with_separate_windows(send_initial, recv_initial);
        // recv_window を recv_initial 超に増加させる
        // recv_initial と extra がともに 1_000_000 以下なので合計は
        // MAX_WINDOW_SIZE (2^31-1) を常に超えない (valid-by-construction)
        fc.add_recv_window(extra).expect("should succeed");
        assert!(!fc.should_send_window_update());
        assert_eq!(fc.window_update_increment(), 0);
        Ok(())
    })?;
    Ok(())
}

/// フロー制御の不変条件テスト
///
/// - 消費量がウィンドウサイズを超えない
/// - WINDOW_UPDATE でオーバーフローしない
///
/// 操作列の長さと各操作の発生回数をゲートで保証し、空シーケンスによる
/// 無検証パスを防ぐ。
#[test]
fn prop_flow_control_invariants() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let consume_gate = std::cell::Cell::new(0usize);
    let window_update_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_window = noprop::sample_u64_in(ctx, 1..=65535) as u32;
        let mut fc = FlowControl::new(initial_window);
        let steps =
            noprop::sample_with_boundaries(ctx, &[1usize, 20], noprop::Ratio::one_nth(5), |ctx| {
                noprop::sample_usize_in(ctx, 1..=20)
            });

        for _ in 0..steps {
            match noprop::sample_weighted_index(ctx, &[2, 1]) {
                0 => {
                    // 送信データ消費: ウィンドウ以内に収まる量だけ消費する
                    let max_consume = fc.send_available();
                    let size = noprop::sample_usize_in(ctx, 0..=max_consume);
                    fc.consume_send(size).expect("should succeed");
                    // 不変条件: 送信ウィンドウは負にならない (消費後も正または 0)
                    assert!(fc.send_window() >= 0);
                    consume_gate.set(consume_gate.get() + 1);
                }
                _ => {
                    // WINDOW_UPDATE: 境界値付きの増分でオーバーフロー経路も探索する
                    let increment = noprop::sample_with_boundaries(
                        ctx,
                        &[1u32, 10_000, MAX_WINDOW_SIZE - 1, MAX_WINDOW_SIZE],
                        noprop::Ratio::one_nth(4),
                        |ctx| 1 + noprop::sample_u64_in(ctx, 0..=10_000u64) as u32,
                    );
                    let _ = fc.recv_window_update(increment);
                    // 不変条件: ウィンドウは MAX_WINDOW_SIZE を超えない
                    assert!(fc.send_window() <= i64::from(MAX_WINDOW_SIZE));
                    assert!(fc.send_window() >= 0);
                    window_update_gate.set(window_update_gate.get() + 1);
                }
            }
        }
        Ok(())
    })?;
    assert!(
        consume_gate.get() > 0,
        "送信消費操作が一度も実行されなかった\n{runner}"
    );
    assert!(
        window_update_gate.get() > 0,
        "WINDOW_UPDATE 操作が一度も実行されなかった\n{runner}"
    );
    Ok(())
}
