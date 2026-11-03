//! SETTINGS 関連の PBT
//!
//! SETTINGS フレームの検証、無効値チェック、変更禁止設定の検出に加え、
//! 未知の識別子の無視、SETTINGS ACK、接続プリフェイスの検出を含む。

use shiguredo_http2::{
    Connection, ErrorCode, Event, Limits, WindowSize,
    frame::{Frame, FrameDecoder, SettingsFrame, StreamId},
    settings::{DEFAULT_INITIAL_WINDOW_SIZE, MAX_INITIAL_WINDOW_SIZE, MAX_MAX_FRAME_SIZE, Setting},
};

use super::encode_frame;

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

/// 蓄積されたイベントをすべて取り出す
fn drain_events(client: &mut Connection) -> Vec<Event> {
    let mut events = Vec::new();
    while let Some(event) = client.poll_event() {
        events.push(event);
    }
    events
}

/// 型検査を通さずに任意の識別子と値を載せた SETTINGS フレームをエンコードする
///
/// 識別子そのものの扱いを検証するため、`Setting::Unknown` を使って `from_wire` を
/// 経由せずに wire 値を直接指定する。
fn encode_raw_setting_frame(id: u16, value: u32) -> Vec<u8> {
    let bytes = encode_frame(&Frame::Settings(SettingsFrame::from_settings([
        Setting::Unknown { id, value },
    ])));
    // エンコーダーが未知の識別子を落としたり書き換えたりする退行を検出する。
    // フレームヘッダーは 9 バイト、SETTINGS のペイロードは 6 バイト固定である。
    assert_eq!(
        &bytes[9..11],
        &id.to_be_bytes(),
        "SETTINGS フレームの識別子が指定値でない"
    );
    assert_eq!(
        &bytes[11..15],
        &value.to_be_bytes(),
        "SETTINGS フレームの値が指定値でない"
    );
    bytes
}

/// 送信バイト列に SETTINGS ACK フレームが含まれるかを返す
fn contains_settings_ack(bytes: &[u8]) -> bool {
    let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
    decoder.feed(bytes);
    while let Some(frame) = decoder
        .decode()
        .expect("送信されたフレームをデコードできなければならない")
    {
        if let Frame::Settings(settings) = frame
            && settings.is_ack()
        {
            return true;
        }
    }
    false
}

/// 最初の SETTINGS を受理した直後の状態を検証する
///
/// Active への遷移、ACK 対象の通知が 1 回だけであること、`ConnectionPreface` の通知、
/// 実際の SETTINGS ACK の送信を確認する。
fn assert_single_settings_ack_and_preface(client: &mut Connection) {
    assert!(
        client.is_active(),
        "最初の SETTINGS の処理で接続が Active にならなければならない"
    );

    let events = drain_events(client);
    let received_count = events
        .iter()
        .filter(|event| matches!(event, Event::SettingsReceived { ack: false }))
        .count();
    assert_eq!(
        received_count, 1,
        "最初の SETTINGS は 1 回だけ ACK の対象として通知されなければならない"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ConnectionPreface)),
        "最初の SETTINGS の処理で ConnectionPreface が通知されなければならない"
    );

    let output = client
        .poll_output()
        .expect("SETTINGS ACK が出力されなければならない");
    assert!(
        contains_settings_ack(&output),
        "最初の SETTINGS にも SETTINGS ACK を送らなければならない"
    );
}

/// NO_RFC7540_PRIORITIES の変更は PROTOCOL_ERROR
///
/// RFC 9218 Section 2.1: この設定は接続中に変更できない
#[test]
fn prop_no_rfc7540_priorities_change_is_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_value = noprop::sample_bool(ctx);
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate should succeed");

        // サーバーから NO_RFC7540_PRIORITIES の SETTINGS を受信
        let mut settings1 = SettingsFrame::new();
        settings1.add(Setting::NoRfc7540Priorities(initial_value));
        let settings1_bytes = encode_frame(&Frame::Settings(settings1));
        client.feed(&settings1_bytes).expect("feed should succeed");
        client.process().expect("process should succeed");

        // サーバーから異なる値の NO_RFC7540_PRIORITIES を受信
        let mut settings2 = SettingsFrame::new();
        settings2.add(Setting::NoRfc7540Priorities(!initial_value));
        let settings2_bytes = encode_frame(&Frame::Settings(settings2));
        client.feed(&settings2_bytes).expect("feed should succeed");

        let result = client.process();
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.is_connection_error());
            assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
        Ok(())
    })?;
    Ok(())
}

