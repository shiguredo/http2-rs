//! ストリーム ID 構築時検査型 (issue 0025 / 0029)
//!
//! HTTP/2 ストリーム識別子 (RFC 9113 §5.1.1) の奇偶ルールと値範囲を
//! 型レベルで強制するための newtype 群。
//!
//! 本ファイルは issue 0025 構築時検査リファクタリングの Phase 1 として追加された。
//! 既存の `pub type StreamId = u32` ([`crate::frame::StreamId`]) は Phase 2 で
//! 本モジュールの enum 型に置き換えられる予定。

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
const STREAM_ID_MAX: u32 = (1u32 << 31) - 1;

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
    pub const fn from_static(id: u32) -> Self {
        assert!(id != 0, "ClientStreamId::from_static: id must not be 0");
        assert!(
            id <= STREAM_ID_MAX,
            "ClientStreamId::from_static: id must be <= 2^31-1"
        );
        assert!(
            !id.is_multiple_of(2),
            "ClientStreamId::from_static: id must be odd (RFC 9113 §5.1.1)"
        );
        match NonZeroU32::new(id) {
            Some(v) => Self(v),
            None => panic!("ClientStreamId::from_static: id must not be 0"),
        }
    }

    /// 検証済み値から構築する (crate 内部専用)
    #[allow(dead_code)] // issue 0030 Phase 2 で decoder から呼ばれる予定
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
    pub const fn from_static(id: u32) -> Self {
        assert!(id != 0, "ServerStreamId::from_static: id must not be 0");
        assert!(
            id <= STREAM_ID_MAX,
            "ServerStreamId::from_static: id must be <= 2^31-1"
        );
        assert!(
            id.is_multiple_of(2),
            "ServerStreamId::from_static: id must be even (RFC 9113 §5.1.1)"
        );
        match NonZeroU32::new(id) {
            Some(v) => Self(v),
            None => panic!("ServerStreamId::from_static: id must not be 0"),
        }
    }

    /// 検証済み値から構築する (crate 内部専用)
    #[allow(dead_code)] // issue 0030 Phase 2 で decoder から呼ばれる予定
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

    /// 検証済み値から構築する (crate 内部専用)
    #[allow(dead_code)] // issue 0030 Phase 2 で decoder から呼ばれる予定
    pub(crate) fn from_validated_parts(id: NonZeroU32) -> Self {
        debug_assert!(id.get() <= STREAM_ID_MAX);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_stream_id_new_ok() {
        let id = ClientStreamId::new(1).unwrap();
        assert_eq!(id.as_u32(), 1);
        let id = ClientStreamId::new(2147483647).unwrap();
        assert_eq!(id.as_u32(), 2147483647);
    }

    #[test]
    fn client_stream_id_new_reserved() {
        assert_eq!(ClientStreamId::new(0), Err(StreamIdError::Reserved));
    }

    #[test]
    fn client_stream_id_new_even() {
        assert_eq!(
            ClientStreamId::new(2),
            Err(StreamIdError::ParityMismatch {
                expected: Parity::Odd,
                got: 2,
            })
        );
    }

    #[test]
    fn client_stream_id_new_out_of_range() {
        assert_eq!(
            ClientStreamId::new(STREAM_ID_MAX + 1),
            Err(StreamIdError::OutOfRange {
                value: STREAM_ID_MAX + 1,
            })
        );
    }

    #[test]
    fn client_stream_id_from_static() {
        const ID: ClientStreamId = ClientStreamId::from_static(7);
        assert_eq!(ID.as_u32(), 7);
    }

    #[test]
    fn server_stream_id_new_ok() {
        let id = ServerStreamId::new(2).unwrap();
        assert_eq!(id.as_u32(), 2);
    }

    #[test]
    fn server_stream_id_new_odd() {
        assert_eq!(
            ServerStreamId::new(3),
            Err(StreamIdError::ParityMismatch {
                expected: Parity::Even,
                got: 3,
            })
        );
    }

    #[test]
    fn server_stream_id_from_static() {
        const ID: ServerStreamId = ServerStreamId::from_static(4);
        assert_eq!(ID.as_u32(), 4);
    }

    #[test]
    fn non_zero_stream_id_new_classifies_parity() {
        let id = NonZeroStreamId::new(1).unwrap();
        assert!(matches!(id, NonZeroStreamId::Client(_)));
        assert_eq!(id.as_u32(), 1);

        let id = NonZeroStreamId::new(4).unwrap();
        assert!(matches!(id, NonZeroStreamId::Server(_)));
        assert_eq!(id.as_u32(), 4);
    }

    #[test]
    fn non_zero_stream_id_new_reserved() {
        assert_eq!(NonZeroStreamId::new(0), Err(StreamIdError::Reserved));
    }

    #[test]
    fn non_zero_stream_id_new_out_of_range() {
        assert_eq!(
            NonZeroStreamId::new(STREAM_ID_MAX + 1),
            Err(StreamIdError::OutOfRange {
                value: STREAM_ID_MAX + 1,
            })
        );
    }

    #[test]
    fn non_zero_stream_id_from_static_client() {
        const ID: NonZeroStreamId = NonZeroStreamId::from_static(9);
        assert!(matches!(ID, NonZeroStreamId::Client(_)));
        assert_eq!(ID.as_u32(), 9);
    }

    #[test]
    fn non_zero_stream_id_from_static_server() {
        const ID: NonZeroStreamId = NonZeroStreamId::from_static(8);
        assert!(matches!(ID, NonZeroStreamId::Server(_)));
        assert_eq!(ID.as_u32(), 8);
    }

    #[test]
    fn non_zero_stream_id_client_and_server() {
        let id = NonZeroStreamId::Client(ClientStreamId::from_static(5));
        assert_eq!(id.client().unwrap().as_u32(), 5);
        assert!(id.server().is_none());

        let id = NonZeroStreamId::Server(ServerStreamId::from_static(6));
        assert!(id.client().is_none());
        assert_eq!(id.server().unwrap().as_u32(), 6);
    }

    #[test]
    fn from_client_and_server_into_non_zero() {
        let c = ClientStreamId::from_static(3);
        let id: NonZeroStreamId = c.into();
        assert_eq!(id.as_u32(), 3);

        let s = ServerStreamId::from_static(4);
        let id: NonZeroStreamId = s.into();
        assert_eq!(id.as_u32(), 4);
    }

    #[test]
    fn stream_id_error_display() {
        assert_eq!(
            StreamIdError::Reserved.to_string(),
            "stream ID 0 is reserved for connection control"
        );
        assert_eq!(
            StreamIdError::ParityMismatch {
                expected: Parity::Odd,
                got: 4,
            }
            .to_string(),
            "stream ID 4 parity mismatch: expected odd"
        );
        assert_eq!(
            StreamIdError::ParityMismatch {
                expected: Parity::Even,
                got: 5,
            }
            .to_string(),
            "stream ID 5 parity mismatch: expected even"
        );
        assert_eq!(
            StreamIdError::OutOfRange { value: u32::MAX }.to_string(),
            format!("stream ID {} exceeds 31-bit range", u32::MAX)
        );
    }

    #[test]
    fn client_stream_id_from_validated_parts() {
        let raw = NonZeroU32::new(5).unwrap();
        let id = ClientStreamId::from_validated_parts(raw);
        assert_eq!(id.as_u32(), 5);
    }

    #[test]
    fn server_stream_id_from_validated_parts() {
        let raw = NonZeroU32::new(6).unwrap();
        let id = ServerStreamId::from_validated_parts(raw);
        assert_eq!(id.as_u32(), 6);
    }

    #[test]
    fn non_zero_stream_id_from_validated_parts() {
        let raw = NonZeroU32::new(7).unwrap();
        let id = NonZeroStreamId::from_validated_parts(raw);
        assert!(matches!(id, NonZeroStreamId::Client(_)));
        assert_eq!(id.as_u32(), 7);
    }
}
