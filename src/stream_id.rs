//! HTTP/2 ストリーム識別子 (RFC 9113 §5.1.1)
//!
//! 奇偶ルールと値範囲を型レベルで強制するための newtype 群。

use core::num::NonZeroU32;

/// ストリーム ID の奇偶 (RFC 9113 §5.1.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Parity {
    /// クライアント開始ストリーム (奇数)
    Odd,
    /// サーバー開始ストリーム (偶数)
    Even,
}

/// ストリーム ID 構築時検査エラー
///
/// RFC 9113 §5.1.1: stream identifier は unsigned 31-bit integer、
/// クライアント開始は奇数、サーバー開始は偶数、0 は接続制御用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamIdError {
    /// 0 が指定された (本来許可しない構築点で)
    Reserved,

    /// 期待する奇偶と一致しない
    ParityMismatch {
        /// 期待する奇偶
        expected: Parity,
        /// 受け取った値
        got: u32,
    },

    /// 31-bit 範囲を超える値が指定された
    OutOfRange {
        /// 違反した値
        value: u32,
    },
}

impl std::fmt::Display for StreamIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reserved => write!(f, "stream ID 0 is reserved for connection control"),
            Self::ParityMismatch { expected, got } => {
                let expected_str = match expected {
                    Parity::Odd => "odd",
                    Parity::Even => "even",
                };
                write!(
                    f,
                    "stream ID {got} parity mismatch: expected {expected_str}"
                )
            }
            Self::OutOfRange { value } => write!(f, "stream ID {value} exceeds 31-bit range"),
        }
    }
}

impl std::error::Error for StreamIdError {}

/// 31-bit 上限 (2^31 - 1)
pub(crate) const STREAM_ID_MAX: u32 = (1u32 << 31) - 1;

/// クライアント開始ストリーム ID (奇数、1..=2^31-1)
///
/// RFC 9113 §5.1.1: クライアント開始ストリームは奇数 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientStreamId(NonZeroU32);

impl ClientStreamId {
    /// 構築時検査つきで生成する
    pub fn new(id: u32) -> Result<Self, StreamIdError> {
        if id == 0 {
            return Err(StreamIdError::Reserved);
        }
        if id > STREAM_ID_MAX {
            return Err(StreamIdError::OutOfRange { value: id });
        }
        if id.is_multiple_of(2) {
            return Err(StreamIdError::ParityMismatch {
                expected: Parity::Odd,
                got: id,
            });
        }
        Ok(Self(NonZeroU32::new(id).expect("non-zero checked above")))
    }

    /// const 文脈で生成する
    ///
    /// # 不正リテラルの compile-fail 例
    ///
    /// 0 は接続制御用なのでクライアントストリーム ID には使えない:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::ClientStreamId =
    ///     shiguredo_http2::ClientStreamId::from_static(0);
    /// ```
    ///
    /// 偶数 ID はサーバー開始の領域なのでクライアントには使えない:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::ClientStreamId =
    ///     shiguredo_http2::ClientStreamId::from_static(2);
    /// ```
    pub const fn from_static(id: u32) -> Self {
        assert!(
            id <= STREAM_ID_MAX,
            "ClientStreamId::from_static: id must be <= 2^31-1"
        );
        assert!(
            !id.is_multiple_of(2),
            "ClientStreamId::from_static: id must be odd (RFC 9113 §5.1.1)"
        );
        // NonZeroU32::new(0) で None になるため 0 もこの match で弾く
        match NonZeroU32::new(id) {
            Some(v) => Self(v),
            None => panic!("ClientStreamId::from_static: id must not be 0 (RFC 9113 §5.1.1)"),
        }
    }

    /// 検証済み値から構築する (crate 内部専用)
    pub(crate) fn from_validated_parts(id: NonZeroU32) -> Self {
        debug_assert!(
            id.get() <= STREAM_ID_MAX,
            "ClientStreamId::from_validated_parts: id must be <= 2^31-1"
        );
        debug_assert!(
            !id.get().is_multiple_of(2),
            "ClientStreamId::from_validated_parts: id must be odd"
        );
        Self(id)
    }

    /// `NonZeroU32` として取得する
    pub const fn get(self) -> NonZeroU32 {
        self.0
    }

    /// `u32` として取得する
    pub const fn as_u32(self) -> u32 {
        self.0.get()
    }
}

/// サーバー開始ストリーム ID (偶数、2..=2^31-2)
///
/// RFC 9113 §5.1.1: サーバー開始ストリームは偶数 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerStreamId(NonZeroU32);

impl ServerStreamId {
    /// 構築時検査つきで生成する
    pub fn new(id: u32) -> Result<Self, StreamIdError> {
        if id == 0 {
            return Err(StreamIdError::Reserved);
        }
        if id > STREAM_ID_MAX {
            return Err(StreamIdError::OutOfRange { value: id });
        }
        if !id.is_multiple_of(2) {
            return Err(StreamIdError::ParityMismatch {
                expected: Parity::Even,
                got: id,
            });
        }
        Ok(Self(NonZeroU32::new(id).expect("non-zero checked above")))
    }

