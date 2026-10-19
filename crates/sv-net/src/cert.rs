//! Identidade de cada membro: um certificado autoassinado por sessão, conferido pela fingerprint
//! (SHA-256) que viaja pela sinalização. Sem autoridade certificadora: só conectamos com quem a
//! sala apresentou.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex};

use rustls::DigitallySignedStruct;
use rustls::SignatureScheme;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DistinguishedName, Error};
use sha2::{Digest, Sha256};

/// Certificado + chave desta sessão.
pub struct Identity {
    pub cert: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    pub fingerprint: String,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl Identity {
    pub fn generate() -> Self {
        let generated =
            rcgen::generate_simple_self_signed(vec!["squadview".to_owned()]).expect("gerar certificado autoassinado");
        let cert = generated.cert.der().clone();
        let key = PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der());
        let fingerprint = fingerprint(&cert);
        Self { cert, key, fingerprint }
    }

    pub fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(self.key.clone_key())
    }
}

/// SHA-256 do certificado em hexadecimal (64 caracteres).
pub fn fingerprint(cert: &CertificateDer<'_>) -> String {
    Sha256::digest(cert.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Do lado de quem conecta: aceita só o certificado com a fingerprint combinada.
#[derive(Debug)]
pub(crate) struct ExpectFingerprint {
    expected: String,
    provider: Arc<CryptoProvider>,
}

impl ExpectFingerprint {
    pub fn new(expected: String) -> Arc<Self> {
        Arc::new(Self {
            expected,
            provider: provider(),
        })
    }
}

impl ServerCertVerifier for ExpectFingerprint {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if fingerprint(end_entity) == self.expected {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General("certificado não é o do membro da sala".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Fingerprints autorizadas a conectar em nós (quem pediu para assistir pela sinalização).
#[derive(Debug, Default, Clone)]
pub struct Allowed(Arc<Mutex<HashSet<String>>>);

impl Allowed {
    pub fn allow(&self, fingerprint: &str) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(fingerprint.to_owned());
    }

    pub fn revoke(&self, fingerprint: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(fingerprint);
    }

    pub fn contains(&self, fingerprint: &str) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).contains(fingerprint)
    }
}

/// Do lado de quem recebe conexões: exige certificado do outro e só aceita as fingerprints
/// autorizadas.
#[derive(Debug)]
pub(crate) struct AllowedClients {
    allowed: Allowed,
    provider: Arc<CryptoProvider>,
}

impl AllowedClients {
    pub fn new(allowed: Allowed) -> Arc<Self> {
        Arc::new(Self {
            allowed,
            provider: provider(),
        })
    }
}

impl ClientCertVerifier for AllowedClients {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        if self.allowed.contains(&fingerprint(end_entity)) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(Error::General("membro não autorizado a assistir".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_sessao_tem_uma_identidade_diferente() {
        let a = Identity::generate();
        let b = Identity::generate();
        assert_eq!(a.fingerprint.len(), 64);
        assert_ne!(a.fingerprint, b.fingerprint);
        assert_eq!(fingerprint(&a.cert), a.fingerprint);
    }

    #[test]
    fn autorizacao_pode_ser_dada_e_retirada() {
        let allowed = Allowed::default();
        allowed.allow("abc");
        assert!(allowed.contains("abc"));
        allowed.revoke("abc");
        assert!(!allowed.contains("abc"));
    }
}
