//! フロー制御 (RFC 9113 Section 5.2)
//!
//! HTTP/2 のフロー制御ウィンドウを管理する。

use crate::error::{Error, ErrorCode};
use crate::settings::DEFAULT_INITIAL_WINDOW_SIZE;

/// 最大ウィンドウサイズ (2^31 - 1)
pub const MAX_WINDOW_SIZE: u32 = 2_147_483_647;

/// フロー制御ウィンドウ
#[derive(Debug, Clone)]
pub struct FlowControl {
    /// 送信ウィンドウサイズ（ピアが許可している送信可能バイト数）
    send_window: i64,
    /// 受信ウィンドウサイズ（ピアに許可している受信可能バイト数）
    recv_window: i64,
    /// 送信側の初期ウィンドウサイズ（リモートの SETTINGS_INITIAL_WINDOW_SIZE）
    send_initial: u32,
    /// 受信側の初期ウィンドウサイズ（ローカルの SETTINGS_INITIAL_WINDOW_SIZE）
    recv_initial: u32,
}

impl FlowControl {
    /// 新しいフロー制御を生成する
    #[must_use]
    pub fn new(initial_window_size: u32) -> Self {
        Self {
            send_window: i64::from(initial_window_size),
            recv_window: i64::from(initial_window_size),
            send_initial: initial_window_size,
            recv_initial: initial_window_size,
        }
    }

    /// 送信/受信ウィンドウを個別に設定して生成する
    ///
    /// RFC 9113 Section 5.2: ストリームのフロー制御において、
    /// 送信ウィンドウはリモートの initial_window_size、
    /// 受信ウィンドウはローカルの initial_window_size で初期化する。
    #[must_use]
    pub fn with_separate_windows(send_initial: u32, recv_initial: u32) -> Self {
        Self {
            send_window: i64::from(send_initial),
            recv_window: i64::from(recv_initial),
            send_initial,
            recv_initial,
        }
    }

    /// 送信ウィンドウサイズを取得する
    #[must_use]
    pub const fn send_window(&self) -> i64 {
        self.send_window
    }

    /// 受信ウィンドウサイズを取得する
    #[must_use]
    pub const fn recv_window(&self) -> i64 {
        self.recv_window
    }

    /// 送信側の初期ウィンドウサイズを取得する
    #[must_use]
    pub const fn send_initial(&self) -> u32 {
        self.send_initial
    }

    /// 受信側の初期ウィンドウサイズを取得する
    #[must_use]
    pub const fn recv_initial(&self) -> u32 {
        self.recv_initial
    }

    /// 送信可能なバイト数を取得する
    #[must_use]
    pub fn send_available(&self) -> usize {
        if self.send_window > 0 {
            self.send_window as usize
        } else {
            0
        }
    }

    /// データ送信を記録する（送信ウィンドウを減らす）
    pub fn consume_send(&mut self, size: usize) -> Result<(), Error> {
        let size = i64::try_from(size).map_err(|_| {
            Error::connection_error(ErrorCode::FlowControlError, "data size too large")
        })?;

        if size > self.send_window {
            return Err(Error::connection_error(
                ErrorCode::FlowControlError,
                "send window exhausted",
            ));
        }

        self.send_window -= size;
        Ok(())
    }

    /// データ受信を記録する（受信ウィンドウを減らす）
    pub fn consume_recv(&mut self, size: usize) -> Result<(), Error> {
        let size = i64::try_from(size).map_err(|_| {
            Error::connection_error(ErrorCode::FlowControlError, "data size too large")
        })?;

        if size > self.recv_window {
            return Err(Error::connection_error(
                ErrorCode::FlowControlError,
                "recv window exceeded",
            ));
        }

        self.recv_window -= size;
        Ok(())
    }

    /// WINDOW_UPDATE 受信時に送信ウィンドウを増やす
    pub fn recv_window_update(&mut self, increment: u32) -> Result<(), Error> {
        if increment == 0 {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "WINDOW_UPDATE with zero increment",
            ));
        }

        let new_window = self.send_window + i64::from(increment);
        if new_window > i64::from(MAX_WINDOW_SIZE) {
            return Err(Error::connection_error(
                ErrorCode::FlowControlError,
                "window size overflow",
            ));
        }

        self.send_window = new_window;
        Ok(())
    }

    /// ローカルで受信ウィンドウを増やす（WINDOW_UPDATE を送信する前に呼び出す）
    pub fn add_recv_window(&mut self, increment: u32) -> Result<(), Error> {
        if increment == 0 {
            return Err(Error::connection_error(
                ErrorCode::ProtocolError,
                "window increment must be non-zero",
            ));
        }

        let new_window = self.recv_window + i64::from(increment);
        if new_window > i64::from(MAX_WINDOW_SIZE) {
            return Err(Error::connection_error(
                ErrorCode::FlowControlError,
                "window size overflow",
            ));
        }

        self.recv_window = new_window;
        Ok(())
    }

    /// SETTINGS_INITIAL_WINDOW_SIZE 変更時に送信ウィンドウを調整する
    ///
    /// RFC 9113 Section 6.9.2: SETTINGS_INITIAL_WINDOW_SIZE 変更時に調整対象となるのは
    /// 送信ウィンドウのみであり、受信側の初期値には影響しない。
    /// 将来の RFC 改訂で変更される可能性がある。
    pub fn update_initial_window_size(&mut self, new_size: u32) -> Result<(), Error> {
        let delta = i64::from(new_size) - i64::from(self.send_initial);
        let new_window = self.send_window + delta;

        if new_window > i64::from(MAX_WINDOW_SIZE) {
            return Err(Error::connection_error(
                ErrorCode::FlowControlError,
                "window size overflow after SETTINGS update",
            ));
        }

        self.send_window = new_window;
        self.send_initial = new_size;
        Ok(())
    }

    /// 受信ウィンドウが低下しているかどうかを返す
    ///
    /// WINDOW_UPDATE を送信するタイミングの判断に使用する。
    #[must_use]
    pub fn should_send_window_update(&self) -> bool {
        // 受信側初期ウィンドウサイズの半分以下になったら更新を推奨
        self.recv_window < i64::from(self.recv_initial / 2)
    }

    /// WINDOW_UPDATE で送信すべき増分を計算する
    #[must_use]
    pub fn window_update_increment(&self) -> u32 {
        let target = i64::from(self.recv_initial);
        let increment = target - self.recv_window;
        if increment > 0 && increment <= i64::from(MAX_WINDOW_SIZE) {
            increment as u32
        } else {
            0
        }
    }
}

impl Default for FlowControl {
    fn default() -> Self {
        Self::new(DEFAULT_INITIAL_WINDOW_SIZE)
    }
}
