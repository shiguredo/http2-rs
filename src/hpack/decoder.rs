//! HPACK デコーダー (RFC 7541)

use bytes::Bytes;

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
                    .insert(header.name.clone(), header.value.clone());
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
            header.name
        };

        let (value, c) = self.decode_string(&data[consumed..])?;
        consumed += c;

        Ok((HeaderField::new(name, value), consumed))
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
            header.name
        };

        let (value, c) = self.decode_string(&data[consumed..])?;
        consumed += c;

        Ok((HeaderField::new(name, value), consumed))
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
            header.name
        };

        let (value, c) = self.decode_string(&data[consumed..])?;
        consumed += c;

        Ok((HeaderField::new_sensitive(name, value, true), consumed))
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
    ///
    /// Huffman 符号化されている場合は復号後の所有バッファを `Bytes::from(Vec<u8>)` で
    /// 包む (allocation 1 回)。リテラルの場合は入力 `&[u8]` から `Bytes::copy_from_slice`
    /// で複製する (将来 HPACK Decoder の入力を `BytesMut` 化すれば split_to で zero-copy
    /// にできるが、本 issue では非スコープ)。
    fn decode_string(&self, data: &[u8]) -> Result<(Bytes, usize)> {
        if data.is_empty() {
            return Err(Error::incomplete());
        }

        let huffman_encoded = data[0] & 0x80 != 0;
        let (length, mut consumed) = integer::decode(data, 7)?;

        let length = length as usize;
        if data.len() < consumed + length {
            return Err(Error::incomplete());
        }

        let string_data = &data[consumed..consumed + length];
        consumed += length;

        let result = if huffman_encoded {
            Bytes::from(huffman::decode(string_data)?)
        } else {
            Bytes::copy_from_slice(string_data)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hpack::Encoder;

    #[test]
    fn test_decode_indexed() {
        let mut decoder = Decoder::new(4096);

        // :method: GET (index 2) = 0x82
        let data = [0x82];
        let headers = decoder.decode(&data).unwrap();

        assert_eq!(headers.len(), 1);
        assert_eq!(&headers[0].name[..], b":method");
        assert_eq!(&headers[0].value[..], b"GET");
    }

    #[test]
    fn test_decode_literal_indexed() {
        let mut decoder = Decoder::new(4096);

        // :authority (index 1) with value "example.com" (not Huffman encoded)
        // 0x41 = 01000001 (incremental indexing, index 1)
        // 0x0b = length 11
        // "example.com"
        let mut data = vec![0x41, 0x0b];
        data.extend_from_slice(b"example.com");

        let headers = decoder.decode(&data).unwrap();

        assert_eq!(headers.len(), 1);
        assert_eq!(&headers[0].name[..], b":authority");
        assert_eq!(&headers[0].value[..], b"example.com");

        // 動的テーブルに追加されていることを確認
        assert_eq!(decoder.dynamic_table().len(), 1);
    }

    #[test]
    fn test_roundtrip() {
        let mut encoder = Encoder::new(4096);
        let mut decoder = Decoder::new(4096);

        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::from_str(":path", "/index.html"),
            HeaderField::from_str(":scheme", "https"),
            HeaderField::from_str(":authority", "www.example.com"),
            HeaderField::from_str("custom-header", "custom-value"),
        ];

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        let decoded = decoder.decode(&encoded).unwrap();

        assert_eq!(decoded.len(), headers.len());
        for (original, decoded) in headers.iter().zip(decoded.iter()) {
            assert_eq!(original.name, decoded.name);
            assert_eq!(original.value, decoded.value);
        }
    }

    #[test]
    fn test_decode_size_update() {
        let mut decoder = Decoder::new(4096);

        // Size update to 1024 = 0x3f (5-bit prefix) + continuation
        // 0x20 | (31 & 0x1f) = 0x3f, then 1024 - 31 = 993 = 0xe1 0x07
        let data = [0x3f, 0xe1, 0x07];
        let headers = decoder.decode(&data).unwrap();

        assert!(headers.is_empty());
        assert_eq!(decoder.dynamic_table().max_size(), 1024);
    }

    #[test]
    fn test_decode_never_indexed() {
        let mut decoder = Decoder::new(4096);

        // Never Indexed with new name "x-token" and value "secret"
        // 0x10 = pattern 00010000 (never indexed, new name)
        // 0x07 = length 7 (not Huffman)
        // "x-token"
        // 0x06 = length 6 (not Huffman)
        // "secret"
        let mut data = vec![0x10, 0x07];
        data.extend_from_slice(b"x-token");
        data.push(0x06);
        data.extend_from_slice(b"secret");

        let headers = decoder.decode(&data).unwrap();

        assert_eq!(headers.len(), 1);
        assert_eq!(&headers[0].name[..], b"x-token");
        assert_eq!(&headers[0].value[..], b"secret");
        assert!(headers[0].sensitive);

        // Never Indexed should not be added to dynamic table
        assert_eq!(decoder.dynamic_table().len(), 0);
    }

    #[test]
    fn test_decode_never_indexed_with_name_index() {
        let mut decoder = Decoder::new(4096);

        // Never Indexed with name index 7 (:scheme) and value "https"
        // 0x17 = pattern 0001 + 0111 (never indexed, index 7)
        // 0x05 = length 5 (not Huffman)
        // "https"
        let mut data = vec![0x17, 0x05];
        data.extend_from_slice(b"https");

        let headers = decoder.decode(&data).unwrap();

        assert_eq!(headers.len(), 1);
        assert_eq!(&headers[0].name[..], b":scheme");
        assert_eq!(&headers[0].value[..], b"https");
        assert!(headers[0].sensitive);
    }

    #[test]
    fn test_roundtrip_with_sensitive() {
        let mut encoder = Encoder::new(4096);
        let mut decoder = Decoder::new(4096);

        let headers = vec![
            HeaderField::from_str(":method", "GET"),
            HeaderField::sensitive("authorization", "Bearer token"),
            HeaderField::from_str(":path", "/"),
        ];

        let mut encoded = Vec::new();
        encoder.encode(&mut encoded, &headers);

        let decoded = decoder.decode(&encoded).unwrap();

        assert_eq!(decoded.len(), 3);
        assert_eq!(&decoded[0].name[..], b":method");
        assert!(!decoded[0].sensitive);
        assert_eq!(&decoded[1].name[..], b"authorization");
        assert!(decoded[1].sensitive);
        assert_eq!(&decoded[2].name[..], b":path");
        assert!(!decoded[2].sensitive);
    }
}
