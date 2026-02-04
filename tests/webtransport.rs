//! WebTransport の単体テスト

use shiguredo_http2::webtransport::{Capsule, CapsuleDecoder, CapsuleEncoder};

/// WT_DRAIN_SESSION はペイロードなしなので固定値テスト
#[test]
fn test_capsule_wt_drain_session_roundtrip() {
    let mut encoder = CapsuleEncoder::new();
    let mut decoder = CapsuleDecoder::new();

    let capsule = Capsule::WtDrainSession;
    encoder.encode(&capsule);

    decoder.feed(encoder.buffer());
    let decoded = decoder.decode().unwrap().unwrap();
    assert_eq!(capsule, decoded);
}