    /// const 文脈で生成する
    ///
    /// # 不正リテラルの compile-fail 例
    ///
    /// 0 は接続制御用なのでサーバーストリーム ID には使えない:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::ServerStreamId =
    ///     shiguredo_http2::ServerStreamId::from_static(0);
    /// ```
    ///
    /// 奇数 ID はクライアント開始の領域なのでサーバーには使えない:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::ServerStreamId =
    ///     shiguredo_http2::ServerStreamId::from_static(1);
    /// ```
    pub const fn from_static(id: u32) -> Self {
        assert!(
            id <= STREAM_ID_MAX,
            "ServerStreamId::from_static: id must be <= 2^31-1"
        );
        assert!(
            id.is_multiple_of(2),
            "ServerStreamId::from_static: id must be even (RFC 9113 §5.1.1)"
        );
        // NonZeroU32::new(0) で None になるため 0 もこの match で弾く
        match NonZeroU32::new(id) {
            Some(v) => Self(v),
            None => panic!("ServerStreamId::from_static: id must not be 0 (RFC 9113 §5.1.1)"),
        }
    }

    /// 検証済み値から構築する (crate 内部専用)
    pub(crate) fn from_validated_parts(id: NonZeroU32) -> Self {
        debug_assert!(
            id.get() <= STREAM_ID_MAX,
            "ServerStreamId::from_validated_parts: id must be <= 2^31-1"
        );
        debug_assert!(
            id.get().is_multiple_of(2),
            "ServerStreamId::from_validated_parts: id must be even"
        );
        Self(id)
    }

    /// `NonZeroU32` として取得する
    pub const fn get(self) -> NonZeroU32 {
        self.0
    }

    /// `u32` として取得する
    pub const fn as_u32(self) -> u32 {
        self.0.get()
    }
}

/// 非ゼロストリーム ID (クライアントまたはサーバー開始)
///
/// stream_id = 0 を構造的に持てないフレーム構築点で使用される。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NonZeroStreamId {
    /// クライアント開始 (奇数)
    Client(ClientStreamId),
    /// サーバー開始 (偶数)
    Server(ServerStreamId),
}

impl NonZeroStreamId {
    /// wire 上の u32 を奇偶で分類しながら検査する
    pub fn new(id: u32) -> Result<Self, StreamIdError> {
        if id == 0 {
            return Err(StreamIdError::Reserved);
        }
        if id > STREAM_ID_MAX {
            return Err(StreamIdError::OutOfRange { value: id });
        }
        if id.is_multiple_of(2) {
            Ok(Self::Server(ServerStreamId::from_validated_parts(
                NonZeroU32::new(id).expect("non-zero checked above"),
            )))
        } else {
            Ok(Self::Client(ClientStreamId::from_validated_parts(
                NonZeroU32::new(id).expect("non-zero checked above"),
            )))
        }
    }

    /// const 文脈で生成する (奇偶を自動分類)
    ///
    /// # 不正リテラルの compile-fail 例
    ///
    /// 0 は接続制御用なので `NonZeroStreamId` には使えない:
    ///
    /// ```compile_fail
    /// const _BAD: shiguredo_http2::NonZeroStreamId =
    ///     shiguredo_http2::NonZeroStreamId::from_static(0);
    /// ```
    pub const fn from_static(id: u32) -> Self {
        assert!(id != 0, "NonZeroStreamId::from_static: id must not be 0");
        assert!(
            id <= STREAM_ID_MAX,
            "NonZeroStreamId::from_static: id must be <= 2^31-1"
        );
        if id.is_multiple_of(2) {
            Self::Server(ServerStreamId::from_static(id))
        } else {
            Self::Client(ClientStreamId::from_static(id))
        }
    }

    /// decoder 内部で検証済みの値から構築する
    ///
    /// 呼び出し側が「非ゼロかつ 31-bit 範囲」を保証していること。
    pub(crate) fn from_validated_parts(id: NonZeroU32) -> Self {
        debug_assert!(
            id.get() <= STREAM_ID_MAX,
            "NonZeroStreamId::from_validated_parts: id must be <= 2^31-1"
        );
        if id.get().is_multiple_of(2) {
            Self::Server(ServerStreamId::from_validated_parts(id))
        } else {
            Self::Client(ClientStreamId::from_validated_parts(id))
        }
    }

    /// `u32` として取得する
    pub const fn as_u32(self) -> u32 {
        match self {
            Self::Client(id) => id.as_u32(),
            Self::Server(id) => id.as_u32(),
        }
    }

    /// `ClientStreamId` として取得する (該当する場合)
    pub const fn client(self) -> Option<ClientStreamId> {
        match self {
            Self::Client(id) => Some(id),
            Self::Server(_) => None,
        }
    }

    /// `ServerStreamId` として取得する (該当する場合)
    pub const fn server(self) -> Option<ServerStreamId> {
        match self {
            Self::Client(_) => None,
            Self::Server(id) => Some(id),
        }
    }
}

impl From<ClientStreamId> for NonZeroStreamId {
    fn from(id: ClientStreamId) -> Self {
        Self::Client(id)
    }
}

