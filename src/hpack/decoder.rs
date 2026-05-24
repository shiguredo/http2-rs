//! HPACK デコーダー (RFC 7541)

use crate::error::{Error, Result};
use crate::hpack::dynamic_table::DynamicTable;
use crate::hpack::huffman;
use crate::hpack::integer;
use crate::hpack::table::{HeaderField, STATIC_TABLE_SIZE, get_static_entry};

/// HPACK デコーダー
#[derive(Debug)]
pub struct Decoder {
    /// 動的テーブル
    dynamic_table: DynamicTable,
    /// 最大許容テーブルサイズ
    max_table_size: usize,
}

impl Decoder {
    /// 新しい `Decoder` を生成する
    #[must_use]
    pub fn new(max_table_size: usize) -> Self {
        Self {
            dynamic_table: DynamicTable::new(max_table_size),
            max_table_size,
        }
    }

    /// 動的テーブルの最大サイズを設定する
    pub fn set_max_table_size(&mut self, max_size: usize) {
        self.max_table_size = max_size;
        self.dynamic_table.set_max_size(max_size);
    }

    /// 動的テーブルへの参照を取得する
    #[must_use]
    pub fn dynamic_table(&self) -> &DynamicTable {
        &self.dynamic_table
    }

    /// ヘッダーブロックをデコードする
    ///
    /// # Errors
    ///
    /// 不正な HPACK データの場合は `Err` を返す。
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<HeaderField>> {
        let mut headers = Vec::new();
        let mut offset = 0;
        // RFC 7541 Section 4.2: Dynamic Table Size Update はヘッダーブロックの先頭でのみ許可
        let mut seen_header = false;

        while offset < data.len() {
            let first_byte = data[offset];

            if first_byte & 0x80 != 0 {
                // Indexed Header Field (Section 6.1)
                let (header, consumed) = self.decode_indexed(&data[offset..])?;
                headers.push(header);
                offset += consumed;
                seen_header = true;
            } else if first_byte & 0x40 != 0 {
                // Literal Header Field with Incremental Indexing (Section 6.2.1)
                let (header, consumed) = self.decode_literal_indexed(&data[offset..])?;
                self.dynamic_table
                    .insert_validated(header.name().to_vec(), header.value().to_vec());
                headers.push(header);
                offset += consumed;
                seen_header = true;
            } else if first_byte & 0x20 != 0 {
                // Dynamic Table Size Update (Section 6.3)
                // RFC 7541 Section 4.2: ヘッダーが出現した後は許可しない
                if seen_header {
                    return Err(Error::hpack_error(
                        "dynamic table size update must be at the beginning of header block",
                    ));
                }
                let consumed = self.decode_size_update(&data[offset..])?;
                offset += consumed;
            } else if first_byte & 0x10 != 0 {
                // Literal Header Field Never Indexed (Section 6.2.3)
                let (header, consumed) = self.decode_literal_never_indexed(&data[offset..])?;
                headers.push(header);
                offset += consumed;
                seen_header = true;
            } else {
                // Literal Header Field without Indexing (Section 6.2.2)
                let (header, consumed) = self.decode_literal(&data[offset..])?;
                headers.push(header);
                offset += consumed;
                seen_header = true;
            }
        }

        Ok(headers)
    }

    /// Indexed Header Field をデコードする (Section 6.1)
    fn decode_indexed(&self, data: &[u8]) -> Result<(HeaderField, usize)> {
        let (index, consumed) = integer::decode(data, 7)?;

        if index == 0 {
            return Err(Error::hpack_error("invalid index 0"));
        }

        let header = self.get_header_by_index(index as usize)?;
        Ok((header, consumed))
    }

    /// Literal Header Field with Incremental Indexing をデコードする (Section 6.2.1)
    fn decode_literal_indexed(&mut self, data: &[u8]) -> Result<(HeaderField, usize)> {
        let (name_index, mut consumed) = integer::decode(data, 6)?;

        let name = if name_index == 0 {
            // 新しい名前
            let (n, c) = self.decode_string(&data[consumed..])?;
            consumed += c;
            n
        } else {
            // 既存の名前を参照
            let header = self.get_header_by_index(name_index as usize)?;
            header.name().to_vec()
        };

        let (value, c) = self.decode_string(&data[consumed..])?;
        consumed += c;

        Ok((
            HeaderField::from_validated_parts(name, value, false),
            consumed,
        ))
    }

    /// Literal Header Field without Indexing をデコードする (Section 6.2.2)
    fn decode_literal(&self, data: &[u8]) -> Result<(HeaderField, usize)> {
        let (name_index, mut consumed) = integer::decode(data, 4)?;

        let name = if name_index == 0 {
            // 新しい名前
            let (n, c) = self.decode_string(&data[consumed..])?;
            consumed += c;
            n
        } else {
            // 既存の名前を参照
            let header = self.get_header_by_index(name_index as usize)?;
            header.name().to_vec()
        };

        let (value, c) = self.decode_string(&data[consumed..])?;
        consumed += c;

        Ok((
            HeaderField::from_validated_parts(name, value, false),
            consumed,
        ))
    }

    /// Literal Header Field Never Indexed をデコードする (Section 6.2.3)
    fn decode_literal_never_indexed(&self, data: &[u8]) -> Result<(HeaderField, usize)> {
        let (name_index, mut consumed) = integer::decode(data, 4)?;

        let name = if name_index == 0 {
            // 新しい名前
            let (n, c) = self.decode_string(&data[consumed..])?;
            consumed += c;
            n
        } else {
            // 既存の名前を参照
            let header = self.get_header_by_index(name_index as usize)?;
            header.name().to_vec()
        };

        let (value, c) = self.decode_string(&data[consumed..])?;
        consumed += c;

        Ok((
            HeaderField::from_validated_parts(name, value, true),
            consumed,
        ))
    }

    /// Dynamic Table Size Update をデコードする (Section 6.3)
    fn decode_size_update(&mut self, data: &[u8]) -> Result<usize> {
        let (new_size, consumed) = integer::decode(data, 5)?;

        if new_size as usize > self.max_table_size {
            return Err(Error::hpack_error(
                "dynamic table size update exceeds maximum",
            ));
        }

        self.dynamic_table.set_max_size(new_size as usize);
        Ok(consumed)
    }

    /// 文字列をデコードする
    fn decode_string(&self, data: &[u8]) -> Result<(Vec<u8>, usize)> {
        if data.is_empty() {
            return Err(Error::hpack_error("incomplete HPACK string"));
        }

        let huffman_encoded = data[0] & 0x80 != 0;
        let (length, mut consumed) = integer::decode(data, 7)?;

        let length = length as usize;
        if data.len() < consumed + length {
            return Err(Error::hpack_error("incomplete HPACK string"));
        }

        let string_data = &data[consumed..consumed + length];
        consumed += length;

        let result = if huffman_encoded {
            huffman::decode(string_data)?
        } else {
            string_data.to_vec()
        };

        Ok((result, consumed))
    }

    /// インデックスからヘッダーフィールドを取得する
    fn get_header_by_index(&self, index: usize) -> Result<HeaderField> {
        if index <= STATIC_TABLE_SIZE {
            // 静的テーブル
            get_static_entry(index)
                .map(|e| e.to_header_field())
                .ok_or_else(|| Error::hpack_error("invalid static table index"))
        } else {
            // 動的テーブル
            self.dynamic_table
                .get_by_absolute_index(index)
                .cloned()
                .ok_or_else(|| Error::hpack_error("invalid dynamic table index"))
        }
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new(crate::settings::DEFAULT_HEADER_TABLE_SIZE as usize)
    }
}