/// 任意の SETTINGS シーケンスで true→false が出現した場合にのみ PROTOCOL_ERROR
///
/// RFC 8441 §3 のプロパティを任意のシーケンスで検証する。
/// クライアントロールで実行する（サーバーロールは別テストでカバー）。
#[test]
fn prop_enable_connect_protocol_sequence() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let len = noprop::sample_usize_in(ctx, 1..=10);
        let sequence: Vec<bool> = (0..len).map(|_| noprop::sample_bool(ctx)).collect();
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate must succeed");

        // 最初にデフォルトの SETTINGS を送信して接続を Active にする
        let default_settings = SettingsFrame::new();
        let default_bytes = encode_frame(&Frame::Settings(default_settings));
        client.feed(&default_bytes).expect("feed must succeed");
        client.process().expect("process must succeed");

        // プロパティ: true→false が出現するかどうかを予測
        let mut saw_true = false;
        let mut expect_error = false;
        for &val in &sequence {
            if val {
                saw_true = true;
            } else if saw_true {
                expect_error = true;
                break;
            }
        }

        // シーケンスを 1 つずつ送信
        let mut actual_error = false;
        let mut actual_error_code = None;
        let mut actual_is_connection_error = false;
        for &val in &sequence {
            let mut sf = SettingsFrame::new();
            sf.add(Setting::EnableConnectProtocol(val));
            let bytes = encode_frame(&Frame::Settings(sf));
            client.feed(&bytes).expect("feed must succeed");
            if let Err(e) = client.process() {
                actual_error = true;
                actual_is_connection_error = e.is_connection_error();
                actual_error_code = e.error_code();
                break;
            }
        }

        assert_eq!(
            actual_error, expect_error,
            "sequence={sequence:?}, saw_true={saw_true}, expect_error={expect_error}, actual_error={actual_error}"
        );
        if actual_error {
            assert!(
                actual_is_connection_error,
                "ダウングレードは接続エラーでなければならない"
            );
            assert_eq!(
                actual_error_code,
                Some(ErrorCode::ProtocolError),
                "ダウングレードは PROTOCOL_ERROR でなければならない"
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// サーバーロールでも ENABLE_CONNECT_PROTOCOL ダウングレードを PROTOCOL_ERROR で拒否する
///
/// RFC 8441 §3 はロール非依存。クライアントからの SETTINGS を受信するサーバー側でも
/// 同じチェックが適用される。
#[test]
fn prop_enable_connect_protocol_sequence_server() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let len = noprop::sample_usize_in(ctx, 1..=10);
        let sequence: Vec<bool> = (0..len).map(|_| noprop::sample_bool(ctx)).collect();
        let mut server = Connection::server(Limits::default());
        server.mark_preface_received();
        server.initiate().expect("initiate must succeed");

        // クライアントからの SETTINGS を受信して接続を Active にする
        let default_settings = SettingsFrame::new();
        let default_bytes = encode_frame(&Frame::Settings(default_settings));
        server.feed(&default_bytes).expect("feed must succeed");
        server.process().expect("process must succeed");

        let mut saw_true = false;
        let mut expect_error = false;
        for &val in &sequence {
            if val {
                saw_true = true;
            } else if saw_true {
                expect_error = true;
                break;
            }
        }

        let mut actual_error = false;
        let mut actual_error_code = None;
        let mut actual_is_connection_error = false;
        for &val in &sequence {
            let mut sf = SettingsFrame::new();
            sf.add(Setting::EnableConnectProtocol(val));
            let bytes = encode_frame(&Frame::Settings(sf));
            server.feed(&bytes).expect("feed must succeed");
            if let Err(e) = server.process() {
                actual_error = true;
                actual_is_connection_error = e.is_connection_error();
                actual_error_code = e.error_code();
                break;
            }
        }

        assert_eq!(
            actual_error, expect_error,
            "sequence={sequence:?}, saw_true={saw_true}, expect_error={expect_error}, actual_error={actual_error}"
        );
        if actual_error {
            assert!(
                actual_is_connection_error,
                "ダウングレードは接続エラーでなければならない"
            );
            assert_eq!(
                actual_error_code,
                Some(ErrorCode::ProtocolError),
                "ダウングレードは PROTOCOL_ERROR でなければならない"
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// EnableConnectProtocol を含まない SETTINGS が間に挟まっても追跡フラグは維持される
///
/// RFC 8441 §3: ダウングレード検出は接続ライフタイム全体で追跡する
#[test]
fn prop_enable_connect_protocol_tracking_across_frames() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let gap_count = noprop::sample_usize_in(ctx, 1..=5);
        let header_table_size = noprop::sample_u64_in(ctx, 1024..=65535) as u32;
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate must succeed");

        // ENABLE_CONNECT_PROTOCOL=1 の SETTINGS を受信
        let mut settings1 = SettingsFrame::new();
        settings1.add(Setting::EnableConnectProtocol(true));
        let settings1_bytes = encode_frame(&Frame::Settings(settings1));
        client.feed(&settings1_bytes).expect("feed must succeed");
        client.process().expect("process must succeed");

        // EnableConnectProtocol を含まない SETTINGS を gap_count 回受信
        for _ in 0..gap_count {
            let mut gap_sf = SettingsFrame::new();
            gap_sf.add(Setting::HeaderTableSize(header_table_size));
            let gap_bytes = encode_frame(&Frame::Settings(gap_sf));
            client.feed(&gap_bytes).expect("feed must succeed");
            client.process().expect("process must succeed");
        }

        // ダウングレードを送信
        let mut settings3 = SettingsFrame::new();
        settings3.add(Setting::EnableConnectProtocol(false));
        let settings3_bytes = encode_frame(&Frame::Settings(settings3));
        client.feed(&settings3_bytes).expect("feed must succeed");

        let result = client.process();
        assert!(
            result.is_err(),
            "フラグは間に挟まる SETTINGS でリセットされてはならない"
        );
        if let Err(e) = result {
            assert!(e.is_connection_error());
            assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
        Ok(())
    })?;
    Ok(())
}

/// 同一 SETTINGS フレーム内の EnableConnectProtocol 重複は出現順に処理される
///
/// RFC 9113 §6.5: 同一パラメータの重複は出現順で処理される。
/// true→false の順なら PROTOCOL_ERROR、false→true の順なら成功。
#[test]
fn prop_enable_connect_protocol_intra_frame_duplicate() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let first = noprop::sample_bool(ctx);
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate must succeed");

        let second = !first;
        let mut sf = SettingsFrame::new();
        sf.add(Setting::EnableConnectProtocol(first));
        sf.add(Setting::EnableConnectProtocol(second));
        let bytes = encode_frame(&Frame::Settings(sf));
        client.feed(&bytes).expect("feed must succeed");

        let result = client.process();

        // first=true, second=false → true→false でダウングレード → PROTOCOL_ERROR
        // first=false, second=true → false→true で有効化 → 成功
        if first {
            assert!(
                result.is_err(),
                "true→false は PROTOCOL_ERROR でなければならない"
            );
            if let Err(e) = result {
                assert!(e.is_connection_error());
                assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
            }
        } else {
            assert!(result.is_ok(), "false→true は成功しなければならない");
        }
        Ok(())
    })?;
    Ok(())
}

