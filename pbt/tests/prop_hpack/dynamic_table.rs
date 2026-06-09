//! HPACK 動的テーブルの PBT (RFC 7541 Section 2.3.2)
//!
//! HPACK 動的テーブルの不変条件を検証する。

use proptest::prelude::*;
use shiguredo_http2::hpack::DynamicTable;

/// テーブル操作
#[derive(Debug, Clone)]
enum TableOp {
    Insert { name: Vec<u8>, value: Vec<u8> },
    SetMaxSize(usize),
    Clear,
}

/// HPACK 動的テーブルに挿入可能な field-name を生成する
/// (token-lowercase + 数字 + '-' + '_', 非空)
fn valid_name() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        prop::sample::select(
            (b'a'..=b'z')
                .chain(b'0'..=b'9')
                .chain([b'-', b'_'])
                .collect::<Vec<_>>(),
        ),
        1..100,
    )
}

/// HPACK 動的テーブルに挿入可能な field-value を生成する
///
/// RFC 9113 §8.2.1: visible ASCII + 内部 SP/HTAB 許容、両端 SP/HTAB は除去、
/// NUL/CR/LF は構築時検査で禁止。
fn valid_value() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0x20u8..=0x7e, 0..100).prop_map(|v| {
        v.iter()
            .position(|&b| b != 0x20 && b != 0x09)
            .map(|start| {
                let end = v.iter().rposition(|&b| b != 0x20 && b != 0x09).unwrap();
                v[start..=end].to_vec()
            })
            .unwrap_or_default()
    })
}

/// テーブル操作の Strategy
fn table_op() -> impl Strategy<Value = TableOp> {
    prop_oneof![
        (valid_name(), valid_value()).prop_map(|(name, value)| TableOp::Insert { name, value }),
        (0..10000usize).prop_map(TableOp::SetMaxSize),
        Just(TableOp::Clear),
    ]
}

/// エントリサイズを計算する (RFC 7541 Section 4.1)
fn entry_size(name: &[u8], value: &[u8]) -> usize {
    name.len() + value.len() + 32
}

