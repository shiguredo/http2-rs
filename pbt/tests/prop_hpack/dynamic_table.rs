//! HPACK 動的テーブルの PBT (RFC 7541 Section 2.3.2)
//!
//! HPACK 動的テーブルの不変条件を検証する。

use shiguredo_http2::hpack::DynamicTable;

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
const CASES: usize = 256;

const TOKEN_CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-_";

/// HPACK 動的テーブルに挿入可能な field-name を生成する
/// (token-lowercase + 数字 + '-' + '_', 非空)
fn sample_name(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    let len = noprop::sample_usize_in(ctx, 1..=99);
    (0..len)
        .map(|_| noprop::sample_choice(ctx, TOKEN_CHARSET))
        .collect()
}

/// HPACK 動的テーブルに挿入可能な field-value を生成する
///
/// RFC 9113 §8.2.1: visible ASCII + 内部 SP/HTAB 許容、両端 SP/HTAB は除去、
/// NUL/CR/LF は構築時検査で禁止。
fn sample_value(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    let len = noprop::sample_usize_in(ctx, 0..=99);
    let v: Vec<u8> = (0..len)
        .map(|_| noprop::sample_u64_in(ctx, 0x20..=0x7E) as u8)
        .collect();
    v.iter()
        .position(|&b| b != 0x20 && b != 0x09)
        .map(|start| {
            let end = v
                .iter()
                .rposition(|&b| b != 0x20 && b != 0x09)
                .expect("should succeed");
            v[start..=end].to_vec()
        })
        .unwrap_or_default()
}

/// テーブル操作の 1 つを生成して適用・検証する
///
/// `insert_gate` は挿入が実際に成功した回数を数える (サイズ不変条件が
/// 空シーケンスで無駄に成立するのを防ぐ)。
fn sample_table_op(
    ctx: &mut noprop::TestCaseContext,
    table: &mut DynamicTable,
    insert_gate: &std::cell::Cell<usize>,
) {
    match noprop::sample_weighted_index(ctx, &[2, 2, 1]) {
        0 => {
            let name = sample_name(ctx);
            let value = sample_value(ctx);
            table.insert(name, value).expect("should succeed");
            insert_gate.set(insert_gate.get() + 1);
        }
        1 => {
            let new_max = noprop::sample_usize_in(ctx, 0..=9999);
            table.set_max_size(new_max);
        }
        _ => {
            table.clear();
        }
    }
}

/// エントリサイズを計算する (RFC 7541 Section 4.1)
fn entry_size(name: &[u8], value: &[u8]) -> usize {
    name.len() + value.len() + 32
}

/// サイズ不変条件: 常に size() <= max_size()
///
/// 数学的意義: 表現不変条件
#[test]
fn prop_size_invariant() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 挿入が一度も発生しないと不変条件が空で成立するため、挿入成功をゲートする
    let insert_gate = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 0..=9999);
        let mut table = DynamicTable::new(max_size);
        let steps = noprop::sample_usize_in(ctx, 0..=49);

        for _ in 0..steps {
            sample_table_op(ctx, &mut table, &insert_gate);

            // 各操作後に不変条件を検査する
            assert!(
                table.size() <= table.max_size(),
                "size ({}) exceeds max_size ({})",
                table.size(),
                table.max_size()
            );
        }
        Ok(())
    })?;
    assert!(
        insert_gate.get() > 0,
        "挿入が一度も実行されなかった (不変条件が無検証で成立する)\n{runner}"
    );
    Ok(())
}

/// サイズ計算の整合性: size() = sum of entry sizes
///
/// 数学的意義: サイズ計算の整合性
#[test]
fn prop_size_equals_sum_of_entries() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 100..=9999);
        let count = noprop::sample_usize_in(ctx, 0..=19);
        let mut table = DynamicTable::new(max_size);

        for _ in 0..count {
            let name = sample_name(ctx);
            let value = sample_value(ctx);
            table.insert(name, value).expect("should succeed");
        }

        // 全エントリのサイズの合計を計算
        let mut sum = 0;
        for i in 0..table.len() {
            if let Some(entry) = table.get(i) {
                sum += entry.size();
            }
        }

        assert_eq!(table.size(), sum, "size() should equal sum of entry sizes");
        Ok(())
    })?;
    Ok(())
}

/// FIFO 順序: 新エントリは常に index 0 に追加
///
/// 数学的意義: FIFO キュー性質
#[test]
fn prop_fifo_order() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 1000..=9999);
        let count = noprop::sample_usize_in(ctx, 1..=9);
        let mut table = DynamicTable::new(max_size);

        for _ in 0..count {
            let name = sample_name(ctx);
            let value = sample_value(ctx);

            table
                .insert(name.clone(), value.clone())
                .expect("should succeed");

            // 最新のエントリは常に index 0
            let newest = table.get(0).expect("value should be present");
            assert_eq!(newest.name(), name);
            assert_eq!(newest.value(), value);
        }
        Ok(())
    })?;
    Ok(())
}

