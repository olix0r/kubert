use super::{BoxError, BoxFuture, BoxService, Request, Response};
use kube_client::core::Duration as KubeDuration;
use std::task::{Context, Poll};
use tokio::time;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResponseHeaders(time::Duration);

#[derive(Debug, thiserror::Error)]
#[error("response headers timeout after {0:?}")]
pub struct ResponseHeadersTimeoutError(time::Duration);

#[derive(Debug)]
struct TimeoutService {
    response_headers_timeout: time::Duration,
    inner: BoxService,
}

pub fn layer(
    ResponseHeaders(response_headers_timeout): ResponseHeaders,
) -> impl tower::layer::Layer<BoxService, Service = BoxService> + Clone {
    tower::layer::layer_fn(move |inner| {
        BoxService::new(TimeoutService {
            response_headers_timeout,
            inner,
        })
    })
}

impl tower::Service<Request> for TimeoutService {
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

// === impl ResponseHeaders ===

impl ResponseHeaders {
    // This default timeout is fairly arbitrary, but intended to be
    // reasonably long enough that no legitimate API calls would be
    // affected. The value of 9s is chose to differentiate it from other 10s
    // timeouts in the system.
    const DEFAULT: Self = Self(time::Duration::from_secs(9));
}

impl Default for ResponseHeaders {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl std::str::FromStr for ResponseHeaders {
    type Err = <KubeDuration as std::str::FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse::<KubeDuration>()?.into()))
    }
}

impl std::fmt::Display for ResponseHeaders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        KubeDuration::from(self.0).fmt(f)
    }
}

#[cfg(test)]
#[test]
fn response_headers_roundtrip() {
    let orig = "2h3m4s5ms".parse::<ResponseHeaders>().expect("valid");
    assert_eq!(
        orig.to_string().parse::<ResponseHeaders>().expect("valid"),
        orig,
    );
}
