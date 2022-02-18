pub use kube::Client;

// TODO configure a --kubeconfig
#[derive(Clone, Debug)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct ClientArgs {
    // Kubernetes context.
    #[cfg_attr(feature = "clap", clap(long))]
    pub context: Option<String>,
}

impl ClientArgs {
    pub async fn try_client(
        self,
    ) -> Result<Client, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let c = kube::config::KubeConfigOptions {
            context: self.context,
            ..Default::default()
        };
        kube::Config::from_kubeconfig(&c)
            .await?
            .try_into()
            .map_err(Into::into)
    }
}
