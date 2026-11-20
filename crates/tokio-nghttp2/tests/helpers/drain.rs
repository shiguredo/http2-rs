//! テストで応答を送信した後の接続クローズを同期する

use std::time::Duration;

use tokio_nghttp2::ServerConnection;

/// 待機中の出力を送信しながら、ピアが接続を閉じるまでイベントを読む。
/// 未読データを残して接続を閉じると、OS が RST を送り、送信した応答が失われることがある。
/// ピアが接続を閉じないときにテストが停止しないよう、待機全体に上限を設ける。
pub(crate) async fn drain_connection(conn: &mut ServerConnection) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while let Ok(Ok(_)) = tokio::time::timeout_at(deadline, conn.next_event()).await {}
}
