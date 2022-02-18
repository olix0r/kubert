pub use kube::{Client, Config};

// TODO configure a --kubeconfig
#[cfg(feature = "cli")]
#[derive(Clone, Debug, clap::Parser)]
pub struct ClientArgs {
    // Kubernetes context.
    #[clap(long)]
    pub context: Option<String>,
}

#[cfg(feature = "cli")]
impl ClientArgs {
    pub async fn try_client(self) -> Result<Client, Box<dyn std::error::Error + 'static>> {
        let c = kube::config::KubeConfigOptions {
            context: self.context,
            ..Default::default()
        };
        Config::from_kubeconfig(&c)
            .await?
            .try_into()
            .map_err(Into::into)
    }
}
