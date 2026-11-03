//! 上限付きの連想配列
//!
//! 追い出しは挿入順ではなくキーの昇順で行うため、キーがストリーム ID のように
//! 単調増加しない場合は、数値順と生成順が一致しない点に注意すること。

use std::collections::BTreeMap;

/// 要素数に上限を持つ連想配列
///
/// 上限を超えた場合、最も小さいキーを持つ要素を削除する (挿入順ではない)。
/// 削除された要素は再登録されるまで参照できなくなる。
#[derive(Debug)]
pub(crate) struct BoundedMap<K: Ord, V> {
    inner: BTreeMap<K, V>,
    max_size: usize,
}

impl<K: Ord, V> BoundedMap<K, V> {
    /// 指定した上限で生成する
    pub(crate) fn new(max_size: usize) -> Self {
        Self {
            inner: BTreeMap::new(),
            max_size,
        }
    }

    /// 要素を追加し、上限を超えたら最も小さいキーの要素を削除する
    pub(crate) fn insert(&mut self, key: K, value: V) {
        self.inner.insert(key, value);
        while self.inner.len() > self.max_size {
            // 要素数が上限を超えているため `pop_first` は必ず `Some` を返す
            let _ = self.inner.pop_first();
        }
    }

    /// キーに対応する値への参照を返す
    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.inner.get(key)
    }

    /// キーが含まれているかを返す
    pub(crate) fn contains(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::BoundedMap;

    #[test]
    fn smallest_key_evicted_on_overflow() {
        let mut map = BoundedMap::new(10);

        for i in 0..10 {
            map.insert(i, i * 2);
        }
        for i in 0..10 {
            assert_eq!(
                map.get(&i),
                Some(&(i * 2)),
                "上限以内のエントリ {i} は含まれていること"
            );
        }

        map.insert(10, 20);

        assert!(map.contains(&10), "新しいエントリが追加されること");
        assert!(
            !map.contains(&0),
            "最も小さいキーのエントリが削除されること"
        );
        assert_eq!(map.get(&5), Some(&10), "残りのエントリは保持されること");
    }

    #[test]
    fn descending_insert_still_evicts_smallest_key() {
        let mut map = BoundedMap::new(2);

        // 挿入順は 10, 5, 1 だが、追い出されるのはキーが最も小さい 1
        map.insert(10, 0);
        map.insert(5, 0);
        map.insert(1, 0);

        assert!(
            !map.contains(&1),
            "キーが最も小さいエントリが削除されること"
        );
        assert!(map.contains(&5), "キーが大きいエントリは保持されること");
        assert!(map.contains(&10), "キーが大きいエントリは保持されること");
    }

    #[test]
    fn zero_capacity_keeps_map_empty() {
        let mut map = BoundedMap::new(0);
        map.insert(1, 10);

        assert!(!map.contains(&1), "上限 0 では登録されないこと");
        assert_eq!(map.get(&1), None, "上限 0 では参照できないこと");
    }

    #[test]
    fn capacity_one_evicts_previous() {
        let mut map = BoundedMap::new(1);
        map.insert(1, 10);
        map.insert(2, 20);

        assert!(!map.contains(&1), "容量 1 では古いキーが削除されること");
        assert_eq!(map.get(&2), Some(&20), "最新のキーだけが残ること");
    }

    #[test]
    fn duplicate_insert_does_not_evict() {
        let mut map = BoundedMap::new(1);
        map.insert(1, 10);
        map.insert(1, 20);

        assert_eq!(
            map.get(&1),
            Some(&20),
            "同じキーは値を上書きし、追い出しは起きないこと"
        );
    }

    #[test]
    fn entries_evicted_continuously_past_limit() {
        let mut map = BoundedMap::new(2);
        map.insert(1, 10);
        map.insert(2, 20);

        for key in 3..=5 {
            map.insert(key, key * 10);
        }

        assert!(!map.contains(&1), "上限を超えて追い出されること");
        assert!(!map.contains(&2), "上限を超えて追い出されること");
        assert!(!map.contains(&3), "上限を超えて追い出されること");
        assert!(map.contains(&4), "直近 2 件のうちの 1 件が残ること");
        assert!(map.contains(&5), "直近 2 件のうちの 1 件が残ること");
    }
}
