use super::*;
use tempfile::TempDir;

use rustls_pki_types::{pem::PemObject, CertificateDer};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

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

/// Each connection's task must complete once its client disconnects, rather
/// than waiting for the server to be drained.
#[tokio::test]
async fn conn_task_completes_after_response() {
    assert_conn_tasks_complete(|mut tls| async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tls.write_all(
            b"GET / HTTP/1.1\r\nHost: kubert.test.example.com\r\nConnection: close\r\n\r\n",
        )
        .await
        .expect("failed to write request");
        let mut rsp = Vec::new();
        tls.read_to_end(&mut rsp)
            .await
            .expect("failed to read response");
        assert!(
            rsp.starts_with(b"HTTP/1.1 200"),
            "unexpected response: {}",
            String::from_utf8_lossy(&rsp)
        );
    })
    .await;
}

/// Same as above, but the client hangs up without sending a request.
#[tokio::test]
async fn conn_task_completes_after_client_hangup() {
    assert_conn_tasks_complete(|tls| async move { drop(tls) }).await;
}

async fn assert_conn_tasks_complete<F, Fut>(client: F)
where
    F: Fn(tokio_rustls::client::TlsStream<TcpStream>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    const CONNS: usize = 5;

    let (_tempdir, tls) = gen_keys();
    let connector = tls_connector(&tls.certs);

    let tcp = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = tcp.local_addr().expect("failed to get local address");
    let (mut drain_tx, drain_rx) = drain::channel();
    let server = tokio::spawn(accept_loop(tcp, drain_rx, HTTPOkService, Arc::new(tls)));

    for _ in 0..CONNS {
        let sock = TcpStream::connect(addr).await.expect("failed to connect");
        let tls = connector
            .connect("kubert.test.example.com".try_into().unwrap(), sock)
            .await
            .expect("TLS handshake failed");
        client(tls).await;
    }

    // Stop accepting, so the only remaining drain watchers belong to connection
    // tasks. Each one is dropped when its task completes.
    server.abort();
    let _ = server.await;
    tokio::time::timeout(std::time::Duration::from_secs(2), drain_tx.closed())
        .await
        .expect("closed connections left their task running until the server drains");
}

fn tls_connector(TlsCertPath(certs): &TlsCertPath) -> tokio_rustls::TlsConnector {
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_file_iter(certs).expect("failed to read certs") {
        roots
            .add(cert.expect("invalid cert"))
            .expect("failed to add root cert");
    }
    // Use an explicit provider rather than installing a process-wide default.
    let provider = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider();
    let mut cfg = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .expect("failed to configure protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    tokio_rustls::TlsConnector::from(Arc::new(cfg))
}

/// Responds to every request with an empty 200.
#[derive(Clone)]
struct HTTPOkService;

impl Service<hyper::Request<hyper::body::Incoming>> for HTTPOkService {
    type Response = hyper::Response<String>;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: hyper::Request<hyper::body::Incoming>) -> Self::Future {
        std::future::ready(Ok(hyper::Response::new(String::new())))
    }
}