/// `send_settings()` (preface 外部処理経路) でも同じ WINDOW_UPDATE が送信される
#[test]
fn prop_send_settings_emits_connection_window_update() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let size = (DEFAULT_INITIAL_WINDOW_SIZE + 1)
            + noprop::sample_u64_in(
                ctx,
                0..=(MAX_INITIAL_WINDOW_SIZE - DEFAULT_INITIAL_WINDOW_SIZE) as u64,
            ) as u32;
        let window = WindowSize::from_static(size);
        let limits = Limits::builder()
            .connection_window_size(window)
            .build()
            .expect("valid limits");
        let mut server = Connection::server(limits);
        server.mark_preface_received();
        server.send_settings().expect("send_settings");

        let output = server.poll_output().expect("output must contain settings");

        let mut decoder = FrameDecoder::new(MAX_MAX_FRAME_SIZE);
        decoder.feed(&output);

        let mut found = None;
        while let Some(frame) = decoder.decode().expect("decode frame") {
            if let Frame::WindowUpdate(wu) = frame
                && matches!(wu.stream_id, StreamId::Connection)
            {
                found = Some(wu.window_size_increment.as_u32());
            }
        }
        let expected = size - DEFAULT_INITIAL_WINDOW_SIZE;
        assert_eq!(
            found,
            Some(expected),
            "expected connection-level WINDOW_UPDATE with increment {expected}",
        );
        Ok(())
    })?;
    Ok(())
}

