//! HPACK 動的テーブル (RFC 7541 Section 2.3.2)
//!
//! HPACK で使用される動的テーブルを提供する。

use crate::hpack::table::{HeaderField, STATIC_TABLE_SIZE};

/// 動的テーブル
///
/// FIFO 順序でエントリを管理する。新しいエントリは先頭に追加され、
/// サイズ制限を超えると古いエントリが末尾から削除される。
#[derive(Debug, Clone)]
pub struct DynamicTable {
    /// エントリのリスト（先頭が最新）
    entries: Vec<HeaderField>,
    /// 現在のサイズ（バイト）
    size: usize,
    /// 最大サイズ（バイト）
    max_size: usize,
}

impl DynamicTable {
    /// 新しい `DynamicTable` を生成する
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::new(),
            size: 0,
            max_size,
        }
    }

    /// エントリ数を取得する
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// テーブルが空かどうかを返す
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 現在のサイズ（バイト）を取得する
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// 最大サイズ（バイト）を取得する
    #[must_use]
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// 最大サイズを設定する
    ///
    /// サイズが減少した場合、エントリが削除されることがある。
    pub fn set_max_size(&mut self, max_size: usize) {
        self.max_size = max_size;
        self.evict();
    }

    /// エントリを追加する (検査つき)
    ///
    /// `name` / `value` は [`HeaderField::new`] で検査され、不正な場合は
    /// エントリを追加せずに `Err` を返す。
    ///
    /// HPACK encoder / decoder 内部経路で wire 上のデータを直接挿入する場合は
    /// [`Self::insert_validated`] を使う。
    ///
    /// # Errors
    ///
    /// field-name / field-value の構文違反時は [`HeaderFieldError`] を返す。
    pub fn insert(
        &mut self,
        name: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
    ) -> Result<(), crate::hpack::HeaderFieldError> {
        let entry = HeaderField::new(name, value)?;
        self.insert_entry(entry);
        Ok(())
    }

    /// 検証済みバイト列からエントリを追加する (crate 内部限定)
    ///
    /// HPACK encoder / decoder 経路で wire 上のデータをそのまま挿入する場合に使う。
    /// 下流の利用者は [`Self::insert`] のみを使う。
    pub(crate) fn insert_validated(&mut self, name: Vec<u8>, value: Vec<u8>) {
        let entry = HeaderField::from_validated_parts(name, value, false);
        self.insert_entry(entry);
    }

    fn insert_entry(&mut self, entry: HeaderField) {
        let entry_size = entry.size();

        // エントリが最大サイズより大きい場合、テーブルをクリアする
        if entry_size > self.max_size {
            self.clear();
            return;
        }

        // サイズ制限を超えないように古いエントリを削除
        while self.size + entry_size > self.max_size {
            if let Some(removed) = self.entries.pop() {
                self.size -= removed.size();
            } else {
                break;
            }
        }

        // エントリを先頭に追加
        self.entries.insert(0, entry);
        self.size += entry_size;
    }

    /// インデックスからエントリを取得する
    ///
    /// インデックスは動的テーブル内でのインデックス（0 から始まる）。
    /// 静的テーブルを含む絶対インデックスから変換するには、
    /// `absolute_index - STATIC_TABLE_SIZE - 1` を使用する。
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&HeaderField> {
        self.entries.get(index)
    }

    /// 絶対インデックス（静的テーブル + 動的テーブル）からエントリを取得する
    ///
    /// 絶対インデックスは 1 から始まり、1-61 が静的テーブル、
    /// 62 以降が動的テーブルを指す。
    #[must_use]
    pub fn get_by_absolute_index(&self, index: usize) -> Option<&HeaderField> {
        if index <= STATIC_TABLE_SIZE {
            None
        } else {
            self.get(index - STATIC_TABLE_SIZE - 1)
        }
    }

    /// ヘッダーフィールドのインデックスを検索する
    ///
    /// 完全一致するエントリがある場合は `(index, true)` を返す。
    /// 名前のみ一致するエントリがある場合は `(index, false)` を返す。
    /// インデックスは動的テーブル内でのインデックス（0 から始まる）。
    #[must_use]
    pub fn find(&self, name: &[u8], value: &[u8]) -> Option<(usize, bool)> {
        let mut name_match = None;

        for (i, entry) in self.entries.iter().enumerate() {
            if entry.name() == name {
                if entry.value() == value {
                    return Some((i, true));
                }
                if name_match.is_none() {
                    name_match = Some(i);
                }
            }
        }

        name_match.map(|i| (i, false))
    }

    /// テーブルをクリアする
    pub fn clear(&mut self) {
        self.entries.clear();
        self.size = 0;
    }

    /// サイズ制限を超えているエントリを削除する
    fn evict(&mut self) {
        while self.size > self.max_size {
            if let Some(removed) = self.entries.pop() {
                self.size -= removed.size();
            } else {
                break;
            }
        }
    }
}

impl Default for DynamicTable {
    fn default() -> Self {
        Self::new(crate::settings::DEFAULT_HEADER_TABLE_SIZE as usize)
    }
}
