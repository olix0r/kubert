//! Utilities for handling errors

use futures_core::{Future, Stream, TryStream};
use futures_util::ready;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::time;
use tracing::info;

pin_project_lite::pin_project! {
    /// Wraps a [`Stream`], handling errors by logging them and applying a backoff
    ///
    /// A delay is applied on consecutive errors--that is, after an error, the stream will be polled
    /// immediately and if that second poll fails, a delay is applied before polling the stream
    /// again until it succeeds.
    #[derive(Debug)]
    pub struct LogAndSleep<S> {
        delay: time::Duration,
        failed: bool,

        #[pin]
        sleep: time::Sleep,
        sleeping: bool,

        #[pin]
        stream: S,
    }
}

impl<S> LogAndSleep<S> {
    /// Creates an error handling stream that uses a fixed delay on consecutive errors
    pub fn fixed_delay(delay: time::Duration, stream: S) -> Self {
        Self {
            delay,
            failed: false,
            sleep: time::sleep(time::Duration::ZERO),
            sleeping: false,
            stream,
        }
    }
}

impl<S> Stream for LogAndSleep<S>
where
    S: TryStream,
    S::Error: std::fmt::Display,
{
    type Item = S::Ok;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            if *this.sleeping {
                ready!(this.sleep.as_mut().poll(cx));
                *this.sleeping = false;
            }

            match ready!(this.stream.as_mut().try_poll_next(cx)) {
                None => {
                    *this.failed = false;
                    return Poll::Ready(None);
                }

                Some(Ok(item)) => {
                    *this.failed = false;
                    return Poll::Ready(Some(item));
                }

                Some(Err(error)) => {
                    info!(%error, "stream failed");
                    if *this.failed {
                        *this.sleeping = true;
                        // If the stream had failed in its previous poll, then set a delay.
                        this.sleep
                            .as_mut()
                            .reset(time::Instant::now() + *this.delay);
                    }
                    *this.failed = true;
                }
            };
        }
    }
}

#[cfg(test)]
mod test {
    use super::LogAndSleep;
    use tokio::time;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_test::{assert_pending, assert_ready_eq, task};

    const DELAY: time::Duration = time::Duration::from_secs(3);

    #[tokio::test]
    async fn does_not_delay_after_ok() {
        time::pause();
        let (tx, mut rx) = {
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            let rx = task::spawn(LogAndSleep::fixed_delay(DELAY, ReceiverStream::new(rx)));
            (tx, rx)
        };

        assert_pending!(rx.poll_next());

        tx.try_send(Ok("first")).expect("stream not full");
        assert_ready_eq!(rx.poll_next(), Some("first"));

        tx.try_send(Err("second")).expect("stream not full");
        assert_pending!(rx.poll_next());

        tx.try_send(Ok("third")).expect("stream not full");
        assert_ready_eq!(rx.poll_next(), Some("third"));
    }

    #[tokio::test]
    async fn delays_on_repeated_errors() {
        time::pause();
        let (tx, mut rx) = {
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            let rx = task::spawn(LogAndSleep::fixed_delay(DELAY, ReceiverStream::new(rx)));
            (tx, rx)
        };

        assert_pending!(rx.poll_next());

        tx.try_send(Err("first")).expect("stream not full");
        assert_pending!(rx.poll_next());

        tx.try_send(Err("second")).expect("stream not full");
        tx.try_send(Ok("third")).expect("stream not full");
        assert_pending!(rx.poll_next());

        tokio::time::sleep(DELAY - time::Duration::from_millis(1)).await;
        assert_pending!(rx.poll_next());

        tokio::time::sleep(time::Duration::from_millis(1)).await;
        assert_ready_eq!(rx.poll_next(), Some("third"));
    }
}
