#[cfg(unix)]
pub(crate) mod unix {
    use crate::shutdown::RegisterError;
    use tokio::signal::unix::{signal, Signal, SignalKind};

    #[derive(Debug)]
    #[must_use = "call `Shutdown::on_signal` to await a signal"]
    #[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
    pub(crate) struct Signals {
        interrupt: Signal,
        terminate: Signal,
    }

    impl Signals {
        pub(crate) fn new() -> Result<Self, RegisterError> {
            let interrupt = signal(SignalKind::interrupt())?;
            let terminate = signal(SignalKind::terminate())?;
            Ok(Self {
                interrupt,
                terminate,
            })
        }

        pub(crate) async fn recv(&mut self) {
            tokio::select! {
                _ = self.interrupt.recv() => {
                    tracing::debug!("Received SIGINT");

                },
                _ = self.terminate.recv() => {
                    tracing::debug!("Received SIGTERM");
                }
            }
        }
    }
}

#[cfg(windows)]
pub(crate) mod windows {
    use crate::shutdown::RegisterError;
    use tokio::signal::windows::{ctrl_shutdown, CtrlShutdown};

    #[derive(Debug)]
    #[must_use = "call `Shutdown::on_signal` to await a signal"]
    #[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
    pub(crate) struct Signals(CtrlShutdown);

    impl Signals {
        pub(crate) fn new() -> Result<Self, RegisterError> {
            // On Windows, we use Ctrl-Shutdown as this is what Kubernetes uses to signal
            // shutdown to windows containers. For reference, see:
            // https://kubernetes.io/docs/concepts/windows/intro/#compatibility-v1-pod
            let ctrl_shutdown = ctrl_shutdown()?;
            Ok(Self(ctrl_shutdown))
        }

        pub(crate) async fn recv(&mut self) {
            self.0.recv().await;
            tracing::debug!("Received Ctrl-Shutdown");
        }
    }
}
