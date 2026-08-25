use anyhow::{Context, Result, anyhow};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs::File;
use std::io;
use std::io::{BufReader, ErrorKind};
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

/// TLS configuration for SMTP server, for StartTLS support.
#[derive(Clone)]
pub enum TlsConfig
{
    /// define TLS using certificate chain.
    /// suitable for self-signed or trusted cert.
    /// certificates: certificate chain, starting with your cert then intermediary certificates.
    /// private_key: private key file.
    ///
    /// generate with:
    /// openssl req -newkey rsa:2048 -x509 -sha256 -days 3650 -nodes -subj "/CN=localhost" -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" -out mycert.crt -keyout mycert.key
    Chain {
        certificates: Vec<String>,
        private_key: String,
    },

    /// define TLS using manually initialized TLS acceptor instance.
    Custom(TlsAcceptor),
}

impl TlsConfig
{
    /// get the TLS acceptor instance from this config.
    pub(crate) fn into_tls_acceptor(self) -> Result<TlsAcceptor>
    {
        match self {
            TlsConfig::Custom(acceptor) =>
                Ok(acceptor),
            TlsConfig::Chain { certificates, private_key } => {
                let mut certs = Vec::new();
                for cert in certificates
                {
                    certs.extend(Self::read_certs(&cert)?)
                }

                let key = Self::read_key(&private_key)?;

                let config = ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(certs, key)
                    .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;

                Ok(TlsAcceptor::from(Arc::new(config)))
            }
        }
    }

    /// read certificate file into a list of cert objects.
    /// path: path of the .crt file
    fn read_certs(path: &str) -> Result<Vec<CertificateDer<'static>>>
    {
        let certs = File::open(path)?;
        let mut certs = BufReader::new(certs);
        rustls_pemfile::certs(&mut certs)
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| anyhow!("Failed to read certificate file {}", path))
    }

    /// read private key file into key object.
    /// path: path to private key file
    fn read_key(path: &str) -> Result<PrivateKeyDer<'static>>
    {
        let key = File::open(path)?;
        let mut key = BufReader::new(key);
        rustls_pemfile::private_key(&mut key)?
            .ok_or_else(|| anyhow!("cannot read private key file {}", path))
    }
}
