//! WebTransport-Init パーサーの PBT
//!
//! RFC 9651 (Structured Field Values for HTTP) の Dictionary として妥当な入力を
//! 生成し、既知キー (`u` / `bl` / `br`) の抽出と、Date / Display String を含む
//! 未知値の読み飛ばしを検証する。
//! 加えて、既知キーへの型違反と、Date / Display String の構文違反の負例を検証する。

use std::cell::Cell;

use shiguredo_http2::webtransport::WtInit;

use super::{CASES, SEED_ENV};

/// RFC 9651 の Integer として妥当な最大値 (15 桁)
const SF_INTEGER_MAX: u64 = 999_999_999_999_999;

/// 生成した bare item に含まれた型を記録するフラグ
#[derive(Debug, Default, Clone, Copy)]
struct GeneratedKinds {
    date: bool,
    display_string: bool,
}

/// RFC 9651 の bare item (Inner List を除く) として妥当な文字列を生成する
fn sample_simple_bare_item(
    ctx: &mut noprop::TestCaseContext,
    kinds: &mut GeneratedKinds,
) -> Vec<u8> {
    match noprop::sample_weighted_index(ctx, &[1, 1, 1, 1, 1, 1, 1, 1]) {
        // Integer (負値も含む)
        0 => {
            let value = noprop::sample_u64_in(ctx, 0..=SF_INTEGER_MAX);
            if noprop::sample_bool(ctx) {
                format!("-{value}").into_bytes()
            } else {
                format!("{value}").into_bytes()
            }
        }
        // Decimal (負値も有効)
        1 => {
            let int = noprop::sample_u64_in(ctx, 0..=999_999_999_999);
            let frac = noprop::sample_u64_in(ctx, 0..=999);
            if noprop::sample_bool(ctx) {
                format!("-{int}.{frac:03}").into_bytes()
            } else {
                format!("{int}.{frac:03}").into_bytes()
            }
        }
        // String (VCHAR / SP のみ)
        2 => {
            let len = noprop::sample_usize_in(ctx, 0..=6);
            let mut out = vec![b'"'];
            for _ in 0..len {
                let b = noprop::sample_u64_in(ctx, 0x20..=0x7e) as u8;
                // DQUOTE は文字列を終端し、バックスラッシュはエスケープを要求するため
                // 単純な文字に置換して妥当な String を生成する
                let b = match b {
                    b'"' | b'\\' => b'x',
                    other => other,
                };
                out.push(b);
            }
            out.push(b'"');
            out
        }
        // Token
        3 => {
            let len = noprop::sample_usize_in(ctx, 0..=6);
            let mut out = vec![b'*'];
            for _ in 0..len {
                let b = noprop::sample_u64_in(ctx, 0..=((b'z' - b'a') as u64)) as u8;
                out.push(b'a' + b);
            }
            out
        }
        // Byte Sequence (base64 として妥当な固定形から選ぶ)
        // パーサーは base64 デコードを検証しない既知の制限があるため、
        // ここでは構文上の代表例のみを生成する
        4 => {
            let candidates: [&[u8]; 5] = [b"::", b":AA==:", b":AAA=:", b":AAAA:", b":/w==:"];
            candidates[noprop::sample_usize_in(ctx, 0..=candidates.len() - 1)].to_vec()
        }
        // Boolean
        5 => {
            if noprop::sample_bool(ctx) {
                b"?1".to_vec()
            } else {
                b"?0".to_vec()
            }
        }
        // Date (負値も有効)
        6 => {
            kinds.date = true;
            let value = noprop::sample_u64_in(ctx, 0..=SF_INTEGER_MAX);
            if noprop::sample_bool(ctx) {
                format!("@-{value}").into_bytes()
            } else {
                format!("@{value}").into_bytes()
            }
        }
        // Display String
        _ => {
            kinds.display_string = true;
            sample_display_string(ctx)
        }
    }
}

/// RFC 9651 の member value (bare item または Inner List) として妥当な文字列を生成する
fn sample_member_value(ctx: &mut noprop::TestCaseContext, kinds: &mut GeneratedKinds) -> Vec<u8> {
    if noprop::sample_ratio(ctx, noprop::Ratio::one_nth(8)) {
        // Inner List の要素には Inner List を入れない (入れ子は不正)
        let len = noprop::sample_usize_in(ctx, 0..=3);
        let mut out = vec![b'('];
        for i in 0..len {
            if i > 0 {
                out.push(b' ');
            }
            out.extend(sample_simple_bare_item(ctx, kinds));
        }
        out.push(b')');
        out
    } else {
        sample_simple_bare_item(ctx, kinds)
    }
}

