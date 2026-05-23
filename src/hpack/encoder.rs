//! HPACK エンコーダー (RFC 7541)

use crate::hpack::dynamic_table::DynamicTable;
use crate::hpack::huffman;
use crate::hpack::integer;
use crate::hpack::table::{HeaderField, STATIC_TABLE_SIZE, find_static_index};

/// HPACK エンコーダー
#[derive(Debug)]
pub struct Encoder {
    /// 動的テーブル
    dynamic_table: DynamicTable,
    /// Huffman 符号化を使用するかどうか
    use_huffman: bool,
}

impl Encoder {
    /// 新しい `Encoder` を生成する
    #[must_use]
    pub fn new(max_table_size: usize) -> Self {
        Self {
            dynamic_table: DynamicTable::new(max_table_size),
            use_huffman: true,
        }
    }

    /// Huffman 符号化の使用を設定する
    pub fn set_huffman(&mut self, use_huffman: bool) {
        self.use_huffman = use_huffman;
    }

    /// 動的テーブルの最大サイズを設定する
    pub fn set_max_table_size(&mut self, max_size: usize) {
        self.dynamic_table.set_max_size(max_size);
    }

    /// 動的テーブルへの参照を取得する
    #[must_use]
    pub fn dynamic_table(&self) -> &DynamicTable {
        &self.dynamic_table
    }

    /// ヘッダーリストをエンコードする
    ///
    /// 機密ヘッダー（`sensitive` フラグが true）は Never Indexed としてエンコードされる。
    pub fn encode(&mut self, buf: &mut Vec<u8>, headers: &[HeaderField]) {
        for header in headers {
            if header.sensitive() {
                // Never Indexed (Section 6.2.3)
                self.encode_header_sensitive(buf, header.name(), header.value());
            } else {
                self.encode_header(buf, header.name(), header.value(), true);
            }
        }
    }

    /// 単一のヘッダーフィールドをエンコードする
    ///
    /// `indexing` が `true` の場合、エントリを動的テーブルに追加する。
    pub fn encode_header(&mut self, buf: &mut Vec<u8>, name: &[u8], value: &[u8], indexing: bool) {
        // 静的テーブルと動的テーブルからインデックスを検索
        if let Some((index, exact)) = self.find_index(name, value) {
            if exact {
                // Indexed Header Field (Section 6.1)
                self.encode_indexed(buf, index);
                return;
            }

            if indexing {
                // Literal Header Field with Incremental Indexing (Section 6.2.1)
                self.encode_literal_indexed(buf, index, value);
                self.dynamic_table
                    .insert_validated(name.to_vec(), value.to_vec());
            } else {
                // Literal Header Field without Indexing (Section 6.2.2)
                self.encode_literal_without_indexing_indexed(buf, index, value);
            }
        } else if indexing {
            // Literal Header Field with Incremental Indexing (Section 6.2.1)
            self.encode_literal_new(buf, name, value);
            self.dynamic_table
                .insert_validated(name.to_vec(), value.to_vec());
        } else {
            // Literal Header Field without Indexing (Section 6.2.2)
            self.encode_literal_without_indexing_new(buf, name, value);
        }
    }

    /// 機密ヘッダーフィールドをエンコードする（Never Indexed）
    ///
    /// RFC 7541 Section 6.2.3: Literal Header Field Never Indexed
    /// このエンコードは中間者がこのヘッダーを動的テーブルにインデックスすることを禁止する。
    pub fn encode_header_sensitive(&self, buf: &mut Vec<u8>, name: &[u8], value: &[u8]) {
        if let Some((index, _exact)) = self.find_index(name, value) {
            // Never Indexed with existing name (0x1x prefix)
            self.encode_never_indexed_with_name_index(buf, index, value);
        } else {
            // Never Indexed with new name (0x10 prefix)
            self.encode_never_indexed_new(buf, name, value);
        }
    }

