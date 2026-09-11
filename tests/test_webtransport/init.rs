//! `WtInit` パーサーと `WtConfig` のピア設定構築規則の単体テスト
//!
//! draft-ietf-webtrans-http2-15 Section 4.3.2 (L583-L604) と RFC 8941
//! Section 4.2 (Parsing Structured Fields) の動作を境界値・型不一致を含めて検証する。
//! 加えて、`WtConfig::peer_default` の Initial Value (Section 11.2) と
//! `apply_init_as_peer` のマージ結果が `WtSession` の送信上限へ反映されることを
//! Sans I/O 層で確認する。

use shiguredo_http2::webtransport::{WtConfig, WtErrorKind, WtInit, WtSession};

/// 正常系: known キー (u/bl/br) を含む Dictionary が全て Some で返ること
#[test]
fn test_parse_all_known_keys() {
    let init =
        WtInit::parse(b"u=100, bl=200, br=300").expect("正常な Dictionary はパースできるはず");
    assert_eq!(init.u, Some(100), "u は 100 が期待値");
    assert_eq!(init.bl, Some(200), "bl は 200 が期待値");
    assert_eq!(init.br, Some(300), "br は 300 が期待値");
}

/// 未知キーが含まれていても known キーだけが抽出されること (Section 4.3.2 L541 の MUST)
#[test]
fn test_parse_ignores_unknown_keys() {
    let init = WtInit::parse(b"u=100, x=999").expect("未知キーは無視されるはず");
    assert_eq!(init.u, Some(100));
    // 未知キー x は WtInit のフィールドには含まれない
    assert_eq!(init.bl, None);
    assert_eq!(init.br, None);
}

/// known キーに付随するパラメータも無視されること (draft-ietf-webtrans-http2-15 Section 4.3.2 の MUST。パラメータの定義は RFC 8941 §3.1.2)
#[test]
fn test_parse_ignores_parameters_on_known_key() {
    let init = WtInit::parse(b"u=100;foo=bar").expect("パラメータは無視されるはず");
    assert_eq!(
        init.u,
        Some(100),
        "パラメータ ;foo=bar が値の取得を阻害しないこと"
    );
}

/// 未知キーに付随するパラメータも無視されること
#[test]
fn test_parse_ignores_parameters_on_unknown_key() {
    let init =
        WtInit::parse(b"x=999;foo=bar, u=100").expect("未知キーのパラメータは無視されるはず");
    assert_eq!(init.u, Some(100));
}

/// 16 桁の数字は RFC 8941 §3.3.1 に従い拒否されること
#[test]
fn test_parse_rejects_integer_exceeding_15_digits() {
    let err =
        WtInit::parse(b"u=1000000000000000").expect_err("16 桁の Integer は WtError を返すべき");
    let msg = format!("{err}");
    assert!(
        msg.contains("exceeds 15 digits") || msg.contains("15"),
        "15 桁制限違反のメッセージを含むこと、実際: {msg}"
    );
}

/// 負値は known キーでは拒否されること (Integer 型ではあるがフロー制御値として無効)
#[test]
fn test_parse_rejects_negative_value_on_known_key() {
    let err = WtInit::parse(b"u=-1").expect_err("負値は WtError を返すべき");
    let msg = format!("{err}");
    assert!(
        msg.contains("non-negative"),
        "非負整数を要求するメッセージを含むこと、実際: {msg}"
    );
}

/// known キーの値が Boolean だった場合は拒否されること
#[test]
fn test_parse_rejects_boolean_value_on_known_key() {
    let err = WtInit::parse(b"u=?1").expect_err("Boolean は known キーには無効");
    let msg = format!("{err}");
    assert!(
        msg.contains("Integer"),
        "Integer を要求するメッセージを含むこと、実際: {msg}"
    );
}

/// known キーの値が String だった場合は拒否されること
#[test]
fn test_parse_rejects_string_value_on_known_key() {
    let err = WtInit::parse(b"u=\"abc\"").expect_err("String は known キーには無効");
    assert!(format!("{err}").contains("Integer"));
}

/// known キーの値が Byte Sequence だった場合は拒否されること
#[test]
fn test_parse_rejects_byte_sequence_value_on_known_key() {
    let err = WtInit::parse(b"u=:YWJj:").expect_err("Byte Sequence は known キーには無効");
    assert!(format!("{err}").contains("Integer"));
}

