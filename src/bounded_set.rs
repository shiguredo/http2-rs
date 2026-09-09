//! 上限付き順序集合
//!
//! ストリーム ID のように単調増加する値の集合を、上限を超えたら
//! 最も小さい値から追い出して保持する。

use std::collections::BTreeSet;

/// 上限付き順序集合
///
/// 要素数が上限を超えた場合、最も小さい要素を削除する。ストリーム ID は
/// 開始主体・方向ごとの系統内では単調増加するため、実用上は最も古い ID が
/// 追い出される。系統をまたぐと数値順と生成順は一致しない点に注意する。
#[derive(Debug)]
pub(crate) struct BoundedSet<T: Ord> {
    inner: BTreeSet<T>,
    max_size: usize,
}

impl<T: Ord> BoundedSet<T> {
    /// 指定した上限で生成する
    pub(crate) fn new(max_size: usize) -> Self {
        Self {
            inner: BTreeSet::new(),
            max_size,
        }
    }

    /// 要素を追加し、上限を超えたら最も小さい要素を削除する
    pub(crate) fn insert(&mut self, value: T) {
        self.inner.insert(value);
        while self.inner.len() > self.max_size {
            // 要素数が上限を超えているため `pop_first` は必ず `Some` を返す
            let _ = self.inner.pop_first();
        }
    }

    /// 要素が含まれているかを返す
    pub(crate) fn contains(&self, value: &T) -> bool {
        self.inner.contains(value)
    }
}

#[cfg(test)]
mod tests {
    use super::BoundedSet;

    #[test]
    fn oldest_entry_evicted_on_overflow() {
        let mut set = BoundedSet::new(10);

        // 上限まで挿入する
        for i in 0..10 {
            set.insert(i);
        }
        for i in 0..10 {
            assert!(
                set.contains(&i),
                "上限以内のエントリ {i} は含まれていること"
            );
        }

        // 上限 + 1 のエントリを挿入すると、最も小さいエントリ (0) が削除される
        set.insert(10);
        assert!(
            !set.contains(&0),
            "上限超過により最も小さいエントリ 0 は削除されていること"
        );
        assert!(
            set.contains(&1),
            "2 番目に小さいエントリ 1 は残っていること"
        );
        assert!(set.contains(&10), "最新のエントリは含まれていること");
    }

    #[test]
    fn oldest_entries_evicted_continuously_past_limit() {
        let mut set = BoundedSet::new(10);

        // 上限まで挿入
        for i in 0..10 {
            set.insert(i);
        }

        // 上限を超えてさらに 10 件挿入する
        for i in 10..20 {
            set.insert(i);
        }

        // 古いエントリは全て削除されている
        for i in 0..10 {
            assert!(
                !set.contains(&i),
                "上限超過により古いエントリ {i} は削除されていること"
            );
        }

        // 後半のエントリは残っている
        assert!(set.contains(&10), "10 番目のエントリは残っていること");
        assert!(set.contains(&19), "19 番目のエントリは残っていること");
    }

    #[test]
    fn zero_capacity_keeps_set_empty() {
        let mut set = BoundedSet::new(0);
        set.insert(1);
        assert!(
            !set.contains(&1),
            "上限 0 では挿入した要素は保持されないこと"
        );
    }

    #[test]
    fn duplicate_insert_does_not_evict() {
        let mut set = BoundedSet::new(2);
        set.insert(1);
        set.insert(2);
        // 既存要素の再挿入では要素数が増えず、追い出しは起きない
        set.insert(1);
        assert!(set.contains(&1), "重複挿入後もエントリ 1 は残ること");
        assert!(set.contains(&2), "重複挿入後もエントリ 2 は残ること");
    }
}
