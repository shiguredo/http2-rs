//! TLS サーバー設定
//!
//! 自己署名の ECDSA P-256 証明書を生成 (もしくはキャッシュから読み込み) し、
//! `tokio_http2::TlsServerConfig` を返す。ALPN は `TlsServerConfig::from_der` 内で
//! `h2` に設定される。
//!
//! # 証明書キャッシュ
//!
//! `/tmp/wt-server-http2-cert.jsonc` に `created_at` / `cert` / `key` を JSONC 形式で
//! 保存する。残り有効期間が `MIN_REMAINING_HOURS` 以上であれば再利用し、
//! そうでなければ新規生成してキャッシュを更新する。
//! Chrome の `serverCertificateHashes` で接続する際に、再起動しても同じ SHA-256
//! ハッシュを使える運用を想定している。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use base64::Engine;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use tokio_http2::TlsServerConfig;

use crate::error::Error;

/// WebTransport の serverCertificateHashes で要求される最大有効期間 (14 日未満)
const CERT_VALIDITY_DAYS: i64 = 13;

/// キャッシュ済み証明書を再利用するために必要な最低残り有効期間
const MIN_REMAINING_HOURS: i64 = 1;

/// キャッシュファイル名
const CACHE_FILENAME: &str = "wt-server-http2-cert.jsonc";

fn cache_path() -> PathBuf {
    std::env::temp_dir().join(CACHE_FILENAME)
}

/// キャッシュ済み証明書と秘密鍵を読み込む
///
/// JSONC ファイルが存在し、残り有効期間が `MIN_REMAINING_HOURS` 以上であればそのまま返す。
/// 存在しないか期限切れの場合は `None` を返す。
fn load_cached_cert() -> Option<(CertificateDer<'static>, Vec<u8>)> {
    let path = cache_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let (json, _) = nojson::RawJson::parse_jsonc(&text).ok()?;
    let root = json.value();

    let created_at: i64 = root
        .to_member("created_at")
        .ok()?
        .required()
        .ok()?
        .try_into()
        .ok()?;
    let cert_b64: String = root
        .to_member("cert")
        .ok()?
        .required()
        .ok()?
        .try_into()
        .ok()?;
    let key_b64: String = root
        .to_member("key")
        .ok()?
        .required()
        .ok()?
        .try_into()
        .ok()?;

    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let expires_at = created_at + CERT_VALIDITY_DAYS * 24 * 3600;
    let remaining_secs = expires_at - now;

    if remaining_secs < MIN_REMAINING_HOURS * 3600 {
        log::info!("cached certificate expires soon, regenerating");
        return None;
    }

    let cert_bytes = base64::engine::general_purpose::STANDARD
        .decode(&cert_b64)
        .ok()?;
    let key_bytes = base64::engine::general_purpose::STANDARD
        .decode(&key_b64)
        .ok()?;

    log::info!(
        "using cached certificate {:?} (expires in {:.1} hours)",
        path,
        remaining_secs as f64 / 3600.0
    );

    Some((CertificateDer::from(cert_bytes), key_bytes))
}

/// 新しい自己署名証明書を生成してキャッシュに保存する
fn generate_and_cache_cert() -> Result<(CertificateDer<'static>, Vec<u8>), Error> {
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

    let cert_der_ref = cert.der();
    let key_bytes = signing_key.serialize_der();

    let cert_b64 = base64::engine::general_purpose::STANDARD.encode(cert_der_ref.as_ref());
    let key_b64 = base64::engine::general_purpose::STANDARD.encode(&key_bytes);
    let created_at = now.unix_timestamp();

    let jsonc = format!(
        "// WebTransport over HTTP/2 server の自己署名証明書キャッシュ (自動生成)\n{}\n",
        nojson::json(|f| {
            f.set_indent_size(2);
            f.set_spacing(true);
            f.object(|f| {
                f.member("created_at", created_at)?;
                f.member("cert", &cert_b64)?;
                f.member("key", &key_b64)
            })
        })
    );

    let path = cache_path();
    std::fs::write(&path, &jsonc)
        .map_err(|e| Error::Tls(format!("failed to write cert cache: {e}")))?;

    log::info!("generated new certificate (cached to {:?})", path);

    let cert_owned = CertificateDer::from(cert_der_ref.as_ref().to_vec());
    Ok((cert_owned, key_bytes))
}

/// 自己署名証明書を取得して TLS 設定を構築する
///
/// キャッシュ済み証明書が有効であればそれを使い、なければ新規生成する。
/// 起動時に SHA-256 ハッシュを base64 で `log::info!` に出力する
/// (Chrome の `serverCertificateHashes` 設定用)。
pub fn generate_tls_server() -> Result<TlsServerConfig, Error> {
    let (cert_der, key_bytes) = match load_cached_cert() {
        Some(cached) => cached,
        None => generate_and_cache_cert()?,
    };

    let hash = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, cert_der.as_ref());
    let hash_b64 = base64::engine::general_purpose::STANDARD.encode(hash.as_ref());
    log::info!("Certificate SHA-256 (base64): {hash_b64}");

    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_bytes));

    TlsServerConfig::from_der(vec![cert_der], key_der)
        .map_err(|e| Error::Tls(format!("TlsServerConfig: {e}")))
}