    /// インデックスを検索する
    ///
    /// 静的テーブルと動的テーブルの両方から検索する。
    fn find_index(&self, name: &[u8], value: &[u8]) -> Option<(usize, bool)> {
        // 静的テーブルを検索
        if let Some((index, exact)) = find_static_index(name, value) {
            if exact {
                return Some((index, true));
            }

            // 動的テーブルも検索して完全一致があればそちらを優先
            if let Some((dyn_index, true)) = self.dynamic_table.find(name, value) {
                return Some((STATIC_TABLE_SIZE + 1 + dyn_index, true));
            }

            return Some((index, false));
        }

        // 動的テーブルを検索
        if let Some((dyn_index, exact)) = self.dynamic_table.find(name, value) {
            return Some((STATIC_TABLE_SIZE + 1 + dyn_index, exact));
        }

        None
    }

    /// Indexed Header Field をエンコードする (Section 6.1)
    fn encode_indexed(&self, buf: &mut Vec<u8>, index: usize) {
        let mut temp = [0u8; 16];
        let len = integer::encode(&mut temp, index as u64, 7, 0x80)
            .expect("16-byte buffer is sufficient for HPACK integer");
        buf.extend_from_slice(&temp[..len]);
    }

    /// Literal Header Field with Incremental Indexing (既存の名前) をエンコードする
    fn encode_literal_indexed(&self, buf: &mut Vec<u8>, name_index: usize, value: &[u8]) {
        // 6-bit prefix, pattern 01xxxxxx
        let mut temp = [0u8; 16];
        let len = integer::encode(&mut temp, name_index as u64, 6, 0x40)
            .expect("16-byte buffer is sufficient for HPACK integer");
        buf.extend_from_slice(&temp[..len]);

        self.encode_string(buf, value);
    }

    /// Literal Header Field with Incremental Indexing (新しい名前) をエンコードする
    fn encode_literal_new(&self, buf: &mut Vec<u8>, name: &[u8], value: &[u8]) {
        // 0x40 = pattern 01000000
        buf.push(0x40);
        self.encode_string(buf, name);
        self.encode_string(buf, value);
    }

    /// Literal Header Field without Indexing (既存の名前) をエンコードする
    fn encode_literal_without_indexing_indexed(
        &self,
        buf: &mut Vec<u8>,
        name_index: usize,
        value: &[u8],
    ) {
        // 4-bit prefix, pattern 0000xxxx
        let mut temp = [0u8; 16];
        let len = integer::encode(&mut temp, name_index as u64, 4, 0x00)
            .expect("16-byte buffer is sufficient for HPACK integer");
        buf.extend_from_slice(&temp[..len]);

        self.encode_string(buf, value);
    }

    /// Literal Header Field without Indexing (新しい名前) をエンコードする
    fn encode_literal_without_indexing_new(&self, buf: &mut Vec<u8>, name: &[u8], value: &[u8]) {
        // 0x00 = pattern 00000000
        buf.push(0x00);
        self.encode_string(buf, name);
        self.encode_string(buf, value);
    }

    /// Literal Header Field Never Indexed (既存の名前) をエンコードする (Section 6.2.3)
    fn encode_never_indexed_with_name_index(
        &self,
        buf: &mut Vec<u8>,
        name_index: usize,
        value: &[u8],
    ) {
        // 4-bit prefix, pattern 0001xxxx
        let mut temp = [0u8; 16];
        let len = integer::encode(&mut temp, name_index as u64, 4, 0x10)
            .expect("16-byte buffer is sufficient for HPACK integer");
        buf.extend_from_slice(&temp[..len]);

        self.encode_string(buf, value);
    }

    /// Literal Header Field Never Indexed (新しい名前) をエンコードする (Section 6.2.3)
    fn encode_never_indexed_new(&self, buf: &mut Vec<u8>, name: &[u8], value: &[u8]) {
        // 0x10 = pattern 00010000
        buf.push(0x10);
        self.encode_string(buf, name);
        self.encode_string(buf, value);
    }