/// SETTINGS_WT_ENABLED の 1→0 ダウングレードは許可される
///
/// draft-ietf-webtrans-http2-16 Section 3.1: SETTINGS_WT_ENABLED は接続ライフタイム中に
/// 更新可能。サーバーが WebTransport を無効化しても既存セッションには影響しない。
/// ENABLE_CONNECT_PROTOCOL と異なり、1→0 のダウングレードは合法。
#[test]
fn prop_wt_enabled_downgrade_allowed() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_value = noprop::sample_bool(ctx);
        let mut client = Connection::client(Limits::default());
        client.initiate().expect("initiate must succeed");

        // 最初の SETTINGS を受信して接続を Active にする
        let default_settings = SettingsFrame::new();
        let default_bytes = encode_frame(&Frame::Settings(default_settings));
        client.feed(&default_bytes).expect("feed must succeed");
        client.process().expect("process must succeed");

        // WT_ENABLED=initial_value の SETTINGS を受信
        let mut settings1 = SettingsFrame::new();
        settings1.add(Setting::WtEnabled(initial_value));
        let settings1_bytes = encode_frame(&Frame::Settings(settings1));
        client.feed(&settings1_bytes).expect("feed must succeed");
        client.process().expect("process must succeed");

        // WT_ENABLED=!initial_value の SETTINGS を受信 (ダウングレードまたはアップグレード)
        let mut settings2 = SettingsFrame::new();
        settings2.add(Setting::WtEnabled(!initial_value));
        let settings2_bytes = encode_frame(&Frame::Settings(settings2));
        client.feed(&settings2_bytes).expect("feed must succeed");

        // 1→0 も 0→1 もエラーにならない
        let result = client.process();
        assert!(
            result.is_ok(),
            "WT_ENABLED の 1→0 ダウングレードは許可されなければならない (initial={initial_value})"
        );
        Ok(())
    })?;
    Ok(())
}

/// SETTINGS_WT_ENABLED の 1 より大きい値は PROTOCOL_ERROR
///
/// draft-ietf-webtrans-http2-16 Section 3.1: クライアントは 1 より大きい値を
/// 接続エラー PROTOCOL_ERROR として扱わなければならない (MUST)。
#[test]
fn prop_wt_enabled_greater_than_one_is_error() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = 2 + noprop::sample_u64_in(ctx, 0..(u32::MAX as u64 - 1)) as u32;
        let mut client = Connection::client(Limits::default());
        client
            .initiate()
            .expect("接続の初期化に成功しなければならない");

        // 最初の SETTINGS を受信して接続を Active にする
        let default_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
        client
            .feed(&default_bytes)
            .expect("SETTINGS の受信に成功しなければならない");
        client
            .process()
            .expect("SETTINGS の処理に成功しなければならない");

        // SETTINGS_WT_ENABLED (0x2b67) に 1 より大きい値を設定する
        let raw = encode_raw_setting_frame(0x2b67, value);

        client
            .feed(&raw)
            .expect("SETTINGS の受信に成功しなければならない");

        let result = client.process();
        assert!(
            result.is_err(),
            "WT_ENABLED={value} は PROTOCOL_ERROR でなければならない"
        );
        if let Err(e) = result {
            assert!(e.is_connection_error());
            assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
        }
        Ok(())
    })?;
    Ok(())
}