/// known キーの値が Token だった場合は拒否されること
#[test]
fn test_parse_rejects_token_value_on_known_key() {
    let err = WtInit::parse(b"u=tok").expect_err("Token は known キーには無効");
    assert!(format!("{err}").contains("Integer"));
}

/// known キーの値が Decimal (`1.5`) だった場合は拒否されること
///
/// RFC 8941 §3.3.2 で定義される Decimal は draft-ietf-webtrans-http2-15
/// Section 4.3.2 L530-L537 が要求する Integer 型ではないので拒否する。
#[test]
fn test_parse_rejects_decimal_value_on_known_key() {
    let err = WtInit::parse(b"u=1.5").expect_err("Decimal は known キーには無効");
    assert!(format!("{err}").contains("Integer"));
}

/// 15 桁ちょうどの正値は受理されること (RFC 8941 §3.3.1 の絶対値上限)
#[test]
fn test_parse_accepts_max_15_digits() {
    let init = WtInit::parse(b"u=999999999999999").expect("15 桁ちょうどは受理されるべき");
    assert_eq!(init.u, Some(999_999_999_999_999));
}

/// 重複キーは last-wins で扱われること (RFC 8941 §4.2.2 step 2.4)
#[test]
fn test_parse_duplicate_key_last_wins() {
    let init = WtInit::parse(b"u=10, u=20").expect("重複キーは last-wins でパース成功");
    assert_eq!(init.u, Some(20), "最後に出現した値が採用されるべき");
}

/// 空文字列は全フィールド None の WtInit を返すこと
#[test]
fn test_parse_empty_input_returns_default() {
    let init = WtInit::parse(b"").expect("空文字列は空 Dictionary としてパース成功");
    assert_eq!(init, WtInit::default());
}

/// 末尾カンマは拒否されること (RFC 8941 §4.2.2 step 2.10)
#[test]
fn test_parse_rejects_trailing_comma() {
    let err = WtInit::parse(b"u=100,").expect_err("末尾カンマは拒否されるべき");
    assert!(format!("{err}").contains("trailing comma"));
}

/// 非 ASCII バイトは拒否されること (RFC 8941 §4.2 step 1)
#[test]
fn test_parse_rejects_non_ascii() {
    let err = WtInit::parse(&[b'u', b'=', 0xFF]).expect_err("非 ASCII バイトは拒否されるべき");
    assert!(format!("{err}").contains("non-ASCII"));
}

/// 未知キーが Inner List 値を持っていても読み飛ばせること
#[test]
fn test_parse_skips_inner_list_on_unknown_key() {
    let init = WtInit::parse(b"x=(1 2 3), u=100").expect("未知キーの Inner List は無視されるはず");
    assert_eq!(init.u, Some(100));
}

/// 未知キーの値が "=" 省略 (= Boolean true) でも読み飛ばせること
#[test]
fn test_parse_skips_unknown_boolean_shorthand() {
    let init = WtInit::parse(b"x, u=100").expect("Boolean 省略形は無視されるはず");
    assert_eq!(init.u, Some(100));
}

/// `WtConfig::apply_init_as_peer` が `u`/`bl`/`br` の Some 値で max マージすること
#[test]
fn test_apply_init_as_peer_max_merge() {
    let mut config = WtConfig::default();
    let initial_uni = config.initial_max_stream_data_uni;
    let initial_bidi_local = config.initial_max_stream_data_bidi_local;
    let initial_bidi_remote = config.initial_max_stream_data_bidi_remote;

    // bl と br に異なる値を与え、マッピングの取り違えを検出できるようにする
    let init = WtInit {
        u: Some(initial_uni + 100),
        bl: Some(initial_bidi_local + 200),
        br: Some(initial_bidi_remote + 300),
    };
    config.apply_init_as_peer(&init);
    assert_eq!(config.initial_max_stream_data_uni, initial_uni + 100);
    assert_eq!(
        config.initial_max_stream_data_bidi_local,
        initial_bidi_local + 200,
        "bl は initial_max_stream_data_bidi_local に反映されること"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_remote,
        initial_bidi_remote + 300,
        "br は initial_max_stream_data_bidi_remote に反映されること"
    );
}