    /// 文字列をエンコードする
    fn encode_string(&self, buf: &mut Vec<u8>, data: &[u8]) {
        if self.use_huffman {
            let encoded = huffman::encode_to_vec(data);
            if encoded.len() < data.len() {
                // Huffman 符号化の方が短い場合
                let mut temp = [0u8; 16];
                let len = integer::encode(&mut temp, encoded.len() as u64, 7, 0x80)
                    .expect("16-byte buffer is sufficient for HPACK integer");
                buf.extend_from_slice(&temp[..len]);
                buf.extend_from_slice(&encoded);
                return;
            }
        }

        // 平文でエンコード
        let mut temp = [0u8; 16];
        let len = integer::encode(&mut temp, data.len() as u64, 7, 0x00)
            .expect("16-byte buffer is sufficient for HPACK integer");
        buf.extend_from_slice(&temp[..len]);
        buf.extend_from_slice(data);
    }

    /// Dynamic Table Size Update をエンコードする (Section 6.3)
    pub fn encode_size_update(&self, buf: &mut Vec<u8>, new_size: usize) {
        let mut temp = [0u8; 16];
        let len = integer::encode(&mut temp, new_size as u64, 5, 0x20)
            .expect("16-byte buffer is sufficient for HPACK integer");
        buf.extend_from_slice(&temp[..len]);
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new(crate::settings::DEFAULT_HEADER_TABLE_SIZE as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_indexed() {
        let encoder = Encoder::new(4096);
        let mut buf = Vec::new();

        // :method: GET (index 2)
        encoder.encode_indexed(&mut buf, 2);
        assert_eq!(buf, vec![0x82]);
    }

    #[test]
    fn test_encode_literal_indexed() {
        let encoder = Encoder::new(4096);
        let mut buf = Vec::new();

        // Using index 1 (:authority) with value "www.example.com"
        encoder.encode_literal_indexed(&mut buf, 1, b"www.example.com");

        // 0x41 = 01000001 (incremental indexing, index 1)
        assert_eq!(buf[0], 0x41);
    }

    #[test]
    fn test_encode_header_list() {
        let mut encoder = Encoder::new(4096);
        let mut buf = Vec::new();

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new(":path", "/").unwrap(),
        ];

        encoder.encode(&mut buf, &headers);

        // :method: GET should be indexed (0x82)
        // :path: / should be indexed (0x84)
        assert!(buf.contains(&0x82));
        assert!(buf.contains(&0x84));
    }

    #[test]
    fn test_encode_never_indexed() {
        let encoder = Encoder::new(4096);
        let mut buf = Vec::new();

        // Never Indexed with new name
        encoder.encode_never_indexed_new(&mut buf, b"authorization", b"secret");

        // First byte should be 0x10 (pattern 00010000)
        assert_eq!(buf[0], 0x10);
    }

    #[test]
    fn test_encode_never_indexed_with_index() {
        let encoder = Encoder::new(4096);
        let mut buf = Vec::new();

        // Never Indexed with name index 23 (authorization in static table)
        encoder.encode_never_indexed_with_name_index(&mut buf, 23, b"secret");

        // First byte should be 0x17 (pattern 0001 + 0111 = index 23)
        assert_eq!(buf[0] & 0xF0, 0x10);
    }

    #[test]
    fn test_encode_sensitive_header() {
        let mut encoder = Encoder::new(4096);
        let mut buf = Vec::new();

        let headers = vec![
            HeaderField::new(":method", "GET").unwrap(),
            HeaderField::new_with_sensitive("authorization", "Bearer token", true).unwrap(),
        ];

        encoder.encode(&mut buf, &headers);

        // :method: GET should be indexed (0x82)
        assert_eq!(buf[0], 0x82);
        // authorization should be Never Indexed (0x1x prefix)
        assert_eq!(buf[1] & 0xF0, 0x10);
    }
}