/// 最大サイズより大きいエントリはテーブルをクリアする (RFC 7541 Section 4.4)
///
/// 数学的意義: 境界条件
#[test]
fn prop_oversized_entry_clears_table() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 50..=499);
        let count = noprop::sample_usize_in(ctx, 1..=4);
        let mut table = DynamicTable::new(max_size);

        // 初期エントリを追加
        for _ in 0..count {
            let name = sample_name(ctx);
            let value = sample_value(ctx);
            if entry_size(&name, &value) <= max_size {
                table.insert(name, value).expect("should succeed");
            }
        }

        let had_entries = !table.is_empty();

        // max_size より大きいエントリを作成
        let large_name = vec![b'x'; max_size];
        let large_value = vec![b'y'; 1];
        assert!(entry_size(&large_name, &large_value) > max_size);

        table
            .insert(large_name, large_value)
            .expect("should succeed");

        // テーブルはクリアされる
        assert!(
            table.is_empty(),
            "Table should be empty after oversized insert"
        );
        assert_eq!(table.size(), 0);

        // had_entries が true だった場合、確かにクリアされた
        if had_entries {
            assert_eq!(table.len(), 0);
        }
        Ok(())
    })?;
    Ok(())
}

/// set_max_size は即座に不変条件を回復する (RFC 7541 Section 4.3)
///
/// 数学的意義: 不変条件の即時回復
#[test]
fn prop_set_max_size_immediate_eviction() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_max = noprop::sample_usize_in(ctx, 500..=4999);
        let count = noprop::sample_usize_in(ctx, 1..=9);
        let new_max = noprop::sample_usize_in(ctx, 0..=499);
        let mut table = DynamicTable::new(initial_max);

        // エントリを追加
        let mut inserted = false;
        for _ in 0..count {
            let name = sample_name(ctx);
            let value = sample_value(ctx);
            table.insert(name, value).expect("should succeed");
            inserted = true;
        }
        let _ = inserted;

        // max_size を減少
        table.set_max_size(new_max);

        // 即座に不変条件が満たされる
        assert!(
            table.size() <= new_max,
            "After set_max_size({}), size ({}) should be <= new_max",
            new_max,
            table.size()
        );
        assert_eq!(table.max_size(), new_max);
        Ok(())
    })?;
    Ok(())
}

/// 挿入したエントリは find で見つかる
///
/// 数学的意義: 挿入と検索の整合性
#[test]
fn prop_find_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 1000..=9999);
        let name = sample_name(ctx);
        let value = sample_value(ctx);
        // エントリサイズは最大 99 + 99 + 32 = 230 で max_size (>= 1000) に必ず収まる
        assert!(entry_size(&name, &value) <= max_size);

        let mut table = DynamicTable::new(max_size);
        table
            .insert(name.clone(), value.clone())
            .expect("construction should succeed");

        // 完全一致で見つかる
        let result = table.find(&name, &value);
        assert_eq!(result, Some((0, true)));
        Ok(())
    })?;
    Ok(())
}

/// find の部分一致 (名前のみ)
///
/// 数学的意義: 部分一致の検索
#[test]
fn prop_find_name_only_match() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 1000..=9999);
        let name = sample_name(ctx);
        let value1 = sample_value(ctx);
        // value2 は value1 と異なる値を引き直す (許容率ほぼ 1 で 8 回で十分)
        let value2 = noprop::sample_with_rejection(ctx, 8, |ctx| {
            let v = sample_value(ctx);
            (v != value1).then_some(v)
        });

        let mut table = DynamicTable::new(max_size);
        table
            .insert(name.clone(), value1)
            .expect("construction should succeed");

        // 名前のみ一致
        let result = table.find(&name, &value2);
        assert_eq!(result, Some((0, false)));
        Ok(())
    })?;
    Ok(())
}

/// find で見つからないエントリ
///
/// 数学的意義: 存在しないエントリの検索
#[test]
fn prop_find_not_found() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 100..=999);
        let name1 = sample_name(ctx);
        let value1 = sample_value(ctx);
        // name2 は name1 と異なる名前を引き直す (許容率ほぼ 1 で 8 回で十分)
        let name2 = noprop::sample_with_rejection(ctx, 8, |ctx| {
            let n = sample_name(ctx);
            (n != name1).then_some(n)
        });

        let mut table = DynamicTable::new(max_size);
        table
            .insert(name1, value1)
            .expect("construction should succeed");

        // 異なる名前は見つからない
        let result = table.find(&name2, b"any");
        assert_eq!(result, None);
        Ok(())
    })?;
    Ok(())
}

/// エントリサイズの公式 (RFC 7541 Section 4.1)
///
/// entry.size() = name.len() + value.len() + 32
/// 数学的意義: RFC 7541 Section 4.1 準拠
#[test]
fn prop_entry_size_formula() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let name = sample_name(ctx);
        let value = sample_value(ctx);
        let expected = name.len() + value.len() + 32;

        let mut table = DynamicTable::new(expected + 100);
        table
            .insert(name.clone(), value.clone())
            .expect("construction should succeed");

        let entry = table.get(0).expect("value should be present");
        assert_eq!(
            entry.size(),
            expected,
            "Entry size should be name.len() + value.len() + 32"
        );
        Ok(())
    })?;
    Ok(())
}