/// `WtConfig::apply_init_as_peer` で SETTINGS 由来の値より小さい値は無視されること
#[test]
fn test_apply_init_as_peer_keeps_larger_settings_value() {
    let mut config = WtConfig::default();
    let initial_uni = config.initial_max_stream_data_uni;
    let initial_bidi_local = config.initial_max_stream_data_bidi_local;
    let initial_bidi_remote = config.initial_max_stream_data_bidi_remote;

    // SETTINGS 由来 (default) より小さい値で上書きしようとしても変わらない
    let init = WtInit {
        u: Some(0),
        bl: Some(0),
        br: Some(0),
    };
    config.apply_init_as_peer(&init);
    assert_eq!(
        config.initial_max_stream_data_uni, initial_uni,
        "u の小さい値は max マージで採用されないこと"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_local, initial_bidi_local,
        "bl の小さい値は max マージで採用されないこと"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_remote, initial_bidi_remote,
        "br の小さい値は max マージで採用されないこと"
    );
}

/// `WtConfig::apply_init_as_peer` で `None` キーは設定値に触れないこと
#[test]
fn test_apply_init_as_peer_none_does_not_touch_config() {
    let mut config = WtConfig::default();
    let snapshot = config.clone();
    config.apply_init_as_peer(&WtInit::default());
    assert_eq!(
        config.initial_max_data, snapshot.initial_max_data,
        "WtInit に含まれないフィールドは触られないこと"
    );
    assert_eq!(
        config.initial_max_stream_data_uni, snapshot.initial_max_stream_data_uni,
        "u=None で initial_max_stream_data_uni は変わらないこと"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_local, snapshot.initial_max_stream_data_bidi_local,
        "bl=None で initial_max_stream_data_bidi_local は変わらないこと"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_remote, snapshot.initial_max_stream_data_bidi_remote,
        "br=None で initial_max_stream_data_bidi_remote は変わらないこと"
    );
}

/// 先頭・末尾の SP は破棄されること (RFC 8941 §4.2 step 2 および step 6)
#[test]
fn test_parse_handles_leading_and_trailing_ows() {
    let init = WtInit::parse(b"  u=100, bl=200  ").expect("先頭末尾の OWS は破棄されるべき");
    assert_eq!(init.u, Some(100));
    assert_eq!(init.bl, Some(200));
}

/// `WtConfig::apply_init_as_peer` で更新された `u` 値が
/// `WtSession::server(config, peer_config)` 経由でローカル開始 uni ストリームの
/// 送信上限に反映されること
#[test]
fn test_apply_init_as_peer_propagates_to_uni_stream_send_max() {
    let mut peer_config = WtConfig::default();
    let original = peer_config.initial_max_stream_data_uni;
    let updated = original + 4096;
    peer_config.apply_init_as_peer(&WtInit {
        u: Some(updated),
        ..Default::default()
    });
    let mut session = WtSession::server(WtConfig::default(), peer_config);
    session.initiate().expect("initiate に失敗");
    let stream_id = session.open_uni_stream().expect("uni ストリームを開けない");
    let stream = session.stream(stream_id).expect("ストリームが存在しない");
    assert_eq!(
        stream.send_available(),
        updated,
        "u がピア用 config に反映されたらローカル uni の送信上限も更新されるべき"
    );
}

/// `WtConfig::peer_default` は SETTINGS_WT_INITIAL_MAX_* の Initial Value (全て 0) を返すこと
/// (draft-ietf-webtrans-http2-15 Section 11.2)
#[test]
fn test_peer_default_is_all_zero() {
    let config = WtConfig::peer_default();
    assert_eq!(config.initial_max_data, 0, "initial_max_data は 0 のはず");
    assert_eq!(
        config.initial_max_stream_data_bidi_local, 0,
        "bidi_local は 0 のはず"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_remote, 0,
        "bidi_remote は 0 のはず"
    );
    assert_eq!(config.initial_max_stream_data_uni, 0, "uni は 0 のはず");
    assert_eq!(
        config.initial_max_streams_bidi, 0,
        "streams_bidi は 0 のはず"
    );
    assert_eq!(config.initial_max_streams_uni, 0, "streams_uni は 0 のはず");
}

