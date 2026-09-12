//! WebTransport ストリーム管理 (RFC 9000 Section 2, 3)
//!
//! # ストリーム ID (RFC 9000 Section 2.1)
//!
//! | Bits | Stream Type                      |
//! |------|----------------------------------|
//! | 0x00 | Client-Initiated, Bidirectional  |
//! | 0x01 | Server-Initiated, Bidirectional  |
//! | 0x02 | Client-Initiated, Unidirectional |
//! | 0x03 | Server-Initiated, Unidirectional |
//!
//! - Bit 0 (0x01): 0=Client-Initiated, 1=Server-Initiated
//! - Bit 1 (0x02): 0=Bidirectional, 1=Unidirectional

use crate::webtransport::error::{WtError, WtResult};

/// WebTransport ストリーム ID (62-bit, RFC 9000 Section 2.1)
pub type WtStreamId = u64;

/// ストリーム ID 操作
pub mod stream_id {
    use super::WtStreamId;

    /// クライアント開始か
    #[must_use]
    pub const fn is_client_initiated(id: WtStreamId) -> bool {
        id & 0x01 == 0
    }

    /// サーバー開始か
    #[must_use]
    pub const fn is_server_initiated(id: WtStreamId) -> bool {
        id & 0x01 == 1
    }

    /// 双方向か
    #[must_use]
    pub const fn is_bidirectional(id: WtStreamId) -> bool {
        id & 0x02 == 0
    }

    /// 単方向か
    #[must_use]
    pub const fn is_unidirectional(id: WtStreamId) -> bool {
        id & 0x02 == 2
    }

    /// 次のストリーム ID を生成 (同タイプ)
    #[must_use]
    pub const fn next(id: WtStreamId) -> WtStreamId {
        id + 4
    }

    /// 初期ストリーム ID
    #[must_use]
    pub const fn first(client: bool, bidirectional: bool) -> WtStreamId {
        let initiator = if client { 0 } else { 1 };
        let direction = if bidirectional { 0 } else { 2 };
        initiator | direction
    }
}

/// 送信側ストリーム状態
///
/// RFC 9000 Section 3.1, Figure 2: States for Sending Parts of Streams のうち、
/// 本実装が到達する状態を表す。
///
/// ```text
///        o
///        | Create Stream (Sending)
///        | Peer Creates Bidirectional Stream
///        v
///    +-------+
///    | Ready | Send RESET_STREAM
///    |       |-----------------------.
///    +-------+                       |
///        |                           |
///        | Send STREAM /             |
///        |      STREAM_DATA_BLOCKED  |
///        v                           |
///    +-------+                       |
///    | Send  | Send RESET_STREAM     |
///    |       |---------------------->|
///    +-------+                       |
///        |                           |
///        | Send STREAM + FIN         |
///        v                           v
///    +-------+                   +-------+
///    | Data  |                   | Reset |
///    | Recvd |                   | Recvd |
///    +-------+                   +-------+
/// ```
///
/// draft-ietf-webtrans-http2-15 Section 5.2: HTTP/2 は順序配送であり、ACK を待たずに
/// 状態を遷移させるため、RFC 9000 Section 3.1 の "Data Sent" と "Reset Sent" は経由しない
/// (FIN / RESET_STREAM の送信と同時に終端状態へ遷移する)。
///
/// 図は `WtSession` の操作で起こる遷移を示す。`WtStream::send_data` は `Ready` から
/// FIN 付きで送信すると `DataRecvd` へ直接遷移し、`WtStream::send_reset` は状態を
/// 検証しないため `DataRecvd` からも `ResetRecvd` へ遷移しうる。
///
/// RFC 9000 Section 3.1: STOP_SENDING を受信したエンドポイントは RESET_STREAM を送信する
/// (図の `Ready` / `Send` から `ResetRecvd` への遷移)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SendState {
    /// 初期状態
    #[default]
    Ready,
    /// STREAM/STREAM_DATA_BLOCKED 送信後
    Send,
    /// 終端: FIN を送信済み
    DataRecvd,
    /// 終端: RESET_STREAM を送信済み
    ResetRecvd,
}

impl SendState {
    /// 送信可能かどうかを返す
    #[must_use]
    pub const fn can_send(self) -> bool {
        matches!(self, Self::Ready | Self::Send)
    }

    /// 終端状態かどうかを返す
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::DataRecvd | Self::ResetRecvd)
    }
}

