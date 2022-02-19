//! Utilities for configuring a [`kube_client::Client`] from the command line

pub use kube_client::*;
use thiserror::Error;

/// Configures a Kubernetes client
// TODO configure a --kubeconfig
#[derive(Clone, Debug)]
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
pub enum ConfigError {
    /// Indicates that the kubeconfig file could not be read
    #[error(transparent)]
    Kubeconfig(#[from] config::KubeconfigError),

    /// Indicates that the client could not be initialized
    #[error(transparent)]
    Client(#[from] Error),
}

impl ClientArgs {
    /// Initializes a Kubernetes client
    ///
    /// This will respect the `$KUBECONFIG` environment variable, but otherwise default to
    /// `~/.kube/config`. The _current-context_ is used unless `context` is set.
    pub async fn try_client(self) -> Result<Client, ConfigError> {
        let c = kube_client::config::KubeConfigOptions {
            context: self.context,
            cluster: self.cluster,
            user: self.user,
        };
        kube_client::Config::from_kubeconfig(&c)
            .await?
            .try_into()
            .map_err(Into::into)
    }
}