impl From<ServerStreamId> for NonZeroStreamId {
    fn from(id: ServerStreamId) -> Self {
        Self::Server(id)
    }
}

impl std::fmt::Display for NonZeroStreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_u32())
    }
}

/// HTTP/2 ストリーム識別子 (RFC 9113 §5.1.1)
///
/// 接続制御用 (0) / クライアント開始 (奇数) / サーバー開始 (偶数) の 3 分類を型で表現する。
///
/// `PartialOrd` / `Ord` は意図的に derive しない。variant をまたいだ順序比較は
/// 意味的に不適切であり、順序比較が必要な箇所では `as_u32()` 経由で行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamId {
    /// 接続レベル制御用 (0)
    Connection,
    /// クライアント開始ストリーム (奇数)
    Client(ClientStreamId),
    /// サーバー開始ストリーム (偶数)
    Server(ServerStreamId),
}

impl StreamId {
    /// wire 上の u32 を分類する
    ///
    /// 呼び出し元が 31-bit マスク済み (id < 2^31) であることを前提とする。
    /// decoder の `decode_header` が上位 1 ビットをマスクするため、この前提は常に成立する。
    pub fn from_wire(id: u32) -> Self {
        debug_assert!(
            id <= STREAM_ID_MAX,
            "StreamId::from_wire: id must be <= 2^31-1, got {id}"
        );
        if id == 0 {
            Self::Connection
        } else if id.is_multiple_of(2) {
            Self::Server(ServerStreamId::from_validated_parts(
                NonZeroU32::new(id).expect("non-zero checked above"),
            ))
        } else {
            Self::Client(ClientStreamId::from_validated_parts(
                NonZeroU32::new(id).expect("non-zero checked above"),
            ))
        }
    }

    /// `u32` として取得する
    pub const fn as_u32(self) -> u32 {
        match self {
            Self::Connection => 0,
            Self::Client(id) => id.as_u32(),
            Self::Server(id) => id.as_u32(),
        }
    }

    /// `NonZeroStreamId` として取得する (`Connection` の場合は `None`)
    pub const fn non_zero(self) -> Option<NonZeroStreamId> {
        match self {
            Self::Connection => None,
            Self::Client(id) => Some(NonZeroStreamId::Client(id)),
            Self::Server(id) => Some(NonZeroStreamId::Server(id)),
        }
    }
}

impl std::fmt::Display for StreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_u32())
    }
}

impl From<ClientStreamId> for StreamId {
    fn from(id: ClientStreamId) -> Self {
        Self::Client(id)
    }
}

impl From<ServerStreamId> for StreamId {
    fn from(id: ServerStreamId) -> Self {
        Self::Server(id)
    }
}

impl From<NonZeroStreamId> for StreamId {
    fn from(id: NonZeroStreamId) -> Self {
        match id {
            NonZeroStreamId::Client(c) => Self::Client(c),
            NonZeroStreamId::Server(s) => Self::Server(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_stream_id_from_validated_parts() {
        let raw = NonZeroU32::new(5).expect("non-zero stream id");
        let id = ClientStreamId::from_validated_parts(raw);
        assert_eq!(id.as_u32(), 5);
    }

    #[test]
    fn server_stream_id_from_validated_parts() {
        let raw = NonZeroU32::new(6).expect("non-zero stream id");
        let id = ServerStreamId::from_validated_parts(raw);
        assert_eq!(id.as_u32(), 6);
    }

    mod validated_parts {
        use proptest::prelude::*;

        use super::{ClientStreamId, NonZeroStreamId, NonZeroU32, STREAM_ID_MAX, ServerStreamId};

        proptest! {
            #[test]
            fn client_stream_id_validated_matches_new(
                id in (1u32..=STREAM_ID_MAX).prop_filter(
                    "奇数のみ",
                    |id| id % 2 == 1,
                ),
            ) {
                let via_new = ClientStreamId::new(id).expect("valid client stream id");
                let nz = NonZeroU32::new(id).expect("non-zero stream id");
                let via_validated = ClientStreamId::from_validated_parts(nz);
                prop_assert_eq!(via_new, via_validated);
            }

            #[test]
            fn server_stream_id_validated_matches_new(
                id in (2u32..=STREAM_ID_MAX).prop_filter(
                    "偶数のみ",
                    |id| id % 2 == 0,
                ),
            ) {
                let via_new = ServerStreamId::new(id).expect("valid server stream id");
                let nz = NonZeroU32::new(id).expect("non-zero stream id");
                let via_validated = ServerStreamId::from_validated_parts(nz);
                prop_assert_eq!(via_new, via_validated);
            }

            #[test]
            fn non_zero_stream_id_validated_matches_new(
                id in 1u32..=STREAM_ID_MAX,
            ) {
                let via_new = NonZeroStreamId::new(id).expect("valid non-zero stream id");
                let nz = NonZeroU32::new(id).expect("non-zero stream id");
                let via_validated = NonZeroStreamId::from_validated_parts(nz);
                prop_assert_eq!(via_new, via_validated);
            }
        }
    }
}
