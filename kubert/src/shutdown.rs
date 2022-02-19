use tokio::signal::unix::{signal, SignalKind};
use tracing::debug;

#[must_use = "call `Shutdown::on_signal` to await a signal"]
pub struct Shutdown(drain::Signal);

pub fn channel() -> (Shutdown, drain::Watch) {
    let (drain_tx, drain_rx) = drain::channel();
    (Shutdown(drain_tx), drain_rx)
}

impl Shutdown {
    pub async fn on_signal(self) -> std::io::Result<()> {
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;

        tokio::select! {
            _ = interrupt.recv() => {
                debug!("Received SIGINT; draining");
            },

            _ = terminate.recv() => {
                debug!("Received SIGTERM; draining");
            }
        }

        tokio::select! {
            _ = self.0.drain() => {
                debug!("Shutdown");
            },

            _ = interrupt.recv() => {
                debug!("Received SIGINT; aborting");
            },

            _ = terminate.recv() => {
                debug!("Received SIGTERM; aborting");
            }
        }

        Ok(())
    }
}
