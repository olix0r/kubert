//! Utilities for configuring a [`kube_client::Client`] from the command line

use hyper::{body::HttpBody, Body, Request, Response};
pub use kube_client::*;
use std::path::PathBuf;
use thiserror::Error;
use tower::{BoxError, Service, ServiceBuilder};

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
}

/// Indicates an error occurred while configuring the Kubernetes client
#[derive(Debug, Error)]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
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
        let client = match self.load_local_config().await {
            Ok(client) => client,
            Err(e) if self.is_customized() => return Err(e),
            Err(_) => Config::incluster()?,
        };

        client.try_into().map_err(Into::into)
    }

    /// Initializes a Kubernetes client from a [`tower::Service`].
    ///
    /// This will respect the `$KUBECONFIG` environment variable, but otherwise default to
    /// `~/.kube/config`. The _current-context_ is used unless `context` is set.
    ///
    /// This is basically equivalent to using `kube_client::Client::new`, except that it
    /// supports kubeconfig configuration from the command-line.
    pub async fn try_from_service<S, B>(self, svc: S) -> Result<Client, ConfigError>
    where
        S: Service<Request<Body>, Response = Response<B>> + Send + Clone + 'static,
        S::Future: Send + 'static,
        S::Error: Into<BoxError>,
        B: HttpBody<Data = bytes::Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        use kube_client::client::{ClientBuilder, ConfigExt};

        let config = match self.load_local_config().await {
            Ok(client) => client,
            Err(e) if self.is_customized() => return Err(e),
            Err(_) => Config::incluster()?,
        };

        let stack = ServiceBuilder::new()
            .layer(config.base_uri_layer())
            // TODO(eliza): add an equivalent gzip config to the one from kube_client?
            .option_layer(config.auth_layer()?)
            .layer(config.extra_headers_layer()?)
            // TODO(eliza): consider adding tracing layer a la kube-client?
            .service(svc);

        Ok(ClientBuilder::new(stack, config.default_namespace).build())
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
