//! TLS サーバー設定
//!
//! 自己署名の ECDSA P-256 証明書を生成して `tokio_http2::TlsServerConfig` を返す。
//! ALPN は `TlsServerConfig::from_der` 内で `h2` に設定される。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use base64::Engine;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use tokio_http2::TlsServerConfig;

use crate::error::Error;

/// WebTransport の serverCertificateHashes で要求される最大有効期間 (14 日未満)
///
/// HTTP/2 WebTransport では必須ではないが、Chrome/Safari 互換のため合わせる。
const CERT_VALIDITY_DAYS: i64 = 13;

/// 自己署名証明書を生成し、SHA-256 ハッシュをログ出力したうえで TLS 設定を返す
pub fn generate_tls_server() -> Result<TlsServerConfig, Error> {
    let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .map_err(|e| Error::Tls(format!("certificate params: {e}")))?;
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)));

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(CERT_VALIDITY_DAYS);

    let signing_key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
        .map_err(|e| Error::Tls(format!("key generation: {e}")))?;
    let cert = params
        .self_signed(&signing_key)
        .map_err(|e| Error::Tls(format!("self-signed cert: {e}")))?;

    let cert_der_owned = cert.der().clone();
    let key_der_bytes = signing_key.serialize_der();

    // SHA-256 ハッシュを base64 で出力する (serverCertificateHashes 用)
    let hash = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, cert_der_owned.as_ref());
    let hash_b64 = base64::engine::general_purpose::STANDARD.encode(hash.as_ref());
    log::info!("Certificate SHA-256 (base64): {hash_b64}");

    let cert_der = CertificateDer::from(cert_der_owned.as_ref().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der_bytes));

    TlsServerConfig::from_der(vec![cert_der], key_der)
        .map_err(|e| Error::Tls(format!("TlsServerConfig: {e}")))
}
