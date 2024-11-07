use rcgen::{Certificate, CertificateParams, DnType, SanType};
use rustls::{ServerConfig, PrivateKey, Certificate as RustlsCert};
use std::sync::Arc;
use time::OffsetDateTime;
use time::Duration;
use tracing::info;

pub struct TlsConfig {
    pub cert_pem: String,
    pub key_pem: String,
    pub server_config: Arc<ServerConfig>,
}

impl TlsConfig {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Generate certificate parameters
        let mut params = CertificateParams::new(vec!["localhost".to_string()]);
        params.distinguished_name.push(DnType::CommonName, "localhost");
        params.distinguished_name.push(DnType::OrganizationName, "Development");
        params.subject_alt_names = vec![
            SanType::DnsName("localhost".to_string()),
            SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        ];
        // set not_before to now
        params.not_before = OffsetDateTime::now_utc();
        // set not_after to now + 365 days
        params.not_after = params.not_before + Duration::days(365);
        
        // Generate certificate
        let cert = Certificate::from_params(params)?;
        let cert_pem = cert.serialize_pem()?;
        let key_pem = cert.serialize_private_key_pem();

        // Convert to rustls format
        let cert_chain = vec![RustlsCert(cert.serialize_der()?)];
        let private_key = PrivateKey(cert.serialize_private_key_der());

        // Create rustls config
        let server_config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(cert_chain, private_key)?;

        info!("Generated self-signed certificate for localhost");

        Ok(Self {
            cert_pem,
            key_pem,
            server_config: Arc::new(server_config),
        })
    }

    pub fn into_server_config(self) -> Arc<rustls::ServerConfig> {
        self.server_config
    }
} 