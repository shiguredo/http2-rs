//! TLS 設定

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use rustls::{ClientConfig, RootCertStore, ServerConfig};

use crate::error::{Error, Result};

/// TLS クライアント設定
#[derive(Clone)]
pub struct TlsClientConfig {
    inner: Arc<ClientConfig>,
}

impl TlsClientConfig {
    /// プラットフォームの証明書検証器を使用して設定を作成
    pub fn with_platform_verifier() -> Result<Self> {
        let provider = rustls::crypto::aws_lc_rs::default_provider();
        let verifier = rustls_platform_verifier::Verifier::new(Arc::new(provider))
            .map_err(|e| Error::Tls(Box::new(e)))?;

        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();

        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// カスタム CA 証明書を使用して設定を作成
    pub fn with_custom_ca(ca_cert_pem: &[u8]) -> Result<Self> {
        let mut root_store = RootCertStore::empty();

        let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(ca_cert_pem)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Tls(Box::new(e)))?;

        if certs.is_empty() {
            return Err(Error::Tls("no CA certificates found".into()));
        }

        for cert in certs {
            root_store.add(cert).map_err(|e| Error::Tls(Box::new(e)))?;
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// 証明書検証を無効化した設定を作成（テスト用）
    pub fn insecure() -> Result<Self> {
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
            .with_no_client_auth();

        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// 内部の ClientConfig を取得
    pub(crate) fn inner(&self) -> Arc<ClientConfig> {
        Arc::clone(&self.inner)
    }
}

/// TLS サーバー設定
#[derive(Clone)]
pub struct TlsServerConfig {
    inner: Arc<ServerConfig>,
}

impl TlsServerConfig {
    /// 証明書と秘密鍵から設定を作成
    pub fn new(cert_pem: &[u8], key_pem: &[u8]) -> Result<Self> {
        let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_pem)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Tls(Box::new(e)))?;

        if certs.is_empty() {
            return Err(Error::Tls("no certificates found".into()));
        }

        let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|e| Error::Tls(Box::new(e)))?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| Error::Tls(Box::new(e)))?;

        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// DER 形式の証明書と秘密鍵から設定を作成
    pub fn from_der(
        certs: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self> {
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| Error::Tls(Box::new(e)))?;

        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// 内部の ServerConfig を取得
    pub(crate) fn inner(&self) -> Arc<ServerConfig> {
        Arc::clone(&self.inner)
    }
}

/// 証明書検証を行わない検証器（テスト用）
#[derive(Debug)]
struct InsecureVerifier;

impl rustls::client::danger::ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
