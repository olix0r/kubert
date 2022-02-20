//! Utilities for configuring a [`kube_client::Client`] from the command line

pub use kube_client::*;
use thiserror::Error;

/// Configures a Kubernetes client
// TODO configure a --kubeconfig
#[derive(Clone, Debug)]
#[cfg_attr(docsrs, doc(cfg(any(feature = "client"))))]
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
}

/// Indicates an error occurred while configuring the Kubernetes client
#[derive(Debug, Error)]
#[cfg_attr(docsrs, doc(cfg(any(feature = "shutdown"))))]
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
        let c = kube_client::config::KubeConfigOptions {
            context: self.context,
            cluster: self.cluster,
            user: self.user,
        };

        let client = match kube_client::Config::from_kubeconfig(&c).await {
            Ok(client) => client,
            Err(e) if c.context.is_some() || c.cluster.is_some() || c.user.is_some() => {
                return Err(e.into())
            }
            Err(_) => kube_client::Config::from_cluster_env()?,
        };

        client.try_into().map_err(Into::into)
    }
}
