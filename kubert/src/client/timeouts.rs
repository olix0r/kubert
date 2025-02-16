use super::svc::{self, BoxError, BoxFuture, BoxService, Request, Response};
use kube_client::core::Duration as KubeDuration;
use std::task::{Context, Poll};
use tokio::time;

/// A timeout for the response headers of an HTTP request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResponseHeadersTimeout(time::Duration);

#[derive(Debug, thiserror::Error)]
#[error("response headers timeout after {0:?}")]
pub struct ResponseHeadersTimeoutError(time::Duration);

#[derive(Debug)]
struct TimeoutService {
    response_headers_timeout: time::Duration,
    inner: BoxService,
}

pub fn layer(
    ResponseHeadersTimeout(response_headers_timeout): ResponseHeadersTimeout,
) -> impl svc::Layer<BoxService, Service = BoxService> + Clone {
    svc::layer_fn(move |inner| {
        BoxService::new(TimeoutService {
            response_headers_timeout,
            inner,
        })
    })
}

impl svc::Service<Request> for TimeoutService {
    type Response = Response;
    type Error = BoxError;
    type Future = BoxFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let Self {
            response_headers_timeout,
            ref mut inner,
        } = *self;
        let call = time::timeout(response_headers_timeout, inner.call(req));
        Box::pin(async move {
            let rsp = call
                .await
                .map_err(|_| ResponseHeadersTimeoutError(response_headers_timeout))??;
            // TODO request timeouts
            Ok(rsp)
        })
    }
}

// === impl ResponseHeadersTimeout ===

impl ResponseHeadersTimeout {
    // This default timeout is fairly arbitrary, but intended to be reasonably
    // long enough that no legitimate API calls would be affected. The value of
    // 9s is chosen to differentiate it from other 10s timeouts in the system.
    const DEFAULT: Self = Self(time::Duration::from_secs(9));
}

impl Default for ResponseHeadersTimeout {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl std::str::FromStr for ResponseHeadersTimeout {
    type Err = <KubeDuration as std::str::FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse::<KubeDuration>()?.into()))
    }
}

impl std::fmt::Display for ResponseHeadersTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        KubeDuration::from(self.0).fmt(f)
    }
}

#[cfg(test)]
#[test]
fn response_headers_timeout_roundtrip() {
    let orig = "2h3m4s5ms"
        .parse::<ResponseHeadersTimeout>()
        .expect("valid");
    assert_eq!(
        orig.to_string()
            .parse::<ResponseHeadersTimeout>()
            .expect("valid"),
        orig,
    );
}
