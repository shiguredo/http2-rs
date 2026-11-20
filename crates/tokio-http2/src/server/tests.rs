//! TCP/TLS を用いた HTTP/2 サーバーの回帰テスト

use std::time::Duration;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{PrivateKeyDer, ServerName};
use shiguredo_http2::WindowSize;
use shiguredo_http2::webtransport::{WtConfig, WtEvent, WtSession};
use tokio::net::TcpSocket;
use tokio_rustls::TlsConnector;

use super::*;
use crate::{TlsClientConfig, WtServerRequest};

#[tokio::test]
async fn test_wt_commands_resume_partially_written_frame() {
    tokio::time::timeout(Duration::from_secs(10), async {
        // TCP と TLS のバッファを先行する WT データより小さくし、
        // クライアントが読み取りを再開するまでフレームを書き切れないようにする。
        const BUFFER_SIZE: u32 = 4096;
        let addr = SocketAddr::from(([127, 0, 0, 1], 0));
        let listener = TcpSocket::new_v4().expect("サーバーソケットの作成に失敗した");
        listener
            .set_send_buffer_size(BUFFER_SIZE)
            .expect("TCP 送信バッファの設定に失敗した");
        listener.bind(addr).expect("バインドに失敗した");
        let listener = listener.listen(1).expect("待ち受けに失敗した");
        let addr = listener.local_addr().expect("待ち受けアドレスがない");

        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_owned()])
                .expect("自己署名証明書の生成に失敗した");
        let key = PrivateKeyDer::try_from(signing_key.serialize_der())
            .expect("秘密鍵を DER 形式で読み取れなかった");
        let tls = TlsServerConfig::from_der(vec![cert.der().clone()], key)
            .expect("サーバー TLS の設定に失敗した");
        let server = Server {
            listener,
            tls_acceptor: TlsAcceptor::from(tls.inner()),
            local_addr: addr,
            limits: Limits::builder()
                .enable_connect_protocol(true)
                .wt_enabled(true)
                .build()
                .expect("サーバーの Limits の構築に失敗した"),
        };

        let peer = async {
            let socket = TcpSocket::new_v4().expect("クライアントソケットの作成に失敗した");
            socket
                .set_recv_buffer_size(BUFFER_SIZE)
                .expect("TCP 受信バッファの設定に失敗した");
            let tcp = socket.connect(addr).await.expect("TCP 接続に失敗した");
            let tls = TlsClientConfig::insecure().expect("クライアント TLS の設定に失敗した");
            let name = ServerName::try_from("localhost").expect("TLS サーバー名が無効");
            let io = TlsConnector::from(tls.inner())
                .connect(name, tcp)
                .await
                .expect("TLS 接続に失敗した");
            let limits = Limits::builder()
                .initial_window_size(WindowSize::from_static(0))
                .enable_connect_protocol(true)
                .wt_enabled(true)
                .webtransport(Some(65_536), None, None, None, Some(1), Some(65_536))
                .build()
                .expect("クライアントの Limits の構築に失敗した");
            let mut client = Connection::client(io, limits);
            client
                .send_preface()
                .await
                .expect("プリフェイス送信に失敗した");
            client
                .initiate()
                .await
                .expect("初期 SETTINGS の送信に失敗した");
            client
        };
        let (conn, mut client) = tokio::join!(server.accept(), peer);
        let mut conn = conn.expect("接続の受け入れに失敗した");
        conn.conn
            .get_mut()
            .get_mut()
            .1
            .set_buffer_limit(Some(BUFFER_SIZE as usize));

        let accept = async {
            loop {
                let event = conn
                    .next_event()
                    .await
                    .expect("サーバーイベントの取得に失敗した");
                if let Event::HeadersReceived {
                    stream_id, headers, ..
                } = event
                {
                    return WtServerRequest::from_connection(conn, stream_id, headers)
                        .accept(WtConfig::default(), None, None)
                        .await
                        .expect("セッションの受け入れに失敗した");
                }
            }
        };
        let connect = async {
            while !matches!(
                client
                    .next_event()
                    .await
                    .expect("SETTINGS の受信に失敗した"),
                Event::SettingsReceived { ack: false }
            ) {}
            let headers = vec![
                HeaderField::from_static(b":method", b"CONNECT"),
                HeaderField::from_static(b":scheme", b"https"),
                HeaderField::from_static(b":path", b"/"),
                HeaderField::from_static(b":authority", b"localhost"),
                HeaderField::from_static(b":protocol", b"webtransport"),
            ];
            let stream_id = client
                .send_request(headers, false)
                .await
                .expect("CONNECT の送信に失敗した");
            while !matches!(
                client.next_event().await.expect("CONNECT 応答の受信に失敗した"),
                Event::HeadersReceived { stream_id: id, .. } if id == stream_id
            ) {}
            stream_id
        };
        let (session, connect_stream) = tokio::join!(accept, connect);
        let parts = session.into_parts();
        let bidi = parts
            .handle
            .open_bidi()
            .await
            .expect("双方向ストリームを開けなかった");
        let payload = vec![0xAB; 48 * 1024];
        bidi.send(payload.clone(), false)
            .await
            .expect("先行データをキューに積めなかった");

        // ウィンドウを開放して先行データの送信を開始させる。TCP の peek は
        // バッファを消費しないため、ドライバはフラッシュの途中で待機し続ける。
        client
            .send_window_update(connect_stream, 65_535)
            .await
            .expect("WINDOW_UPDATE の送信に失敗した");
        let (tcp, _) = client.get_ref().get_ref();
        assert_eq!(
            tcp.peek(&mut [0]).await.expect("TCP の受信確認に失敗した"),
            1
        );

        // 出力を伴わないコマンドが読み取り再開前に完了すれば、select! の
        // コマンド分岐が書き込み待ちの next_event をキャンセルしたことが分かる。
        parts
            .handle
            .export_keying_material(b"test", b"", 16)
            .await
            .expect("書き込み待ちのドライバがコマンドを処理しなかった");

        let tail = b"tail";
        let commands = async {
            // send を先にキューへ積み、応答を待っている間に close もキューへ積む。
            let (send, close) = tokio::join!(
                biased;
                bidi.send(tail.to_vec(), true),
                parts.handle.close(0, "done"),
            );
            send.expect("後続データの送信に失敗した");
            close.expect("セッションの close に失敗した");
        };
        let receive = async {
            let mut wt = WtSession::client(WtConfig::default(), WtConfig::default());
            wt.initiate()
                .expect("クライアントの WT セッションの初期化に失敗した");
            let mut received = Vec::new();
            let mut finished = false;
            let mut closed = false;
            loop {
                let event = client
                    .next_event()
                    .await
                    .expect("HTTP/2 フレームの受信に失敗した");
                if let Event::DataReceived {
                    stream_id,
                    data,
                    end_stream,
                } = event
                {
                    assert_eq!(stream_id, connect_stream);
                    wt.feed(&data).expect("WT データの入力に失敗した");
                    wt.process().expect("WT capsule の処理に失敗した");
                    while let Some(event) = wt.poll_event() {
                        match event {
                            WtEvent::StreamData { data, fin, .. } => {
                                received.extend_from_slice(&data);
                                finished |= fin;
                            }
                            WtEvent::SessionClosed {
                                error_code: 0,
                                reason,
                            } if reason == "done" => {
                                closed = true;
                            }
                            _ => {}
                        }
                    }
                    if end_stream {
                        break;
                    }
                }
            }
            assert_eq!(
                received,
                [payload.as_slice(), tail].concat(),
                "WT データの欠落または重複"
            );
            assert!(finished, "WT ストリームの FIN が届いていない");
            assert!(closed, "WT_CLOSE_SESSION が届いていない");
        };
        tokio::join!(biased; commands, receive);
        parts
            .driver
            .await
            .expect("ドライバタスクの join に失敗した")
            .expect("ドライバが異常終了した");
    })
    .await
    .expect("バックプレッシャー中の送受信がタイムアウトした");
}
