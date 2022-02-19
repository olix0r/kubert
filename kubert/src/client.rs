pub use kube::Client;
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
    Kubeconfig(#[from] kube::config::KubeconfigError),

    #[error(transparent)]
    Client(#[from] kube::Error),
}

impl ClientArgs {
    pub async fn try_client(self) -> Result<Client, Error> {
        let c = kube::config::KubeConfigOptions {
            context: self.context,
            ..Default::default()
        };
        kube::Config::from_kubeconfig(&c)
            .await?
            .try_into()
            .map_err(Error::from)
    }
}
