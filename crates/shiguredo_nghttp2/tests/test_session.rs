use shiguredo_nghttp2::{FrameType, Header, Http2Event, Session};

use std::pin::Pin;

/// クライアントセッションを生成し、呼び出し元へ move して返す
///
/// コンストラクタの戻り値 (`Pin<Box<Session>>`) が関数境界を越えて
/// move されることを明示するためのヘルパー。
fn relocated_client_session() -> Pin<Box<Session>> {
    Session::client().expect("クライアントセッションが生成できること")
}

/// recv() を経由せず DATA 付き submit_request → send() を呼んでも
/// data_source_read_callback で NGHTTP2_ERR_CALLBACK_FAILURE にならないこと
#[test]
fn test_send_before_recv_with_data_provider_succeeds() {
    let mut session = Session::client().expect("クライアントセッションが生成できること");
    let headers = vec![
        Header::method("POST"),
        Header::scheme("https"),
        Header::authority("example.com"),
        Header::path("/"),
    ];
    let stream_id = session
        .submit_request(&headers, Some(b"hello"), true)
        .expect("submit_request が成功すること");
    assert!(stream_id > 0, "クライアント開始ストリーム ID は正の値");

    // send() が CALLBACK_FAILURE を返さず、出力を生成できること
    let output = session
        .send()
        .expect("send が CALLBACK_FAILURE を返さないこと");
    assert!(!output.is_empty(), "送信バイト列が空でないこと");

    // DATA フレームの FrameSent イベントが正しく流れること
    let mut saw_data_frame = false;
    while let Some(event) = session.poll_event() {
        if matches!(
            event,
            Http2Event::FrameSent {
                frame_type: FrameType::Data,
                ..
            }
        ) {
            saw_data_frame = true;
        }
    }
    assert!(
        saw_data_frame,
        "DATA フレームの FrameSent イベントが取得できること"
    );
}

/// recv() を経由せず submit_request(headers, None, false) → send() (deferred) →
/// submit_data → send() を呼んでも data_source_read_callback で
/// NGHTTP2_ERR_CALLBACK_FAILURE にならないこと
#[test]
fn test_send_with_submit_data_succeeds() {
    let mut session = Session::client().expect("クライアントセッションが生成できること");
    let headers = vec![
        Header::method("POST"),
        Header::scheme("https"),
        Header::authority("example.com"),
        Header::path("/"),
    ];
    let stream_id = session
        .submit_request(&headers, None, false)
        .expect("submit_request が成功すること");
    assert!(stream_id > 0, "クライアント開始ストリーム ID は正の値");

    // data provider を deferred 状態にするため、空バッファのまま 1 度 send() を呼ぶ
    let output = session
        .send()
        .expect("send が CALLBACK_FAILURE を返さないこと");
    assert!(!output.is_empty(), "送信バイト列が空でないこと");

    // deferred 状態になった data provider にデータを追加して再開する
    session
        .submit_data(stream_id, b"hello", true)
        .expect("submit_data が成功すること");

    // 2 度目の send() も CALLBACK_FAILURE を返さず、出力を生成できること
    let output = session
        .send()
        .expect("send が CALLBACK_FAILURE を返さないこと");
    assert!(!output.is_empty(), "送信バイト列が空でないこと");

    // DATA フレームの FrameSent イベントが正しく流れること
    let mut saw_data_frame = false;
    while let Some(event) = session.poll_event() {
        if matches!(
            event,
            Http2Event::FrameSent {
                frame_type: FrameType::Data,
                ..
            }
        ) {
            saw_data_frame = true;
        }
    }
    assert!(
        saw_data_frame,
        "DATA フレームの FrameSent イベントが取得できること"
    );
}

/// recv() を経由せず submit_settings → send() を呼んだ場合でも
/// FrameSent イベントが取得できること
#[test]
fn test_send_settings_emits_frame_sent_event() {
    let mut session = Session::client().expect("クライアントセッションが生成できること");
    session
        .submit_settings(&[])
        .expect("submit_settings が成功すること");

    let output = session.send().expect("send が成功すること");
    assert!(!output.is_empty(), "送信バイト列が空でないこと");

    let mut saw_settings_frame = false;
    while let Some(event) = session.poll_event() {
        if matches!(
            event,
            Http2Event::FrameSent {
                stream_id: 0,
                frame_type: FrameType::Settings,
            }
        ) {
            saw_settings_frame = true;
        }
    }
    assert!(
        saw_settings_frame,
        "SETTINGS フレームの FrameSent イベントが取得できること"
    );
}

/// コンストラクタ内で user_data が登録され、`Pin<Box<Session>>` を別スコープへ
/// move しても data_source_read_callback が正しい Session を参照できること。
///
/// SETTINGS のみの送信では data_source_read_callback が発火しないため、
/// DATA 付きの送信で data_source_read_callback が発火する経路を検証する。
#[test]
fn test_user_data_registered_once_in_constructor_survives_move() {
    let mut session = relocated_client_session();
    let headers = vec![
        Header::method("POST"),
        Header::scheme("https"),
        Header::authority("example.com"),
        Header::path("/"),
    ];
    let stream_id = session
        .submit_request(&headers, Some(b"hello"), true)
        .expect("submit_request が成功すること");
    assert!(stream_id > 0, "クライアント開始ストリーム ID は正の値");

    let output = session
        .send()
        .expect("send が CALLBACK_FAILURE を返さないこと");
    assert!(!output.is_empty(), "送信バイト列が空でないこと");

    // DATA フレームの FrameSent イベントが正しく流れること
    let mut saw_data_frame = false;
    while let Some(event) = session.poll_event() {
        if matches!(
            event,
            Http2Event::FrameSent {
                frame_type: FrameType::Data,
                ..
            }
        ) {
            saw_data_frame = true;
        }
    }
    assert!(
        saw_data_frame,
        "DATA フレームの FrameSent イベントが取得できること"
    );
}
