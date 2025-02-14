//! Utilities for configuring a [`kube_client::Client`] from the command line
pub use kube_client::*;
use std::path::PathBuf;
use thiserror::Error;

/// Configures a Kubernetes client
#[derive(Clone, Debug, Default)]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct ClientArgs {
    /// The name of the kubeconfig cluster to use
    #[cfg_attr(feature = "clap", clap(long))]
    pub cluster: Option<String>,

    /// The name of the kubeconfig context to use
    #[cfg_attr(feature = "clap", clap(long))]
    pub context: Option<String>,

    /// The name of the kubeconfig user to use
    #[cfg_attr(feature = "clap", clap(long))]
    pub user: Option<String>,

    /// The path to the kubeconfig file to use
    #[cfg_attr(feature = "clap", clap(long))]
    pub kubeconfig: Option<PathBuf>,

    /// Username to impersonate for Kubernetes operations
    #[cfg_attr(feature = "clap", clap(long = "as"))]
    pub impersonate_user: Option<String>,

    /// Group to impersonate for Kubernetes operations
    #[cfg_attr(feature = "clap", clap(long = "as-group"))]
    pub impersonate_group: Option<String>,

    /// The timeout for response headers from the Kubernetes API.
    #[cfg_attr(feature = "clap", clap(
        long = "kube-api-response-headers-timeout",
        default_value_t = timeouts::ResponseHeaders::default(),
    ))]
    pub response_headers_timeout: timeouts::ResponseHeaders,
}

/// Indicates an error occurred while configuring the Kubernetes client
#[derive(Debug, Error)]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
#[non_exhaustive]
pub enum ConfigError {
    /// Indicates that the kubeconfig file could not be read
    #[error(transparent)]
    Kubeconfig(#[from] config::KubeconfigError),

    /// Indicates that the in-cluster configuration could not be read
    #[error(transparent)]
    InCluster(#[from] config::InClusterError),

    /// Indicates that the client could not be initialized
    #[error(transparent)]
    Client(#[from] Error),
}

impl ClientArgs {
    /// Initializes a Kubernetes client
    ///
    /// This will respect the `$KUBECONFIG` environment variable, but otherwise default to
    /// `~/.kube/config`. The _current-context_ is used unless `context` is set.
    ///
    /// This is basically equivalent to using `kube_client::Client::try_default`, except that it
    /// supports kubeconfig configuration from the command-line.
    pub async fn try_client(self) -> Result<Client, ConfigError> {
        let config = match self.load_local_config().await {
            Ok(config) => config,
            Err(e) if self.is_customized() => return Err(e),
            Err(_) => Config::incluster()?,
        };

        let client = kube_client::client::ClientBuilder::try_from(config)?
            .with_layer(&timeouts::layer(self.response_headers_timeout))
            .build();

        Ok(client)
    }

    /// Indicates whether the command-line arguments attempt to customize the Kubernetes
    /// configuration.
    fn is_customized(&self) -> bool {
        self.context.is_some()
            || self.cluster.is_some()
            || self.user.is_some()
            || self.impersonate_user.is_some()
            || self.impersonate_group.is_some()
            || self.kubeconfig.is_some()
    }

    /// Loads a local (i.e. not in-cluster) Kubernetes client configuration
    ///
    /// First, the `--kubeconfig` argument is used. If that is not set, the `$KUBECONFIG`
    /// environment variable is used. If that is not set, the `~/.kube/config` file is used.
    async fn load_local_config(&self) -> Result<Config, ConfigError> {
        let options = config::KubeConfigOptions {
            context: self.context.clone(),
            cluster: self.cluster.clone(),
            user: self.user.clone(),
        };

        let mut kubeconfig = match &self.kubeconfig {
            Some(path) => config::Kubeconfig::read_from(path.as_path())?,
            None => config::Kubeconfig::read()?,
        };

        if let Some(user) = &self.impersonate_user {
            for auth in kubeconfig.auth_infos.iter_mut() {
                if let Some(ai) = auth.auth_info.as_mut() {
                    ai.impersonate = Some(user.clone());
                }
            }
        }

        if let Some(group) = &self.impersonate_group {
            for auth in kubeconfig.auth_infos.iter_mut() {
                if let Some(ai) = auth.auth_info.as_mut() {
                    ai.impersonate_groups = Some(vec![group.clone()]);
                }
            }
        }

        Config::from_custom_kubeconfig(kubeconfig, &options)
            .await
            .map_err(Into::into)
    }
}

type BoxService = tower::util::BoxService<Request, Response, BoxError>;
type Request = hyper::Request<kube_client::client::Body>;
type Response = hyper::Response<BoxBody>;
type BoxBody = Box<dyn hyper::body::Body<Data = bytes::Bytes, Error = BoxError> + Send + Unpin>;
type BoxError = tower::BoxError;
type BoxFuture = futures_util::future::BoxFuture<'static, Result<Response, BoxError>>;

mod timeouts {
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
}