/// clear 後のテーブル状態
///
/// 数学的意義: クリア操作の効果
#[test]
fn prop_clear_resets_state() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 100..=9999);
        let count = noprop::sample_usize_in(ctx, 1..=19);
        let mut table = DynamicTable::new(max_size);

        // エントリを追加
        for _ in 0..count {
            let name = sample_name(ctx);
            let value = sample_value(ctx);
            table.insert(name, value).expect("should succeed");
        }

        // クリア
        table.clear();

        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert_eq!(table.size(), 0);
        // max_size は変わらない
        assert_eq!(table.max_size(), max_size);
        Ok(())
    })?;
    Ok(())
}

/// 古いエントリは eviction で削除される
///
/// 数学的意義: FIFO eviction ポリシー
#[test]
fn prop_eviction_removes_oldest() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 100..=499);
        let mut table = DynamicTable::new(max_size);

        // 小さいエントリを追加 (サイズ = 3 + 1 + 32 = 36)
        let small_entry_size = 36;
        let num_entries = max_size / small_entry_size;
        let num_entries = num_entries.max(2);

        for i in 0..num_entries {
            table
                .insert(format!("n{i:02}").into_bytes(), vec![b'v'])
                .expect("should succeed");
        }

        assert_eq!(table.len(), num_entries);

        // 追加のエントリを挿入 -> 最も古いエントリが削除される
        table
            .insert(b"new".as_slice(), vec![b'v'])
            .expect("should succeed");

        // 最新のエントリが index 0 にある
        assert_eq!(
            table.get(0).expect("value should be present").name(),
            b"new"
        );

        // 最も古いエントリ (n00) は削除されている
        let mut found_n00 = false;
        for i in 0..table.len() {
            if table.get(i).expect("value should be present").name() == b"n00" {
                found_n00 = true;
                break;
            }
        }
        assert!(!found_n00, "Oldest entry should be evicted");
        Ok(())
    })?;
    Ok(())
}

/// set_max_size(0) はテーブルをクリアする
///
/// 数学的意義: 境界ケース
#[test]
fn prop_set_max_size_zero_clears() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let initial_max = noprop::sample_usize_in(ctx, 100..=999);
        let count = noprop::sample_usize_in(ctx, 1..=4);
        let mut table = DynamicTable::new(initial_max);

        for _ in 0..count {
            let name = sample_name(ctx);
            let value = sample_value(ctx);
            if entry_size(&name, &value) <= initial_max {
                table.insert(name, value).expect("should succeed");
            }
        }

        table.set_max_size(0);

        assert!(table.is_empty());
        assert_eq!(table.size(), 0);
        assert_eq!(table.max_size(), 0);
        Ok(())
    })?;
    Ok(())
}

/// get_by_absolute_index の検証
///
/// 絶対インデックス 62 以降が動的テーブルを指す (RFC 7541 Section 2.3.3、静的テーブル長は 61)
#[test]
fn prop_get_by_absolute_index() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 1000..=9999);
        let count = noprop::sample_usize_in(ctx, 1..=4);
        let mut table = DynamicTable::new(max_size);
        let mut inserted: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

        for _ in 0..count {
            let name = sample_name(ctx);
            let value = sample_value(ctx);
            if entry_size(&name, &value) <= max_size {
                table
                    .insert(name.clone(), value.clone())
                    .expect("should succeed");
                inserted.push((name, value));
            }
        }

        // 静的テーブルのインデックス (1-61) は None を返す
        for i in 0..=61 {
            assert!(table.get_by_absolute_index(i).is_none());
        }

        // 動的テーブルのインデックス (62-) は対応するエントリを返す
        // 最新のエントリが 62、その次が 63、...
        for (i, (name, value)) in inserted.iter().rev().enumerate() {
            let abs_index = 62 + i;
            if let Some(entry) = table.get_by_absolute_index(abs_index) {
                assert_eq!(entry.name(), name);
                assert_eq!(entry.value(), value);
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// 連続した挿入でサイズが正しく更新される
///
/// 数学的意義: サイズ追跡の正確性
#[test]
fn prop_size_tracking_accuracy() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let max_size = noprop::sample_usize_in(ctx, 1000..=9999);
        let count = noprop::sample_usize_in(ctx, 0..=19);
        let mut table = DynamicTable::new(max_size);

        for _ in 0..count {
            let name = sample_name(ctx);
            let value = sample_value(ctx);
            let size_before = table.size();
            let entry_sz = entry_size(&name, &value);

            table.insert(name, value).expect("should succeed");

            // エントリが追加された場合、サイズは増加する
            // eviction が発生した場合、サイズは減少する可能性がある
            // いずれにせよ、size <= max_size
            assert!(table.size() <= max_size);

            // 空のテーブルに追加した場合
            if size_before == 0 && entry_sz <= max_size {
                assert_eq!(table.size(), entry_sz);
            }
        }
        Ok(())
    })?;
    Ok(())
}
