//! TLS for the phone link: the desktop's self-signed certificate (created
//! once, kept in the data dir) and the phone's verifier, which accepts only
//! the certificate whose SHA-256 fingerprint came from the pairing link.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;

/// Name in the certificate and the SNI the phone sends. Only the pinned
/// fingerprint is checked, so it never has to match an address.
pub(crate) const SERVER_NAME: &str = "zcode-gpui.local";

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

pub(crate) fn fingerprint(cert: &[u8]) -> String {
    Sha256::digest(cert)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) struct DesktopIdentity {
    pub cert: CertificateDer<'static>,
    key: Vec<u8>,
    pub fingerprint: String,
}

impl DesktopIdentity {
    /// Load `cert.der`/`key.der` from `dir`, creating them on first use.
    pub(crate) fn load_or_create(dir: &Path) -> Result<Self, String> {
        let cert_path = dir.join("phone-remote-cert.der");
        let key_path = dir.join("phone-remote-key.der");
        if let (Ok(cert), Ok(key)) = (std::fs::read(&cert_path), std::fs::read(&key_path)) {
            return Ok(Self::from_der(cert, key));
        }
        let generated = rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_string()])
            .map_err(|e| format!("certificate generation failed: {e}"))?;
        let cert = generated.cert.der().to_vec();
        let key = generated.signing_key.serialize_der();
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(&key_path, &key).map_err(|e| e.to_string())?;
        std::fs::write(&cert_path, &cert).map_err(|e| e.to_string())?;
        Ok(Self::from_der(cert, key))
    }

    fn from_der(cert: Vec<u8>, key: Vec<u8>) -> Self {
        let fingerprint = fingerprint(&cert);
        Self {
            cert: CertificateDer::from(cert),
            key,
            fingerprint,
        }
    }

    pub(crate) fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, String> {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()));
        let config = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(vec![self.cert.clone()], key)
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(config))
    }
}

#[derive(Debug)]
struct PinnedFingerprint {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedFingerprint {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let actual = fingerprint(end_entity);
        if crate::remote::pairing::constant_time_eq(actual.as_bytes(), self.fingerprint.as_bytes())
        {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "desktop certificate does not match the paired fingerprint".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub(crate) fn client_config(fingerprint: &str) -> Result<Arc<rustls::ClientConfig>, String> {
    let provider = provider();
    let verifier = Arc::new(PinnedFingerprint {
        fingerprint: fingerprint.to_ascii_lowercase(),
        provider: provider.clone(),
    });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

pub(crate) fn server_name() -> ServerName<'static> {
    ServerName::try_from(SERVER_NAME).expect("static server name")
}
