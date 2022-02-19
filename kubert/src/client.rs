pub use kube_client::Client;
use thiserror::Error;

// TODO configure a --kubeconfig
#[derive(Clone, Debug)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct ClientArgs {
    // Kubernetes context.
    #[cfg_attr(feature = "clap", clap(long))]
    pub context: Option<String>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Kubeconfig(#[from] kube_client::config::KubeconfigError),

    #[error(transparent)]
    Client(#[from] kube_client::Error),
}

impl ClientArgs {
    pub async fn try_client(self) -> Result<Client, Error> {
        let c = kube_client::config::KubeConfigOptions {
            context: self.context,
            ..Default::default()
        };
        kube_client::Config::from_kubeconfig(&c)
            .await?
            .try_into()
            .map_err(Error::from)
    }
}