/// 受信側ストリーム状態
///
/// RFC 9000 Section 3.2, Figure 3: States for Receiving Parts of Streams のうち、
/// 本実装が到達する状態を表す。
///
/// ```text
///        o
///        | Recv STREAM / STREAM_DATA_BLOCKED / RESET_STREAM
///        | Create Bidirectional Stream (Sending)
///        | Recv MAX_STREAM_DATA / STOP_SENDING (Bidirectional)
///        | Create Higher-Numbered Stream
///        v
///    +-------+
///    | Recv  | Recv RESET_STREAM
///    |       |-----------------------.
///    +-------+                       |
///        |                           |
///        | Recv STREAM + FIN         |
///        v                           v
///    +-------+                   +-------+
///    | Data  |                   | Reset |
///    | Recvd |                   | Read  |
///    +-------+                   +-------+
///        |
///        | App Read All Data
///        v
///    +-------+
///    | Data  |
///    | Read  |
///    +-------+
/// ```
///
/// draft-ietf-webtrans-http2-15 Section 5.2: HTTP/2 は順序配送であり、後続データの有無が
/// FIN の受信時点で確定するため、RFC 9000 Section 3.2 の "Size Known" を経由せず、
/// 最終サイズの確定と同時に "Data Recvd" へ遷移する。また RESET_STREAM の受信時には
/// アプリへの通知 (`WtEvent::StreamReset`) まで同時に行うため、RFC 9000 Section 3.2 の
/// "Reset Recvd" を経由せず "Reset Read" へ直接遷移する。
///
/// 図は `WtSession` の capsule 処理と `poll_event` による配送で起こる遷移を示す。
/// `WtStream::recv_reset` は状態を検証しないため、`WtStream` の公開メソッドを
/// 直接呼ぶ場合は図に無い遷移も起こりうる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecvState {
    /// 初期状態
    #[default]
    Recv,
    /// FIN を受信済み (アプリが読み取ると DataRead になり終端となる)
    DataRecvd,
    /// 終端: アプリがデータ読み取り完了
    DataRead,
    /// 終端: RESET_STREAM を受信し、アプリへの通知を配送キューへ積んだ状態
    ResetRead,
}

impl RecvState {
    /// 受信可能かどうかを返す
    #[must_use]
    pub const fn can_recv(self) -> bool {
        matches!(self, Self::Recv)
    }

    /// 終端状態かどうかを返す
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::DataRead | Self::ResetRead)
    }
}

/// WebTransport ストリーム
#[derive(Debug)]
pub struct WtStream {
    /// ストリーム ID
    id: WtStreamId,
    /// 双方向ストリームかどうか
    bidirectional: bool,
    /// ローカルが開設したストリームかどうか
    locally_initiated: bool,
    /// 送信状態
    send_state: SendState,
    /// 受信状態
    recv_state: RecvState,
    /// 送信済みバイト数 (オフセット)
    send_offset: u64,
    /// 受信済みバイト数 (オフセット)
    recv_offset: u64,
    /// ピアが許可した送信上限
    send_max: u64,
    /// ローカルが許可した受信上限 (ピアへ最後に広告した値)
    ///
    /// `WtSession::send_max_stream_data` と `WtSession::grow_stream_recv_window` が
    /// 広告と同時に更新する
    recv_max: u64,
    /// STOP_SENDING を送信したかどうか
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.3: 冪等性チェック用。Section 6.6:
    /// WT_STOP_SENDING を送った側は同じストリームへ WT_MAX_STREAM_DATA を送れないため、
    /// 送信側 API (`WtSession::send_max_stream_data` /
    /// `WtSession::grow_stream_recv_window`) の検証にも使用する
    stop_sending_sent: bool,
    /// STOP_SENDING を受信したかどうか
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.3 / Section 6.6: 2 回目の
    /// WT_STOP_SENDING の拒否と、WT_STOP_SENDING を受信した後の
    /// WT_MAX_STREAM_DATA の拒否に使用する
    stop_sending_received: bool,
    /// データを受信したかどうか
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.4: empty capsule チェック用
    has_received_data: bool,
}

impl WtStream {
    /// 新しいストリームを生成する
    ///
    /// `send_max` にはピアが広告した送信上限、`recv_max` にはローカルが広告した受信上限を渡す。
    #[must_use]
    pub fn new(
        id: WtStreamId,
        send_max: u64,
        recv_max: u64,
        bidirectional: bool,
        locally_initiated: bool,
    ) -> Self {
        Self {
            id,
            bidirectional,
            locally_initiated,
            send_state: SendState::Ready,
            recv_state: RecvState::Recv,
            send_offset: 0,
            recv_offset: 0,
            send_max,
            recv_max,
            stop_sending_sent: false,
            stop_sending_received: false,
            has_received_data: false,
        }
    }

