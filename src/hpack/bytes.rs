//! `HeaderField` の内部表現用バイト列型
//!
//! `enum HeaderBytes { Static(&'static [u8]), Owned(Vec<u8>) }` を提供する。
//! 静的バイト列を `const fn` の [`crate::hpack::HeaderField::from_static`]
//! で構築できるようにするために導入する。

/// HPACK ヘッダーの name/value 用バイト列表現
///
/// - `Static(&'static [u8])`: リテラル等の静的バイト列。
///   `HeaderField::from_static` から構築される。
/// - `Owned(Vec<u8>)`: 所有バイト列。ランタイム値や HPACK decoder 経路から構築される。
///
/// 等価性とハッシュは `as_slice()` のバイト列としての一致のみを見るため、
/// `Static(b"GET")` と `Owned(b"GET".to_vec())` は同値と扱う。
#[derive(Debug, Clone)]
pub(crate) enum HeaderBytes {
    /// 'static バイト列
    Static(&'static [u8]),
    /// 所有バイト列
    Owned(Vec<u8>),
}

impl HeaderBytes {
    /// バイトスライスへの参照を取得する
    pub(crate) fn as_slice(&self) -> &[u8] {
        match self {
            Self::Static(s) => s,
            Self::Owned(v) => v.as_slice(),
        }
    }

    /// 長さを取得する
    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }
}

impl PartialEq for HeaderBytes {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for HeaderBytes {}

impl std::hash::Hash for HeaderBytes {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    #[test]
    fn header_bytes_as_slice() {
        let s = HeaderBytes::Static(b"foo");
        assert_eq!(s.as_slice(), b"foo");
        let o = HeaderBytes::Owned(b"bar".to_vec());
        assert_eq!(o.as_slice(), b"bar");
    }

    #[test]
    fn header_bytes_len() {
        assert_eq!(HeaderBytes::Static(b"").len(), 0);
        assert_eq!(HeaderBytes::Owned(b"abc".to_vec()).len(), 3);
    }

    #[test]
    fn header_bytes_static_owned_equal_when_same_slice() {
        let s = HeaderBytes::Static(b"GET");
        let o = HeaderBytes::Owned(b"GET".to_vec());
        assert_eq!(s, o);
        assert_eq!(o, s);
    }

    #[test]
    fn header_bytes_hash_consistent_across_variants() {
        let s = HeaderBytes::Static(b"content-type");
        let o = HeaderBytes::Owned(b"content-type".to_vec());
        let mut hs = DefaultHasher::new();
        s.hash(&mut hs);
        let mut ho = DefaultHasher::new();
        o.hash(&mut ho);
        assert_eq!(hs.finish(), ho.finish());
    }
}
