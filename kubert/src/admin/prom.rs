use hyper::{header, Body, Request, Response, StatusCode};

#[derive(Clone, Debug)]
pub(super) struct Prometheus(prometheus::Registry);

impl From<prometheus::Registry> for Prometheus {
    fn from(reg: prometheus::Registry) -> Self {
        Self(reg)
    }
}

impl Prometheus {
    pub(super) fn respond(&self, req: Request<Body>) -> Response<Body> {
        match *req.method() {
            hyper::Method::GET | hyper::Method::HEAD => {
                match self.encode(accepts_gzip(req.headers())) {
                    Ok(body) => Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, prometheus::TEXT_FORMAT)
                        .body(body)
                        .expect("valid response"),

                    Err(error) => {
                        tracing::info!(%error, "Failed to encode prometheus metrics");
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::default())
                            .expect("valid response")
                    }
                }
            }

            _ => Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .header(header::ALLOW, "GET, HEAD")
                .body(Body::default())
                .expect("valid response"),
        }
    }

    fn encode(&self, gzip: bool) -> prometheus::Result<hyper::Body> {
        // Prefer to send a gzipped response if the client accepts it.
        if gzip {
            use deflate::{write::GzEncoder, CompressionOptions};
            let mut gz = GzEncoder::new(Vec::new(), CompressionOptions::fast());
            self.write(&mut gz)?;
            return Ok(gz.finish()?.into());
        }

        let mut buf = Vec::<u8>::new();
        self.write(&mut buf)?;
        Ok(buf.into())
    }

    fn write(&self, f: &mut impl std::io::Write) -> prometheus::Result<()> {
        use prometheus::Encoder;

        let metrics = self.0.gather();
        prometheus::TextEncoder.encode(&metrics, f)
    }
}

fn accepts_gzip(headers: &header::HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .any(|value| {
            value
                .to_str()
                .ok()
                .map(|value| value.contains("gzip"))
                .unwrap_or(false)
        })
}
