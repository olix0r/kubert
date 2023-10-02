use boring::{
    error::ErrorStack,
    pkey::PKey,
    ssl::{SslConnector, SslConnectorBuilder, SslMethod},
    x509::X509,
};
use hyper_boring::HttpsConnector;
use kube_client::config::{AuthInfo, Config};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors from BoringSSL TLS
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Failed to create BoringSSL HTTPS connector
    #[error("failed to create BoringSSL HTTPS connector: {0}")]
    CreateHttpsConnector(#[source] ErrorStack),

    /// Failed to build SslConnectorBuilder
    #[error("failed to build BoringSSL SslConnectorBuilder: {0}")]
    CreateBuilder(#[source] ErrorStack),

    /// Failed to deserialize PEM-encoded chain of certificates
    #[error("failed to deserialize PEM-encoded chain of certificates: {0}")]
    DeserializeCertificateChain(#[source] ErrorStack),

    /// Failed to deserialize PEM-encoded private key
    #[error("failed to deserialize PEM-encoded private key: {0}")]
    DeserializePrivateKey(#[source] ErrorStack),

    /// Failed to set private key
    #[error("failed to set private key: {0}")]
    SetPrivateKey(#[source] ErrorStack),

    /// Failed to get a leaf certificate, the certificate chain is empty
    #[error("failed to get a leaf certificate, the certificate chain is empty")]
    GetLeafCertificate,

    /// Failed to set the leaf certificate
    #[error("failed to set the leaf certificate: {0}")]
    SetLeafCertificate(#[source] ErrorStack),

    /// Failed to append a certificate to the chain
    #[error("failed to append a certificate to the chain: {0}")]
    AppendCertificate(#[source] ErrorStack),

    /// Failed to deserialize DER-encoded root certificate
    #[error("failed to deserialize DER-encoded root certificate: {0}")]
    DeserializeRootCertificate(#[source] ErrorStack),

    /// Failed to add a root certificate
    #[error("failed to add a root certificate: {0}")]
    AddRootCertificate(#[source] ErrorStack),

    /// Failed to load client certificate from kubeconfig
    #[error("failed to load client certificate: {0}")]
    LoadClientCertificate(#[source] LoadDataError),

    /// Failed to load client key from kubeconfig
    #[error("failed to load client key: {0}")]
    LoadClientKey(#[source] LoadDataError),
}

/// Errors from loading data from a base64 string or a file
#[derive(Debug, Error)]
pub enum LoadDataError {
    /// Failed to decode base64 data
    #[error("failed to decode base64 data: {0}")]
    DecodeBase64(#[source] base64::DecodeError),

    /// Failed to read file
    #[error("failed to read file '{1:?}': {0}")]
    ReadFile(#[source] std::io::Error, PathBuf),

    /// No base64 data or file path was provided
    #[error("no base64 data or file")]
    NoBase64DataOrFile,
}

pub(crate) fn https_connector(
    cfg: &Config,
) -> Result<HttpsConnector<hyper::client::HttpConnector>, Error> {
    let mut connector = hyper::client::HttpConnector::new();
    connector.enforce_http(false);
    let identity = identity_pem(&cfg.auth_info)?;
    let builder = ssl_connector_builder(identity.as_ref(), cfg.root_cert.as_ref())?;
    let mut https =
        HttpsConnector::with_connector(connector, builder).map_err(Error::CreateHttpsConnector)?;
    if cfg.accept_invalid_certs {
        https.set_callback(|ssl, _uri| {
            ssl.set_verify(boring::ssl::SslVerifyMode::NONE);
            Ok(())
        });
    }
    Ok(https)
}

/// Create `boring::ssl::SslConnectorBuilder` required for `hyper_boring::HttpsConnector`.
fn ssl_connector_builder(
    identity_pem: Option<&Vec<u8>>,
    root_certs: Option<&Vec<Vec<u8>>>,
) -> Result<SslConnectorBuilder, Error> {
    let mut builder = SslConnector::builder(SslMethod::tls()).map_err(Error::CreateBuilder)?;
    if let Some(pem) = identity_pem {
        let mut chain = X509::stack_from_pem(pem)
            .map_err(Error::DeserializeCertificateChain)?
            .into_iter();
        let leaf_cert = chain.next().ok_or(Error::GetLeafCertificate)?;
        builder
            .set_certificate(&leaf_cert)
            .map_err(Error::SetLeafCertificate)?;
        for cert in chain {
            builder
                .add_extra_chain_cert(cert)
                .map_err(Error::AppendCertificate)?;
        }

        let pkey = PKey::private_key_from_pem(pem).map_err(Error::DeserializePrivateKey)?;
        builder
            .set_private_key(&pkey)
            .map_err(Error::SetPrivateKey)?;
    }

    if let Some(ders) = root_certs {
        for der in ders {
            let cert = X509::from_der(der).map_err(Error::DeserializeRootCertificate)?;
            builder
                .cert_store_mut()
                .add_cert(cert)
                .map_err(Error::AddRootCertificate)?;
        }
    }

    Ok(builder)
}

fn identity_pem(cfg: &AuthInfo) -> Result<Option<Vec<u8>>, Error> {
    use secrecy::ExposeSecret;
    use std::fs;

    fn load_from_base64_or_file<P: AsRef<Path>>(
        value: Option<&str>,
        file: Option<P>,
    ) -> Result<Vec<u8>, LoadDataError> {
        fn load_from_base64(value: &str) -> Result<Vec<u8>, LoadDataError> {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(value)
                .map_err(LoadDataError::DecodeBase64)
        }

        fn load_from_file<P: AsRef<Path>>(file: &P) -> Result<Vec<u8>, LoadDataError> {
            fs::read(file).map_err(|source| LoadDataError::ReadFile(source, file.as_ref().into()))
        }

        // Ensure there is a trailing newline in the blob
        // Don't bother if the blob is empty
        fn ensure_trailing_newline(mut data: Vec<u8>) -> Vec<u8> {
            if data.last().map(|end| *end != b'\n').unwrap_or(false) {
                data.push(b'\n');
            }
            data
        }

        let data = value
            .map(load_from_base64)
            .or_else(|| file.as_ref().map(load_from_file))
            .unwrap_or_else(|| Err(LoadDataError::NoBase64DataOrFile))?;
        Ok(ensure_trailing_newline(data))
    }

    if let Some(exec_pem) = cfg.exec.as_ref().and_then(auth_plugin_identity_pem) {
        return Ok(Some(exec_pem));
    }

    // no client cert *and* no client key is not an error --- if one or the
    // other is missing, that's an error.
    if cfg.client_certificate_data.is_none()
        && cfg.client_certificate.is_none()
        && cfg.client_key_data.is_none()
        && cfg.client_key.is_none()
    {
        return Ok(None);
    }

    let client_cert = load_from_base64_or_file(
        cfg.client_certificate_data.as_deref(),
        cfg.client_certificate,
    )
    .map_err(Error::LoadClientCertificate)?;

    let client_key = {
        let data = cfg
            .client_key_data
            .as_ref()
            .map(|secret| secret.expose_secret().as_str());
        load_from_base64_or_file(data, cfg.client_key).map_err(Error::LoadClientKey)?
    };

    Ok(Some(make_identity_pem(client_cert, client_key)))
}

// This is necessary to retrieve an identity when an exec plugin
// returns a client certificate and key instead of a token.
// This has be to be checked on TLS configuration vs tokens
// which can be added in as an AuthLayer.
fn auth_plugin_identity_pem(auth: &kube_client::config::ExecConfig) -> Option<Vec<u8>> {
    use kube_client::config::ExecInteractiveMode;
    use serde::{Deserialize, Serialize};
    use std::process::{Command, Stdio};
    /// ExecCredentials is used by exec-based plugins to communicate credentials to
    /// HTTP transports.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct ExecCredential {
        kind: Option<String>,
        #[serde(rename = "apiVersion")]
        api_version: Option<String>,
        spec: Option<ExecCredentialSpec>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ExecCredentialStatus>,
    }

    /// ExecCredenitalSpec holds request and runtime specific information provided
    /// by transport.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct ExecCredentialSpec {
        #[serde(skip_serializing_if = "Option::is_none")]
        interactive: Option<bool>,
    }

    /// ExecCredentialStatus holds credentials for the transport to use.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct ExecCredentialStatus {
        #[serde(rename = "expirationTimestamp")]
        expiration_timestamp: Option<String>,
        token: Option<String>,
        #[serde(rename = "clientCertificateData")]
        client_certificate_data: Option<String>,
        #[serde(rename = "clientKeyData")]
        client_key_data: Option<String>,
    }

    let mut cmd = Command::new(&auth.command?);
    if let Some(args) = &auth.args {
        cmd.args(args);
    }
    if let Some(env) = &auth.env {
        let envs = env
            .iter()
            .flat_map(|env| match (env.get("name"), env.get("value")) {
                (Some(name), Some(value)) => Some((name, value)),
                _ => None,
            });
        cmd.envs(envs);
    }

    let interactive = auth.interactive_mode != Some(ExecInteractiveMode::Never);
    if interactive {
        cmd.stdin(Stdio::inherit());
    } else {
        cmd.stdin(Stdio::piped());
    }

    // Provide exec info to child process
    let exec_info = {
        let info = ExecCredential {
            api_version: auth.api_version.clone(),
            kind: None,
            spec: Some(ExecCredentialSpec {
                interactive: Some(interactive),
            }),
            status: None,
        };
        match serde_json::to_string(&info) {
            Ok(i) => i,
            Err(error) => {
                tracing::error!(%error, ?info, "failed to serialize exec info");
                return None;
            }
        }
    };

    cmd.env("KUBERNETES_EXEC_INFO", exec_info);

    if let Some(envs) = &auth.drop_env {
        for env in envs {
            cmd.env_remove(env);
        }
    }

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let out = match cmd.output() {
        Ok(out) if !out.status.success() => {
            tracing::error!(?cmd, status = ?out.status, "failed to execute auth plugin");
            return None;
        }
        Err(error) => {
            tracing::error!(?cmd, %error, "failed to execute auth plugin");
            return None;
        }
        Ok(out) => out,
    };

    let creds: ExecCredential = match serde_json::from_slice(&out.stdout) {
        Err(error) => {
            tracing::error!(
                ?error,
                output = ?String::from_utf8_lossy(&out.stdout[..]),
                "failed to parse auth plugin output as JSON",
            );
            return None;
        }
        Ok(creds) => creds,
    };

    let status = match creds.status {
        None => {
            tracing::error!(
                output = ?creds,
                "auth plugin did not return credentials",
            );
            return None;
        }
        Some(status) => status,
    };

    match (status.client_certificate_data, status.client_key_data) {
        (None, None) => None,
        (Some(_), None) => {
            tracing::warn!("missing client key data from auth plugin");
            None
        }
        (None, Some(_)) => {
            tracing::warn!("missing client certificate data from auth plugin");
            None
        }
        (Some(cert), Some(key)) => Some(make_identity_pem(cert.into_bytes(), key)),
    }
}

fn make_identity_pem(cert: Vec<u8>, key: impl AsRef<[u8]>) -> Vec<u8> {
    let key = key.as_ref();
    let mut buf = cert;
    buf.reserve(key.len() + 2);
    buf.push(b'\n');
    buf.extend_from_slice(key);
    buf.push(b'\n');
    buf
}