/// SETTINGS_WT_ENABLED の旧識別子 (0x2b60) は未知の SETTINGS として無視される
///
/// draft-ietf-webtrans-http2-16 Section 9.1: 非互換な変更では SETTINGS_WT_ENABLED の
/// 識別子を変更する。draft-ietf-webtrans-http2-15 の 0x2b60 は未知の SETTINGS として
/// 無視し (RFC 9113 Section 6.5.2)、WebTransport サポートの有効化にも値域違反にもしない。
/// 無効状態から有効化されないことに加え、有効状態から無効化されないことも検証する。
#[test]
fn prop_old_wt_enabled_id_ignored() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 旧識別子を適用する前に新しい識別子で有効化しておくかを決める
        let enabled_first = noprop::sample_bool(ctx);
        // 値域違反になり得る境界値と任意値を混ぜて旧識別子の値を生成する
        let value = noprop::sample_with_boundaries(
            ctx,
            &[0u32, 1, 2, u32::MAX],
            noprop::Ratio::one_nth(4),
            noprop::sample_u32,
        );
        let mut client = Connection::client(Limits::default());
        client
            .initiate()
            .expect("接続の初期化に成功しなければならない");

        // 最初の SETTINGS を受信して接続を Active にする
        let default_bytes = encode_frame(&Frame::Settings(SettingsFrame::new()));
        client
            .feed(&default_bytes)
            .expect("SETTINGS の受信に成功しなければならない");
        client
            .process()
            .expect("SETTINGS の処理に成功しなければならない");

        // 新しい識別子 (0x2b67) で WebTransport サポートを有効化する
        if enabled_first {
            let enable = encode_raw_setting_frame(0x2b67, 1);
            client
                .feed(&enable)
                .expect("SETTINGS の受信に成功しなければならない");
            client
                .process()
                .expect("SETTINGS の処理に成功しなければならない");
        }
        let before = client.remote_settings().wt_enabled();
        assert_eq!(
            before, enabled_first,
            "新しい識別子 (0x2b67) の適用結果が期待と異なる (enabled_first={enabled_first})"
        );

        // これまでの SETTINGS 処理で溜まったイベントと送信出力を破棄し、
        // 旧識別子のフレームだけを評価できるようにする
        let _ = drain_events(&mut client);
        let _ = client.poll_output();

        // SETTINGS_WT_ENABLED の旧識別子 (0x2b60) を送信する
        let raw = encode_raw_setting_frame(0x2b60, value);
        client
            .feed(&raw)
            .expect("SETTINGS の受信に成功しなければならない");
        client
            .process()
            .expect("旧識別子は未知の SETTINGS として無視されなければならない");

        // RFC 9113 Section 6.5.3: 未知の SETTINGS を無視した場合も ACK を送らなければならない
        let events = drain_events(&mut client);
        let received_count = events
            .iter()
            .filter(|event| matches!(event, Event::SettingsReceived { ack: false }))
            .count();
        assert_eq!(
            received_count, 1,
            "旧識別子のみの SETTINGS も 1 回だけ ACK の対象として通知されなければならない"
        );

        // ACK フレームそのものが送信されていることも確認する
        let output = client
            .poll_output()
            .expect("旧識別子の SETTINGS に対する ACK が出力されなければならない");
        assert!(
            contains_settings_ack(&output),
            "未知の SETTINGS のみのフレームにも SETTINGS ACK を送らなければならない"
        );

        // 旧識別子は WebTransport サポートの状態を変更しない
        assert_eq!(
            client.remote_settings().wt_enabled(),
            before,
            "旧識別子 0x2b60 (value={value}) で WebTransport サポートの状態が変化してはならない"
        );
        Ok(())
    })?;
    Ok(())
}

