use tokio::time;

#[derive(Clone, Debug)]
pub struct HandleErrors {
    delay: time::Duration,
}

struct BackoffState {
    delay: time::Duration,
    failed: bool,
}

impl HandleErrors {
    pub fn fixed_delay(delay: time::Duration) -> Self {
        Self { delay }
    }

    /// Wraps a fallible stream, stripping and logging all errors
    ///
    /// When consecutive errors are encountered, a delay is applied before polling the stream again.
    pub fn handle_errors<S, T, E>(stream: S) -> impl Stream<Item = T>
    where
        S: Stream<Item = Result<T, E>>,
        E: std::fmt::Display,
    {
        let state = BackoffState {
            delay: time::Duration::from_millis(1),
            failed: false,
        };
        stream
            .scan(state, |state, result| {
                let delay = state.delay;
                let failed = state.failed;
                state.failed = result.is_err();

                async move {
                    match result {
                        Ok(item) => Some(Some(item)),
                        Err(error) => {
                            tracing::info!(%error, "stream failed");
                            if failed {
                                tracing::info!(?delay);
                                time::sleep(delay).await;
                            }
                            Some(None)
                        }
                    }
                }
            })
            .filter_map(|item| item)
    }
}