proptest! {
    /// サイズ不変条件: 常に size() <= max_size()
    ///
    /// 数学的意義: 表現不変条件
    #[test]
    fn prop_size_invariant(
        max_size in 0..10000usize,
        ops in prop::collection::vec(table_op(), 0..50),
    ) {
        let mut table = DynamicTable::new(max_size);

        for op in ops {
            match op {
                TableOp::Insert { name, value } => {
                    table.insert(name, value).unwrap();
                }
                TableOp::SetMaxSize(new_max) => {
                    table.set_max_size(new_max);
                }
                TableOp::Clear => {
                    table.clear();
                }
            }

            // 不変条件: size <= max_size
            prop_assert!(
                table.size() <= table.max_size(),
                "size ({}) exceeds max_size ({})",
                table.size(),
                table.max_size()
            );
        }
    }

    /// サイズ計算の整合性: size() = sum of entry sizes
    ///
    /// 数学的意義: サイズ計算の整合性
    #[test]
    fn prop_size_equals_sum_of_entries(
        max_size in 100..10000usize,
        entries in prop::collection::vec((valid_name(), valid_value()), 0..20),
    ) {
        let mut table = DynamicTable::new(max_size);

        for (name, value) in entries {
            table.insert(name, value).unwrap();
        }

        // 全エントリのサイズの合計を計算
        let mut sum = 0;
        for i in 0..table.len() {
            if let Some(entry) = table.get(i) {
                sum += entry.size();
            }
        }

        prop_assert_eq!(
            table.size(),
            sum,
            "size() should equal sum of entry sizes"
        );
    }

    /// FIFO 順序: 新エントリは常に index 0 に追加
    ///
    /// 数学的意義: FIFO キュー性質
    #[test]
    fn prop_fifo_order(
        max_size in 1000..10000usize,
        entries in prop::collection::vec((valid_name(), valid_value()), 1..10),
    ) {
        let mut table = DynamicTable::new(max_size);

        for (name, value) in &entries {
            table.insert(name.clone(), value.clone()).unwrap();

            // 最新のエントリは常に index 0
            let newest = table.get(0).unwrap();
            prop_assert_eq!(newest.name(), name);
            prop_assert_eq!(newest.value(), value);
        }
    }

    /// 最大サイズより大きいエントリはテーブルをクリアする (RFC 7541 Section 4.4)
    ///
    /// 数学的意義: 境界条件
    #[test]
    fn prop_oversized_entry_clears_table(
        max_size in 50..500usize,
        initial_entries in prop::collection::vec((valid_name(), valid_value()), 1..5),
    ) {
        let mut table = DynamicTable::new(max_size);

        // 初期エントリを追加
        for (name, value) in initial_entries {
            if entry_size(&name, &value) <= max_size {
                table.insert(name, value).unwrap();
            }
        }

        let had_entries = !table.is_empty();

        // max_size より大きいエントリを作成
        let large_name = vec![b'x'; max_size];
        let large_value = vec![b'y'; 1];
        prop_assert!(entry_size(&large_name, &large_value) > max_size);

        table.insert(large_name, large_value).unwrap();

        // テーブルはクリアされる
        prop_assert!(table.is_empty(), "Table should be empty after oversized insert");
        prop_assert_eq!(table.size(), 0);

        // had_entries が true だった場合、確かにクリアされた
        if had_entries {
            prop_assert_eq!(table.len(), 0);
        }
    }

    /// set_max_size は即座に不変条件を回復する (RFC 7541 Section 4.3)
    ///
    /// 数学的意義: 不変条件の即時回復
    #[test]
    fn prop_set_max_size_immediate_eviction(
        initial_max in 500..5000usize,
        entries in prop::collection::vec((valid_name(), valid_value()), 1..10),
        new_max in 0..500usize,
    ) {
        let mut table = DynamicTable::new(initial_max);

        // エントリを追加
        for (name, value) in entries {
            table.insert(name, value).unwrap();
        }

        // max_size を減少
        table.set_max_size(new_max);

        // 即座に不変条件が満たされる
        prop_assert!(
            table.size() <= new_max,
            "After set_max_size({}), size ({}) should be <= new_max",
            new_max,
            table.size()
        );
        prop_assert_eq!(table.max_size(), new_max);
    }

    /// 挿入したエントリは find で見つかる
    ///
    /// 数学的意義: 挿入と検索の整合性
    #[test]
    fn prop_find_consistency(
        max_size in 1000..10000usize,
        name in valid_name(),
        value in valid_value(),
    ) {
        // エントリが max_size に収まる場合のみテスト
        let size = entry_size(&name, &value);
        prop_assume!(size <= max_size);

        let mut table = DynamicTable::new(max_size);
        table.insert(name.clone(), value.clone()).unwrap();

        // 完全一致で見つかる
        let result = table.find(&name, &value);
        prop_assert_eq!(result, Some((0, true)));
    }

    /// find の部分一致 (名前のみ)
    ///
    /// 数学的意義: 部分一致の検索
    #[test]
    fn prop_find_name_only_match(
        max_size in 1000..10000usize,
        name in valid_name(),
        value1 in valid_value(),
        value2 in valid_value(),
    ) {
        prop_assume!(value1 != value2);
        prop_assume!(entry_size(&name, &value1) <= max_size);

        let mut table = DynamicTable::new(max_size);
        table.insert(name.clone(), value1).unwrap();

        // 名前のみ一致
        let result = table.find(&name, &value2);
        prop_assert_eq!(result, Some((0, false)));
    }

    /// find で見つからないエントリ
    ///
    /// 数学的意義: 存在しないエントリの検索
    #[test]
    fn prop_find_not_found(
        max_size in 100..1000usize,
        name1 in valid_name(),
        value1 in valid_value(),
        name2 in valid_name(),
    ) {
        prop_assume!(name1 != name2);
        prop_assume!(entry_size(&name1, &value1) <= max_size);

        let mut table = DynamicTable::new(max_size);
        table.insert(name1, value1).unwrap();

        // 異なる名前は見つからない
        let result = table.find(&name2, b"any");
        prop_assert_eq!(result, None);
    }

    /// エントリサイズの公式 (RFC 7541 Section 4.1)
    ///
    /// entry.size() = name.len() + value.len() + 32
    /// 数学的意義: RFC 7541 Section 4.1 準拠
    #[test]
    fn prop_entry_size_formula(
        name in valid_name(),
        value in valid_value(),
    ) {
        let expected = name.len() + value.len() + 32;

        let mut table = DynamicTable::new(expected + 100);
        table.insert(name.clone(), value.clone()).unwrap();

        let entry = table.get(0).unwrap();
        prop_assert_eq!(
            entry.size(),
            expected,
            "Entry size should be name.len() + value.len() + 32"
        );
    }

    /// clear 後のテーブル状態
    ///
    /// 数学的意義: クリア操作の効果
    #[test]
    fn prop_clear_resets_state(
        max_size in 100..10000usize,
        entries in prop::collection::vec((valid_name(), valid_value()), 1..20),
    ) {
        let mut table = DynamicTable::new(max_size);

        // エントリを追加
        for (name, value) in entries {
            table.insert(name, value).unwrap();
        }

        // クリア
        table.clear();

        prop_assert!(table.is_empty());
        prop_assert_eq!(table.len(), 0);
        prop_assert_eq!(table.size(), 0);
        // max_size は変わらない
        prop_assert_eq!(table.max_size(), max_size);
    }

    /// 古いエントリは eviction で削除される
    ///
    /// 数学的意義: FIFO eviction ポリシー
    #[test]
    fn prop_eviction_removes_oldest(
        max_size in 100..500usize,
    ) {
        let mut table = DynamicTable::new(max_size);

        // 小さいエントリを追加 (サイズ = 3 + 1 + 32 = 36)
        let small_entry_size = 36;
        let num_entries = max_size / small_entry_size;

        for i in 0..num_entries {
            table.insert(format!("n{i:02}").into_bytes(), vec![b'v']).unwrap();
        }

        prop_assert_eq!(table.len(), num_entries);

        // 追加のエントリを挿入 -> 最も古いエントリが削除される
        table.insert(b"new", vec![b'v']).unwrap();

        // 最新のエントリが index 0 にある
        prop_assert_eq!(table.get(0).unwrap().name(), b"new");

        // 最も古いエントリ (n00) は削除されている
        let mut found_n00 = false;
        for i in 0..table.len() {
            if table.get(i).unwrap().name() == b"n00" {
                found_n00 = true;
                break;
            }
        }
        prop_assert!(!found_n00, "Oldest entry should be evicted");
    }

    /// set_max_size(0) はテーブルをクリアする
    ///
    /// 数学的意義: 境界ケース
    #[test]
    fn prop_set_max_size_zero_clears(
        initial_max in 100..1000usize,
        entries in prop::collection::vec((valid_name(), valid_value()), 1..5),
    ) {
        let mut table = DynamicTable::new(initial_max);

        for (name, value) in entries {
            if entry_size(&name, &value) <= initial_max {
                table.insert(name, value).unwrap();
            }
        }

        table.set_max_size(0);

        prop_assert!(table.is_empty());
        prop_assert_eq!(table.size(), 0);
        prop_assert_eq!(table.max_size(), 0);
    }

    /// get_by_absolute_index の検証
    ///
    /// 絶対インデックス 62 以降が動的テーブルを指す (RFC 7541 Section 2.3.3、静的テーブル長は 61)
    #[test]
    fn prop_get_by_absolute_index(
        max_size in 1000..10000usize,
        entries in prop::collection::vec((valid_name(), valid_value()), 1..5),
    ) {
        let mut table = DynamicTable::new(max_size);
        let mut inserted: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

        for (name, value) in entries {
            if entry_size(&name, &value) <= max_size {
                table.insert(name.clone(), value.clone()).unwrap();
                inserted.push((name, value));
            }
        }

        // 静的テーブルのインデックス (1-61) は None を返す
        for i in 0..=61 {
            prop_assert!(table.get_by_absolute_index(i).is_none());
        }

        // 動的テーブルのインデックス (62-) は対応するエントリを返す
        // 最新のエントリが 62、その次が 63、...
        for (i, (name, value)) in inserted.iter().rev().enumerate() {
            let abs_index = 62 + i;
            if let Some(entry) = table.get_by_absolute_index(abs_index) {
                prop_assert_eq!(entry.name(), name);
                prop_assert_eq!(entry.value(), value);
            }
        }
    }

    /// 連続した挿入でサイズが正しく更新される
    ///
    /// 数学的意義: サイズ追跡の正確性
    #[test]
    fn prop_size_tracking_accuracy(
        max_size in 1000..10000usize,
        entries in prop::collection::vec((valid_name(), valid_value()), 0..20),
    ) {
        let mut table = DynamicTable::new(max_size);

        for (name, value) in entries {
            let size_before = table.size();
            let entry_sz = entry_size(&name, &value);

            table.insert(name, value).unwrap();

            // エントリが追加された場合、サイズは増加する
            // eviction が発生した場合、サイズは減少する可能性がある
            // いずれにせよ、size <= max_size
            prop_assert!(table.size() <= max_size);

            // 空のテーブルに追加した場合
            if size_before == 0 && entry_sz <= max_size {
                prop_assert_eq!(table.size(), entry_sz);
            }
        }
    }
}