    /// ストリーム ID を取得する
    #[must_use]
    pub const fn id(&self) -> WtStreamId {
        self.id
    }

    /// 双方向ストリームかどうかを返す
    #[must_use]
    pub const fn is_bidirectional(&self) -> bool {
        self.bidirectional
    }

    /// 受信パートがあるかどうかを返す
    ///
    /// 双方向ストリーム、またはピアが開始した単方向ストリームは受信パートを持つ。
    /// ローカルが開始した単方向ストリーム (送信専用) は受信パートを持たない。
    /// [`Self::can_recv`] が受信状態のみを見るのに対し、本メソッドはストリームの方向で
    /// 判定する。
    #[must_use]
    pub const fn has_recv_part(&self) -> bool {
        self.bidirectional || !self.locally_initiated
    }

    /// 送信パートがあるかどうかを返す
    ///
    /// 双方向ストリーム、またはローカルが開始した単方向ストリームは送信パートを持つ。
    /// ピアが開始した単方向ストリーム (受信専用) は送信パートを持たない。
    /// [`Self::can_send`] が送信状態のみを見るのに対し、本メソッドはストリームの方向で
    /// 判定する。
    #[must_use]
    pub const fn has_send_part(&self) -> bool {
        self.bidirectional || self.locally_initiated
    }

    /// 送信状態を取得する
    #[must_use]
    pub const fn send_state(&self) -> SendState {
        self.send_state
    }

    /// 受信状態を取得する
    #[must_use]
    pub const fn recv_state(&self) -> RecvState {
        self.recv_state
    }

    /// 送信可能かどうかを返す
    #[must_use]
    pub const fn can_send(&self) -> bool {
        self.send_state.can_send()
    }

    /// 受信可能かどうかを返す
    #[must_use]
    pub const fn can_recv(&self) -> bool {
        self.recv_state.can_recv()
    }

    /// 送信済みバイト数を取得する
    #[must_use]
    pub const fn send_offset(&self) -> u64 {
        self.send_offset
    }

    /// 受信済みバイト数を取得する
    #[must_use]
    pub const fn recv_offset(&self) -> u64 {
        self.recv_offset
    }

    /// 送信可能な残りバイト数を取得する
    #[must_use]
    pub fn send_available(&self) -> u64 {
        self.send_max.saturating_sub(self.send_offset)
    }

    /// 受信可能な残りバイト数を取得する
    #[must_use]
    pub fn recv_available(&self) -> u64 {
        self.recv_max.saturating_sub(self.recv_offset)
    }

    /// 受信上限 (ピアへ最後に広告した最大バイト数) を取得する
    #[must_use]
    pub const fn recv_max(&self) -> u64 {
        self.recv_max
    }

    /// データを送信する
    ///
    /// # 引数
    ///
    /// - `size`: 送信するバイト数
    /// - `fin`: FIN フラグ
    pub fn send_data(&mut self, size: u64, fin: bool) -> WtResult<()> {
        if !self.send_state.can_send() {
            return Err(WtError::stream_state_error("cannot send in current state"));
        }

        // draft-ietf-webtrans-http2-15 Section 6.6: ストリームレベルのフロー制御上限チェック
        let new_offset = self.send_offset.saturating_add(size);
        if new_offset > self.send_max {
            return Err(WtError::flow_control_error("stream send limit exceeded"));
        }
        self.send_offset = new_offset;

        if fin {
            // draft-ietf-webtrans-http2-15 Section 5.2: HTTP/2 の順序配送により
            // ACK が不要なため、FIN の送信と同時に終端状態へ遷移する
            self.send_state = SendState::DataRecvd;
        } else {
            self.send_state = SendState::Send;
        }

        Ok(())
    }

    /// リセットを送信する
    pub fn send_reset(&mut self) {
        // draft-ietf-webtrans-http2-15 Section 5.2: HTTP/2 の順序配送により
        // ACK が不要なため、RESET_STREAM の送信と同時に終端状態へ遷移する
        self.send_state = SendState::ResetRecvd;
    }

