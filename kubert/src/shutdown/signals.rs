#[cfg(target_os = "linux")]
pub(crate) mod linux {
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

#[cfg(target_os = "windows")]
pub(crate) mod windows {
    use crate::shutdown::RegisterError;
    use tokio::signal::windows::{ctrl_break, ctrl_c, CtrlBreak, CtrlC};

    #[derive(Debug)]
    #[must_use = "call `Shutdown::on_signal` to await a signal"]
    #[cfg_attr(docsrs, doc(cfg(feature = "shutdown")))]
    pub(crate) struct Signals {
        ctrl_break: CtrlBreak,
        ctrl_c: CtrlC,
    }

    impl Signals {
        pub(crate) fn new() -> Result<Self, RegisterError> {
            let ctrl_break = ctrl_break()?;
            let ctrl_c = ctrl_c()?;

            Ok(Self { ctrl_break, ctrl_c })
        }

        pub(crate) async fn recv(&mut self) {
            tokio::select! {
                _ = self.ctrl_break.recv() => {
                    tracing::debug!("Received Ctrl-Break");

                },
                _ = self.ctrl_c.recv() => {
                    tracing::debug!("Received Ctrl-C");
                }
            }
        }
    }
}