/// ピアが SETTINGS を広告しない場合、0 を基準に `apply_init_as_peer` が
/// WebTransport-Init のヘッダー値をそのまま採用すること (Section 4.3 / Section 11.2)
#[test]
fn test_peer_default_apply_init_as_peer_adopts_header_values() {
    let mut config = WtConfig::peer_default();
    config.apply_init_as_peer(&WtInit {
        u: Some(1000),
        bl: Some(2000),
        br: Some(3000),
    });
    assert_eq!(
        config.initial_max_stream_data_uni, 1000,
        "u が採用されること"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_local, 2000,
        "bl が採用されること"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_remote, 3000,
        "br が採用されること"
    );
}

/// ピア用 config の初期値が 0 の場合、`WtSession` の送信上限と
/// ストリーム開設可否も 0 / 不可になること (Section 11.2)
#[test]
fn test_peer_default_zeros_session_send_limits() {
    let mut session = WtSession::server(WtConfig::default(), WtConfig::peer_default());
    session.initiate().expect("initiate に失敗");
    assert_eq!(
        session.flow_control().send_available(),
        0,
        "ピアが SETTINGS を広告しない場合のセッション送信上限は 0 のはず"
    );
    assert!(
        !session.flow_control().can_open_bidi_stream(),
        "ピアが広告しない場合に bidi ストリームを開けてはいけない"
    );
    assert!(
        !session.flow_control().can_open_uni_stream(),
        "ピアが広告しない場合に uni ストリームを開けてはいけない"
    );
    assert!(session.open_bidi_stream().is_err());
    assert!(session.open_uni_stream().is_err());
}

/// セッション送信上限が 0 (SETTINGS_WT_INITIAL_MAX_DATA 未広告) でストリーム送信が
/// 失敗しても、ストリームの送信済みバイト数と送信状態が変化しないこと。
///
/// driver はストリームエラー後もセッションを継続するため、部分的な状態変更が残ると
/// 再送時の送信済みバイト数や WT_RESET_STREAM の Reliable Size が実際の送信量とずれる
/// (draft-ietf-webtrans-http2-15 Section 6.2 / Section 6.5)。
#[test]
fn test_peer_default_send_limit_failure_keeps_stream_state() {
    // initial_max_data は未広告 (0)、ストリーム送信上限のみヘッダー相当で許可する
    let mut peer_config = WtConfig::peer_default();
    peer_config.initial_max_stream_data_bidi_remote = 64 * 1024;
    peer_config.initial_max_streams_bidi = 10;
    let mut session = WtSession::server(WtConfig::default(), peer_config);
    session.initiate().expect("initiate に失敗");

    // fin=false の拒否で送信済みバイト数が進まないこと
    let stream_id = session
        .open_bidi_stream()
        .expect("bidi ストリームを開けない");
    let err = session
        .send_stream_data(stream_id, b"x", false)
        .expect_err("セッション送信上限 0 では送信できないはず");
    assert_eq!(
        err.kind(),
        WtErrorKind::FlowControlError,
        "フロー制御エラーが返るはず: {err}"
    );
    let stream = session.stream(stream_id).expect("ストリームが存在しない");
    assert_eq!(
        stream.send_offset(),
        0,
        "送信済みバイト数が進んではいけない"
    );
    assert_eq!(
        stream.send_available(),
        64 * 1024,
        "ストリーム送信上限が消費されてはいけない"
    );

    // fin=true の拒否で送信終端状態に遷移しないこと
    let stream_id2 = session
        .open_bidi_stream()
        .expect("bidi ストリームを開けない");
    let err2 = session
        .send_stream_data(stream_id2, b"y", true)
        .expect_err("セッション送信上限 0 では送信できないはず");
    assert_eq!(err2.kind(), WtErrorKind::FlowControlError);
    let stream2 = session.stream(stream_id2).expect("ストリームが存在しない");
    assert!(
        stream2.can_send(),
        "FIN 送信の拒否で送信状態が終端になってはいけない"
    );
    assert_eq!(stream2.send_offset(), 0);

    // WT_MAX_DATA の受信でセッション送信上限が増えれば再送できること
    session
        .flow_control_mut()
        .update_send_max(1 << 20)
        .expect("send_max の更新に失敗");
    session
        .send_stream_data(stream_id2, b"y", true)
        .expect("送信上限回復後の送信に失敗");
    let stream2 = session.stream(stream_id2).expect("ストリームが存在しない");
    assert_eq!(
        stream2.send_offset(),
        1,
        "再送で送信済みバイト数が更新されること"
    );
}
