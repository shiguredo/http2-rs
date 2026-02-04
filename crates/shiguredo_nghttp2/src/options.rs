//! nghttp2 セッションオプション

use crate::error::{Result, check_nghttp2};

/// セッションオプション
///
/// Builder パターンで構築し、`Session::client_with_options()` /
/// `Session::server_with_options()` に渡す。
pub struct SessionOptions {
    option: *mut nghttp2_sys::nghttp2_option,
}

impl SessionOptions {
    /// 新しいセッションオプションを作成
    pub fn new() -> Result<Self> {
        let mut option: *mut nghttp2_sys::nghttp2_option = std::ptr::null_mut();
        unsafe {
            check_nghttp2(nghttp2_sys::nghttp2_option_new(&mut option))?;
        }
        Ok(Self { option })
    }

    /// 自動 WINDOW_UPDATE を無効化
    ///
    /// 有効にすると、nghttp2 はデータ受信時に自動で WINDOW_UPDATE を送信しない。
    /// `consume()` / `consume_connection()` / `consume_stream()` で手動管理する。
    pub fn no_auto_window_update(self, val: bool) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_no_auto_window_update(self.option, val as libc::c_int);
        }
        self
    }

    /// ピアの最大同時ストリーム数の初期値を設定
    ///
    /// ピアから SETTINGS を受信する前に適用される値。
    pub fn peer_max_concurrent_streams(self, val: u32) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_peer_max_concurrent_streams(self.option, val);
        }
        self
    }

    /// 自動 PING ACK を無効化
    ///
    /// 有効にすると、PING フレーム受信時に自動で ACK を送信しない。
    pub fn no_auto_ping_ack(self, val: bool) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_no_auto_ping_ack(self.option, val as libc::c_int);
        }
        self
    }

    /// 送信ヘッダーブロックの最大長を設定
    pub fn max_send_header_block_length(self, val: usize) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_max_send_header_block_length(self.option, val);
        }
        self
    }

    /// deflate 動的テーブルの最大サイズを設定
    pub fn max_deflate_dynamic_table_size(self, val: usize) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_max_deflate_dynamic_table_size(self.option, val);
        }
        self
    }

    /// 送信 ACK の最大数を設定 (DoS 対策)
    pub fn max_outbound_ack(self, val: usize) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_max_outbound_ack(self.option, val);
        }
        self
    }

    /// 受信 SETTINGS パラメータの最大数を設定 (DoS 対策)
    pub fn max_settings(self, val: usize) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_max_settings(self.option, val);
        }
        self
    }

    /// ストリームリセットのレート制限を設定 (DoS 対策)
    ///
    /// `burst`: バースト許容数、`rate`: 1 秒あたりの許容数
    pub fn stream_reset_rate_limit(self, burst: u64, rate: u64) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_stream_reset_rate_limit(self.option, burst, rate);
        }
        self
    }

    /// CONTINUATION フレームの最大数を設定 (DoS 対策)
    pub fn max_continuations(self, val: usize) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_max_continuations(self.option, val);
        }
        self
    }

    /// グリッチ (プロトコル違反) のレート制限を設定 (DoS 対策)
    ///
    /// `burst`: バースト許容数、`rate`: 1 秒あたりの許容数
    pub fn glitch_rate_limit(self, burst: u64, rate: u64) -> Self {
        unsafe {
            nghttp2_sys::nghttp2_option_set_glitch_rate_limit(self.option, burst, rate);
        }
        self
    }

    /// 内部の nghttp2_option ポインタを取得
    pub(crate) fn as_ptr(&self) -> *const nghttp2_sys::nghttp2_option {
        self.option
    }
}

// SAFETY: nghttp2_option はスレッド間の共有状態を持たず、
// 構築後は Session 作成時に読み取り専用で参照されるのみ。
// Drop 時の nghttp2_option_del も所有権に基づく安全な操作である。
unsafe impl Send for SessionOptions {}
unsafe impl Sync for SessionOptions {}

impl Drop for SessionOptions {
    fn drop(&mut self) {
        unsafe {
            nghttp2_sys::nghttp2_option_del(self.option);
        }
    }
}
