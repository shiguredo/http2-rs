//! tokio-nghttp2 クライアントと tokio-http2 サーバーの疎通確認
//!
//! nghttp2 C ライブラリ側から本実装側へ接続し、GET を 1 往復できることを確認する。

use interop_h2::{RESPONSE_BODY, get_with_nghttp2_client, serve_get_with_http2_server};

/// nghttp2 実装のクライアントから本実装のサーバーへ GET を 1 往復できること
#[tokio::test]
async fn test_nghttp2_client_reaches_http2_server() {
    // 本実装 (tokio-http2) のサーバーを起動する
    let (addr, server) = serve_get_with_http2_server().await;

    // nghttp2 実装 (tokio-nghttp2) のクライアントで GET を送信する
    let response = get_with_nghttp2_client(addr).await;

    // 接続・ステータス・本文の 3 点を確認する
    assert_eq!(response.status, b"200", "ステータスコードが 200 でない");
    assert_eq!(response.body, RESPONSE_BODY, "レスポンスボディが一致しない");

    // 応答処理タスクの失敗をテストの失敗として伝播させる
    server.await.expect("the server task failed");
}
