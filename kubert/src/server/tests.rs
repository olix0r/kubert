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
        let pem = cert.cert.pem();
        file.write_all(pem.as_bytes())
            .expect("failed to write certs PEM to tempfile");
        TlsCertPath(path)
    };

    let key = {
        let path = dir.path().join("key.pem");
        let mut file = File::create(&path).expect("failed to create private key file");
        let pem = cert.key_pair.serialize_pem();
        file.write_all(pem.as_bytes())
            .expect("failed to write private key PEM to tempfile");
        TlsKeyPath(path)
    };

    (dir, TlsPaths { key, certs })
}

#[cfg(feature = "rustls-tls")]
#[tokio::test]
async fn load_tls_rustls() {
    tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("installing aws-lc-rs provider must succeed");
    let (_tempdir, TlsPaths { key, certs }) = gen_keys();
    match super::tls_rustls::load_tls(&key, &certs).await {
        Ok(_) => println!("load_tls: success!"),
        Err(error) => panic!("load_tls failed! {error}"),
    }
}

#[cfg(feature = "openssl-tls")]
#[tokio::test]
async fn load_tls_openssl() {
    let (_tempdir, TlsPaths { key, certs }) = gen_keys();
    match super::tls_openssl::load_tls(&key, &certs).await {
        Ok(_) => println!("load_tls: success!"),
        Err(error) => panic!("load_tls failed! {error}"),
    }
}
