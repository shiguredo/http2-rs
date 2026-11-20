//! HTTP/2 コネクションの非同期 I/O のテスト

use std::future::ready;
use std::time::Duration;

use tokio::io::{AsyncReadExt, BufStream, duplex};
use tokio_http2::{Connection, Limits};

#[tokio::test]
async fn test_flush_resumes_with_only_transport_output_pending() {
    let limits = Limits::default();
    let mut expected_connection = shiguredo_http2::Connection::server(limits.clone());
    expected_connection
        .send_settings()
        .expect("期待する SETTINGS を生成できなかった");
    let expected = expected_connection
        .poll_output()
        .expect("SETTINGS 出力がない");

    // BufStream は SETTINGS 全体を一度の write で受理できるが、
    // 対向が読み始めるまで下位の duplex に最後の 1 バイトを書き込めない。
    let (io, mut peer) = duplex(expected.len() - 1);
    let io = BufStream::with_capacity(16, expected.len() + 1, io);
    let mut server = Connection::server(io, limits);
    tokio::select! {
        biased;
        result = server.initiate() => panic!("送信先が満杯になる前にフラッシュが完了した: {result:?}"),
        () = ready(()) => {}
    }

    // 新しい HTTP/2 出力を生成せず、下位のフラッシュ待ちだけを再開する。
    let mut received = vec![0; expected.len()];
    let (send, read) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server.flush(), peer.read_exact(&mut received))
    })
    .await
    .expect("保留中の SETTINGS の送受信がタイムアウトした");
    send.expect("SETTINGS のフラッシュを再開できなかった");
    read.expect("SETTINGS 全体を受信できなかった");
    assert_eq!(received, expected, "SETTINGS が欠落または重複した");
}