    /// データを受信する
    ///
    /// # 引数
    ///
    /// - `size`: 受信したバイト数
    /// - `fin`: FIN フラグ
    pub fn recv_data(&mut self, size: u64, fin: bool) -> WtResult<()> {
        if !self.recv_state.can_recv() {
            return Err(WtError::stream_state_error(
                "cannot receive in current state",
            ));
        }

        // draft-ietf-webtrans-http2-15 Section 6.6: ストリームレベルのフロー制御上限チェック
        let new_offset = self.recv_offset.saturating_add(size);
        if new_offset > self.recv_max {
            return Err(WtError::flow_control_error("stream recv limit exceeded"));
        }
        self.recv_offset = new_offset;

        if fin {
            // draft-ietf-webtrans-http2-15 Section 5.2: HTTP/2 の順序配送により
            // 全データ到着済みなので、最終サイズの確定と同時に DataRecvd へ遷移する
            self.recv_state = RecvState::DataRecvd;
        }

        Ok(())
    }

    /// リセットを受信する
    pub fn recv_reset(&mut self) {
        // RESET_STREAM の受信とアプリへの通知 (WtEvent::StreamReset) を同時に行うため、
        // アプリの読み取りを待たずに終端状態へ遷移する
        self.recv_state = RecvState::ResetRead;
    }

    /// 受信データをアプリケーションが読み取ったことをマークする
    ///
    /// `poll_event()` で `StreamData { fin: true }` を pop した際に呼び出す。
    /// DataRecvd → DataRead へ遷移する。
    pub fn mark_data_read(&mut self) {
        if self.recv_state == RecvState::DataRecvd {
            self.recv_state = RecvState::DataRead;
        }
    }

    /// 送信上限を更新する
    ///
    /// draft-ietf-webtrans-http2-15 Section 6.6:
    /// 値が減少した場合は WT_FLOW_CONTROL_ERROR セッションエラーを返す。
    ///
    /// 注: draft-ietf-webtrans-http2-15 由来の暫定仕様であり、RFC 化に伴い変更される可能性がある。
    pub fn update_send_max(&mut self, maximum: u64) -> WtResult<()> {
        if maximum < self.send_max {
            return Err(WtError::flow_control_error(
                "WT_MAX_STREAM_DATA value decreased",
            ));
        }
        self.send_max = maximum;
        Ok(())
    }

    /// 受信上限を更新する
    ///
    /// varint の最大値 (2^62 - 1) を超えた場合はエラーを返す (RFC 9000 Section 16)。
    /// 現在値より大きい場合のみ反映し、同値と減少は無視する
    /// (RFC 9000 Section 4.1: 小さい上限の広告はエラーではなく効果が無い)。
    pub fn update_recv_max(&mut self, maximum: u64) -> WtResult<()> {
        if maximum > super::varint::MAX_VALUE {
            return Err(WtError::flow_control_error(
                "recv_max exceeds varint maximum value",
            ));
        }
        if maximum > self.recv_max {
            self.recv_max = maximum;
        }
        Ok(())
    }

    /// ストリームが完全に閉じたかどうかを返す
    ///
    /// 双方向: 送信側と受信側の両方が終端状態
    /// 送信専用単方向 (ローカル開設 uni): 送信側の終端のみ
    /// 受信専用単方向 (ピア開設 uni): 受信側の終端のみ
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        if self.bidirectional {
            self.send_state.is_terminal() && self.recv_state.is_terminal()
        } else if self.locally_initiated {
            self.send_state.is_terminal()
        } else {
            self.recv_state.is_terminal()
        }
    }

    /// STOP_SENDING を送信済みかどうかを返す
    #[must_use]
    pub const fn stop_sending_sent(&self) -> bool {
        self.stop_sending_sent
    }

    /// STOP_SENDING を受信済みかどうかを返す
    #[must_use]
    pub const fn stop_sending_received(&self) -> bool {
        self.stop_sending_received
    }

    /// STOP_SENDING 送信済みフラグを設定する
    pub fn set_stop_sending_sent(&mut self) {
        self.stop_sending_sent = true;
    }

    /// STOP_SENDING 受信済みフラグを設定する
    pub fn set_stop_sending_received(&mut self) {
        self.stop_sending_received = true;
    }

    /// データを受信したかどうかを返す
    #[must_use]
    pub const fn has_received_data(&self) -> bool {
        self.has_received_data
    }

    /// データを受信したフラグを設定する
    pub fn set_has_received_data(&mut self) {
        self.has_received_data = true;
    }
}