/// 最初の SETTINGS フレームでの SETTINGS_WT_ENABLED の新旧識別子の扱いを検証する
///
/// draft-ietf-webtrans-http2-16 Section 3.1: 新しい識別子の値が 1 より大きい場合は
/// 接続エラー PROTOCOL_ERROR とする。接続エラー時は SETTINGS ACK を送らない
/// (RFC 9113 Section 6.5.3 の ACK 規則に基づく実装判断)。
/// draft-ietf-webtrans-http2-15 の旧識別子は未知の SETTINGS として無視し、
/// 接続を Active にして ACK を送る (RFC 9113 Section 6.5.2 / Section 6.5.3)。
#[test]
fn prop_initial_settings_wt_enabled_identifier_handling() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 最初の SETTINGS に載せる内容を選ぶ
        // 0 = 旧識別子 (未知の SETTINGS として無視) / 1 = 新しい識別子の有効値 1 /
        // 2 = 新しい識別子の値域違反 (2 以上)
        let scenario = noprop::sample_weighted_index(ctx, &[1, 1, 1]);
        let mut client = Connection::client(Limits::default());
        client
            .initiate()
            .expect("接続の初期化に成功しなければならない");
        // 接続前の出力 (preface と自分の SETTINGS) を捨てる
        let _ = client.poll_output();

        match scenario {
            0 => {
                // 最初の SETTINGS に旧識別子を載せても未知の SETTINGS として無視される。
                // 値域違反になり得る境界値と任意値を混ぜる
                let old_value = noprop::sample_with_boundaries(
                    ctx,
                    &[0u32, 1, 2, u32::MAX],
                    noprop::Ratio::one_nth(4),
                    noprop::sample_u32,
                );
                let raw = encode_raw_setting_frame(0x2b60, old_value);
                client
                    .feed(&raw)
                    .expect("SETTINGS の受信に成功しなければならない");
                client
                    .process()
                    .expect("旧識別子は未知の SETTINGS として無視されなければならない");

                assert!(
                    !client.remote_settings().wt_enabled(),
                    "旧識別子 0x2b60 (value={old_value}) で有効化されてはならない"
                );
                assert_single_settings_ack_and_preface(&mut client);
            }
            1 => {
                // 最初の SETTINGS に新しい識別子の有効値 1 を載せると有効化される
                let raw = encode_raw_setting_frame(0x2b67, 1);
                client
                    .feed(&raw)
                    .expect("SETTINGS の受信に成功しなければならない");
                client
                    .process()
                    .expect("新しい識別子の有効値は受理されなければならない");

                assert!(
                    client.remote_settings().wt_enabled(),
                    "新しい識別子 0x2b67=1 で有効化されなければならない"
                );
                assert_single_settings_ack_and_preface(&mut client);
            }
            _ => {
                // 最初の SETTINGS に新しい識別子の値域違反を載せると接続エラーになる
                let invalid_value = 2 + noprop::sample_u64_in(ctx, 0..(u32::MAX as u64 - 1)) as u32;
                let raw = encode_raw_setting_frame(0x2b67, invalid_value);
                client
                    .feed(&raw)
                    .expect("SETTINGS の受信に成功しなければならない");
                let result = client.process();
                assert!(
                    result.is_err(),
                    "WT_ENABLED={invalid_value} は最初の SETTINGS でも接続エラーでなければならない"
                );
                if let Err(e) = result {
                    assert!(e.is_connection_error());
                    assert_eq!(e.error_code(), Some(ErrorCode::ProtocolError));
                }
                assert!(
                    !client.is_active(),
                    "接続エラー時は接続が Active になってはならない"
                );

                let events = drain_events(&mut client);
                assert!(
                    !events
                        .iter()
                        .any(|event| matches!(event, Event::ConnectionPreface)),
                    "接続エラー時は ConnectionPreface を通知してはならない"
                );
                if let Some(output) = client.poll_output() {
                    assert!(
                        !contains_settings_ack(&output),
                        "接続エラー時は SETTINGS ACK を送ってはならない"
                    );
                }
            }
        }
        Ok(())
    })?;
    Ok(())
}
