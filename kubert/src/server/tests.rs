use super::*;
use tempfile::TempDir;

fn gen_keys() -> (TempDir, TlsPaths) {
    use std::{fs::File, io::Write};

    let dir = TempDir::with_prefix("kubert-test").expect("failed to create temporary directory");

    let cert = rcgen::generate_simple_self_signed(vec!["kubert.test.example.com".to_string()])
        .expect("failed to generate certs");

    let certs = {
        let path = dir.path().join("cert.pem");
        let mut file = File::create(&path).expect("failed to create cert file");
        let pem = cert
            .serialize_pem()
            .expect("failed to serializez certs PEM");
        file.write_all(pem.as_bytes())
            .expect("failed to write certs PEM to tempfile");
        TlsCertPath(path)
    };

    let key = {
        let path = dir.path().join("key.pem");
        let mut file = File::create(&path).expect("failed to create private key file");
        let pem = cert.serialize_private_key_pem();
        file.write_all(pem.as_bytes())
            .expect("failed to write private key PEM to tempfile");
        TlsKeyPath(path)
    };

    (dir, TlsPaths { key, certs })
}

#[tokio::test]
// if no TLS features are enabled, this will panic.
#[cfg_attr(
    not(any(feature = "rustls-tls", feature = "openssl-tls")),
    should_panic
)]
async fn load_tls() {
    let (_tempdir, TlsPaths { key, certs }) = gen_keys();
    match super::tls::load_tls(&key, &certs).await {
        Ok(_) => eprintln!("load_tls: success!"),
        Err(error) => panic!("load_tls failed! {error}"),
    }
}
