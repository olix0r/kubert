use super::*;

use hyper::header;

#[derive(Clone, Debug)]
pub(super) struct Prometheus {
    registry: Arc<prometheus_client::registry::Registry>,
}

impl Prometheus {
    pub(super) fn new(reg: prometheus_client::registry::Registry) -> Self {
        Self {
            registry: reg.into(),
        }
    }

    pub(super) fn handle_metrics(&self, req: Request<Body>) -> Response<Body> {
        if !matches!(*req.method(), hyper::Method::GET | hyper::Method::HEAD) {
            return Response::builder()
                .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
                .header(header::ALLOW, "GET, HEAD")
                .body(Body::default())
                .unwrap();
        }

        let gzip = accepts_gzip(req.headers());
        let body = match self.encode_body(gzip) {
            Ok(body) => body,
            Err(error) => {
                tracing::error!(%error, "Failed to encode metrics");
                return Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::default())
                    .unwrap();
            }
        };

        let mut rsp = Response::builder().status(hyper::StatusCode::OK).header(
            header::CONTENT_TYPE,
            "application/openmetrics-text; version=1.0.0; charset=utf-8",
        );
        if gzip {
            rsp = rsp.header(header::CONTENT_ENCODING, "gzip");
        }
        rsp.body(body).expect("response must be valid")
    }

    fn encode_body(&self, gzip: bool) -> std::result::Result<hyper::Body, std::fmt::Error> {
        if gzip {
            struct GzFmt<'a>(&'a mut deflate::write::GzEncoder<Vec<u8>>);
            impl std::fmt::Write for GzFmt<'_> {
                fn write_str(&mut self, s: &str) -> std::fmt::Result {
                    use std::io::Write as _;
                    self.0.write_all(s.as_bytes()).map_err(|_| std::fmt::Error)
                }
            }

            let mut gz = deflate::write::GzEncoder::new(vec![], deflate::Compression::Fast);
            prometheus_client::encoding::text::encode(&mut GzFmt(&mut gz), &self.registry)?;
            let buf = gz.finish().map_err(|_| std::fmt::Error)?;
            return Ok(hyper::Body::from(buf));
        }

        let mut buf = String::new();
        prometheus_client::encoding::text::encode(&mut buf, &self.registry)?;
        Ok(hyper::Body::from(buf))
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