/// RFC 9651 の Display String として妥当な文字列を生成する
fn sample_display_string(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    let len = noprop::sample_usize_in(ctx, 0..=6);
    let mut out = vec![b'%', b'"'];
    for _ in 0..len {
        if noprop::sample_ratio(ctx, noprop::Ratio::one_nth(5)) {
            // 生のバックスラッシュはエスケープではなく内容として扱われる
            out.push(b'\\');
            continue;
        }
        let b = noprop::sample_u64_in(ctx, 0..=((b'z' - b'a') as u64)) as u8;
        out.push(b'a' + b);
    }
    // 末尾に percent encoding された UTF-8 (é) を付ける場合がある
    if noprop::sample_ratio(ctx, noprop::Ratio::one_nth(3)) {
        out.extend_from_slice(b"%c3%a9");
    }
    out.push(b'"');
    out
}

/// 妥当な Dictionary と、既知キーの期待値、Date / Display String を生成したかを返す
fn sample_dictionary(ctx: &mut noprop::TestCaseContext) -> (Vec<u8>, WtInit, GeneratedKinds) {
    let mut known_members: Vec<Vec<u8>> = Vec::new();
    let mut unknown_members: Vec<Vec<u8>> = Vec::new();
    let mut expected = WtInit::default();
    let mut kinds = GeneratedKinds::default();

    // 既知キーを 0〜3 個含める
    for (key, slot) in [(b"u".as_slice(), 0usize), (b"bl", 1), (b"br", 2)] {
        if !noprop::sample_ratio(ctx, noprop::Ratio::one_nth(2)) {
            continue;
        }
        let value = noprop::sample_u64_in(ctx, 0..=SF_INTEGER_MAX);
        let mut member = Vec::new();
        member.extend_from_slice(key);
        member.push(b'=');
        member.extend_from_slice(value.to_string().as_bytes());
        match slot {
            0 => expected.u = Some(value),
            1 => expected.bl = Some(value),
            _ => expected.br = Some(value),
        }
        // 既知キーにパラメータを付ける場合がある (未知パラメータは無視される)
        if noprop::sample_ratio(ctx, noprop::Ratio::one_nth(3)) {
            member.extend_from_slice(b";p=");
            // パラメータの値は bare item のみ (Inner List は入れられない)
            member.extend(sample_simple_bare_item(ctx, &mut kinds));
        }
        known_members.push(member);
    }

    // 未知キーを 0〜3 個含める (Date / Display String を含む)
    for index in 0..noprop::sample_usize_in(ctx, 0..=3) {
        let mut member = format!("x{index}=").into_bytes();
        member.extend(sample_member_value(ctx, &mut kinds));
        if noprop::sample_ratio(ctx, noprop::Ratio::one_nth(3)) {
            member.extend_from_slice(b";p=");
            // パラメータの値は bare item のみ (Inner List は入れられない)
            member.extend(sample_simple_bare_item(ctx, &mut kinds));
        }
        unknown_members.push(member);
    }

    // 未知メンバーを先に置く場合も作る (新型の読み飛ばしが後続の既知キーを壊さない)
    let mut members = if noprop::sample_bool(ctx) {
        let mut members = unknown_members;
        members.extend(known_members);
        members
    } else {
        let mut members = known_members;
        members.extend(unknown_members);
        members
    };
    let mut text = Vec::new();
    for (index, member) in members.drain(..).enumerate() {
        if index > 0 {
            text.extend_from_slice(b", ");
        }
        text.extend(member);
    }

    // 空の Dictionary も対象にする
    (text, expected, kinds)
}

