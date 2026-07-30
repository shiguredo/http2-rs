//! `WtInit` パーサーと `WtConfig::apply_init` マージ規則の単体テスト
//!
//! draft-ietf-webtrans-http2-15 Section 4.3.2 (L519-L541) と RFC 8941
//! Section 4.2 (Parsing Structured Fields) の動作を境界値・型不一致を含めて検証する。
//! 加えて、`apply_init` 後の `WtConfig` が `WtSession` の初期最大データ量に
//! 反映されることを Sans I/O 層で確認する。

use shiguredo_http2::webtransport::{WtConfig, WtInit, WtSession};

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

/// `WtConfig::apply_init` が `u`/`bl`/`br` の Some 値で max マージすること
#[test]
fn test_apply_init_max_merge() {
    let mut config = WtConfig::default();
    let initial_uni = config.initial_max_stream_data_uni;
    let initial_bidi_remote = config.initial_max_stream_data_bidi_remote;
    let initial_bidi_local = config.initial_max_stream_data_bidi_local;

    // SETTINGS 由来の値より大きい値で上書きされる
    let init = WtInit {
        u: Some(initial_uni + 100),
        bl: Some(initial_bidi_remote + 200),
        br: Some(initial_bidi_local + 300),
    };
    config.apply_init(&init);
    assert_eq!(config.initial_max_stream_data_uni, initial_uni + 100);
    assert_eq!(
        config.initial_max_stream_data_bidi_remote,
        initial_bidi_remote + 200
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_local,
        initial_bidi_local + 300
    );
}

/// `WtConfig::apply_init` で SETTINGS 由来の値より小さい値は無視されること
#[test]
fn test_apply_init_keeps_larger_settings_value() {
    let mut config = WtConfig::default();
    let initial_uni = config.initial_max_stream_data_uni;

    // SETTINGS 由来 (default) より小さい値で上書きしようとしても変わらない
    let init = WtInit {
        u: Some(0),
        ..Default::default()
    };
    config.apply_init(&init);
    assert_eq!(
        config.initial_max_stream_data_uni, initial_uni,
        "max マージで小さい値は採用されないこと"
    );
}

/// `WtConfig::apply_init` で `None` キーは設定値に触れないこと
#[test]
fn test_apply_init_none_does_not_touch_config() {
    let mut config = WtConfig::default();
    let snapshot = config.clone();
    config.apply_init(&WtInit::default());
    assert_eq!(
        config.initial_max_data, snapshot.initial_max_data,
        "WtInit に含まれないフィールドは触られないこと"
    );
    assert_eq!(
        config.initial_max_stream_data_uni, snapshot.initial_max_stream_data_uni,
        "u=None で initial_max_stream_data_uni は変わらないこと"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_remote, snapshot.initial_max_stream_data_bidi_remote,
        "bl=None で initial_max_stream_data_bidi_remote は変わらないこと"
    );
    assert_eq!(
        config.initial_max_stream_data_bidi_local, snapshot.initial_max_stream_data_bidi_local,
        "br=None で initial_max_stream_data_bidi_local は変わらないこと"
    );
}

/// 先頭・末尾の SP は破棄されること (RFC 8941 §4.2 step 2 および step 6)
#[test]
fn test_parse_handles_leading_and_trailing_ows() {
    let init = WtInit::parse(b"  u=100, bl=200  ").expect("先頭末尾の OWS は破棄されるべき");
    assert_eq!(init.u, Some(100));
    assert_eq!(init.bl, Some(200));
}

/// `apply_init` で更新された `u` 値が `WtSession::server(config.clone(), config)` 経由で
/// 単方向ストリームの初期最大データ量に反映されること
#[test]
fn test_apply_init_propagates_to_uni_stream_initial_max() {
    let mut config = WtConfig::default();
    let original = config.initial_max_stream_data_uni;
    let updated = original + 4096;
    config.apply_init(&WtInit {
        u: Some(updated),
        ..Default::default()
    });
    // サーバーセッションを構築し initiate してから自身で uni ストリームを開く
    let mut session = WtSession::server(config.clone(), config);
    session.initiate().expect("initiate");
    let stream_id = session.open_uni_stream().expect("open uni");
    let stream = session.stream(stream_id).expect("stream");
    assert_eq!(
        stream.send_available(),
        updated,
        "u が反映されたら uni ストリームの初期最大データ量も更新されるべき"
    );
}

/// `apply_init` で更新された `br` 値が自身が開く双方向ストリームの
/// 受信上限 (recv_max) に反映されること。
/// 新モデルでは `br` → `config.bidi_local` → locally-opened ストリームの recv_max に対応する。
#[test]
fn test_apply_init_propagates_to_bidi_local_stream_initial_max() {
    let mut config = WtConfig::default();
    let original = config.initial_max_stream_data_bidi_local;
    let updated = original + 8192;
    config.apply_init(&WtInit {
        br: Some(updated),
        ..Default::default()
    });
    let mut session = WtSession::server(config.clone(), config);
    session.initiate().expect("initiate");
    let stream_id = session.open_bidi_stream().expect("open bidi");
    let stream = session.stream(stream_id).expect("stream");
    assert_eq!(
        stream.recv_available(),
        updated,
        "br が反映されたら自身が開く双方向ストリームの受信上限も更新されるべき"
    );
}

/// `apply_init` で `u` を SETTINGS デフォルト未満に指定した場合、
/// `WtSession` 経由のストリーム初期値は SETTINGS デフォルトのまま維持されること
#[test]
fn test_apply_init_smaller_value_keeps_default_in_session() {
    let mut config = WtConfig::default();
    let default_uni = config.initial_max_stream_data_uni;
    config.apply_init(&WtInit {
        u: Some(0),
        ..Default::default()
    });
    let mut session = WtSession::server(config.clone(), config);
    session.initiate().expect("initiate");
    let stream_id = session.open_uni_stream().expect("open uni");
    let stream = session.stream(stream_id).expect("stream");
    assert_eq!(
        stream.send_available(),
        default_uni,
        "u=0 で SETTINGS デフォルトを下回らないこと"
    );
}
