use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::Ipv4Addr;

pub const VERSION: u8 = 1;
pub const DNS_NAME: &str = "host.aipp.invalid";
pub const PORT: u16 = 41783;

/// Pairing supplies the only trust anchor. Use a fixed certificate DNS name so that
/// a DHCP address change doesn't alter the host's cryptographic identity.
#[cfg(any(target_os = "android", test))]
pub fn pinned_client(certificate: &str, target: std::net::SocketAddr, timeout: std::time::Duration) -> anyhow::Result<reqwest::Client> {
    let root = reqwest::Certificate::from_der(&STANDARD.decode(certificate)?)?;
    Ok(reqwest::Client::builder().no_proxy().https_only(true)
        .tls_certs_only([root]).redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(6)).timeout(timeout)
        .resolve(DNS_NAME,target).build()?)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub name: String,
    pub address: Ipv4Addr,
    // The exact pairing certificate is the client's only TLS trust anchor.
    pub certificate: String,
}

impl Endpoint {
    pub fn id(&self) -> anyhow::Result<String> {
        anyhow::ensure!(self.address.is_private() || self.address.is_link_local(), "请选择局域网地址");
        valid_name(&self.name)?;
        let der = STANDARD.decode(&self.certificate)?;
        anyhow::ensure!(!der.is_empty() && der.len() <= 2048, "配对证书长度无效");
        let cert = rustls::pki_types::CertificateDer::from(der);
        let key = rustls::server::ParsedCertificate::try_from(&cert)?.subject_public_key_info();
        Ok(hex::encode(Sha256::digest(key.as_ref())))
    }
}

pub fn valid_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!name.trim().is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control), "设备名称须为 1 至 64 个可见字符");
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Invitation {
    pub protocol_version: u8,
    pub endpoint: Endpoint,
    pub code: uuid::Uuid,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairRequest {
    pub protocol_version: u8,
    pub code: uuid::Uuid,
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Access {
    pub assistants: Vec<i64>,
    pub butler: bool,
    #[serde(default)]
    pub chat: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Destination {
    New { assistant: i64 },
    Existing { conversation: i64 },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatAction {
    OpenButler { assistant: Option<i64> },
    Send { request: uuid::Uuid, destination: Destination, text: String },
    Receipt { request: uuid::Uuid },
    Stop { request: uuid::Uuid },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: uuid::Uuid,
    pub name: String,
    pub access: Access,
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub token: String,
}