/// 妥当な Dictionary をパースでき、既知キーの値が一致することを検証する
#[test]
fn prop_wt_init_accepts_valid_dictionary() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    let date_gate = Cell::new(0u32);
    let display_gate = Cell::new(0u32);
    runner.run(CASES, |ctx| {
        let (text, expected, kinds) = sample_dictionary(ctx);
        if kinds.date {
            date_gate.set(date_gate.get() + 1);
        }
        if kinds.display_string {
            display_gate.set(display_gate.get() + 1);
        }
        let parsed = WtInit::parse(&text).unwrap_or_else(|err| {
            panic!(
                "妥当な Dictionary は受理されるはず: {err:?} input={}",
                String::from_utf8_lossy(&text)
            )
        });
        assert_eq!(parsed.u, expected.u, "u の値が一致しなければならない");
        assert_eq!(parsed.bl, expected.bl, "bl の値が一致しなければならない");
        assert_eq!(parsed.br, expected.br, "br の値が一致しなければならない");
        Ok(())
    })?;
    assert!(date_gate.get() > 0, "Date を含む入力が必要\n{runner}");
    assert!(
        display_gate.get() > 0,
        "Display String を含む入力が必要\n{runner}"
    );
    Ok(())
}

/// 既知キーに Integer 以外を指定した Dictionary を拒否することを検証する
#[test]
fn prop_wt_init_rejects_non_integer_for_known_key() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let key = match noprop::sample_usize_in(ctx, 0..=2) {
            0 => b"u".as_slice(),
            1 => b"bl",
            _ => b"br",
        };
        // Integer / Decimal 以外の値を生成する (なった場合は Date に置き換える)
        let mut kinds = GeneratedKinds::default();
        let mut item = sample_member_value(ctx, &mut kinds);
        if item
            .first()
            .is_some_and(|b| b.is_ascii_digit() || *b == b'-')
        {
            item = b"@0".to_vec();
        }
        let mut text = Vec::new();
        text.extend_from_slice(key);
        text.push(b'=');
        text.extend(item);
        let err = WtInit::parse(&text).expect_err("既知キーの型違反は拒否されるはず");
        assert!(
            format!("{err}").contains("must be an Integer"),
            "Integer 型違反のメッセージを含むこと、入力: {}、実際: {err}",
            String::from_utf8_lossy(&text)
        );
        Ok(())
    })?;
    Ok(())
}

/// 妥当な形式から 1 箇所だけ壊した Date / Display String を拒否することを検証する
///
/// RFC 9651 Section 4.2.9 / Section 4.2.10。固定リテラルではなく、
/// ASCII 小文字のランダムな接頭辞を付けて入力の多様性を確保する。
#[test]
fn prop_wt_init_rejects_mutated_new_types() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let len = noprop::sample_usize_in(ctx, 0..=4);
        let mut prefix = String::new();
        for _ in 0..len {
            let b = noprop::sample_u64_in(ctx, 0..=((b'z' - b'a') as u64)) as u8;
            prefix.push((b'a' + b) as char);
        }

        // Date の負例: Decimal か数値なし
        let invalid_date = if noprop::sample_bool(ctx) {
            let int = noprop::sample_u64_in(ctx, 0..=999_999_999_999);
            let frac = noprop::sample_u64_in(ctx, 0..=999);
            format!("x{prefix}=@{int}.{frac:03}").into_bytes()
        } else if noprop::sample_bool(ctx) {
            format!("x{prefix}=@").into_bytes()
        } else {
            format!("x{prefix}=@z").into_bytes()
        };
        let err = WtInit::parse(&invalid_date).expect_err("不正な Date は拒否されるはず");
        let msg = format!("{err}");
        assert!(
            msg.contains("date") || msg.contains("integer"),
            "Date の構文エラーであること、入力: {}、実際: {err}",
            String::from_utf8_lossy(&invalid_date)
        );

        // Display String の負例: 大文字 16 進数 / 終端欠落 / 非 16 進数 / 不正 UTF-8
        let (invalid_display, expected_reason) = match noprop::sample_usize_in(ctx, 0..=3) {
            0 => (
                format!("x{prefix}=%\"{prefix}%AB\"").into_bytes(),
                "lowercase hexadecimal",
            ),
            1 => (
                format!("x{prefix}=%\"{prefix}").into_bytes(),
                "unterminated display string",
            ),
            2 => (
                format!("x{prefix}=%\"{prefix}%zz\"").into_bytes(),
                "lowercase hexadecimal",
            ),
            _ => (
                format!("x{prefix}=%\"%ff{prefix}\"").into_bytes(),
                "not valid UTF-8",
            ),
        };
        let err =
            WtInit::parse(&invalid_display).expect_err("不正な Display String は拒否されるはず");
        assert!(
            format!("{err}").contains(expected_reason),
            "期待する理由 ({expected_reason}) を含むこと、入力: {}、実際: {err}",
            String::from_utf8_lossy(&invalid_display)
        );
        Ok(())
    })?;
    Ok(())
}
