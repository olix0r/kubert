//! Configures the global default tracing subscriber

use std::sync::Arc;
use thiserror::Error;
use tracing::{metadata::LevelFilter, span, subscriber::Interest, Metadata, Subscriber};
use tracing_subscriber::{
    filter::ParseError,
    layer::{Context, Filter},
    EnvFilter, Layer,
};

pub use tracing_subscriber::util::TryInitError as LogInitError;

/// Configures whether logs should be emitted in plaintext (the default) or as JSON-encoded
/// messages
#[derive(Clone, Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
pub enum LogFormat {
    /// The default plaintext format
    Plain,

    /// The JSON-encoded format
    Json,
}

/// Configures the global default tracing filters.
///
/// A cloneable version of [`tracing_subscriber::EnvFilter`].
#[derive(Clone, Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
pub struct LogFilter(Arc<EnvFilter>);

/// Indicates that an invalid log format was specified
#[derive(Debug, Error)]
#[error("invalid log level: {0} must be 'plain' or 'json'")]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
pub struct InvalidLogFormat(String);

// ==== impl LogFilter ===

impl LogFilter {
    /// Returns a new `LogFilter` from the value of the `RUST_LOG` environment
    /// variable, ignoring any invalid filter directives.
    ///
    /// If the environment variable is empty or not set, or if it contains only
    /// invalid directives, a default directive enabling the [`ERROR`] level is
    /// added.
    ///
    /// [`ERROR`]: tracing::Level::ERROR
    #[inline]
    pub fn from_default_env() -> Self {
        Self(EnvFilter::from_default_env().into())
    }
}

impl std::str::FromStr for LogFilter {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let filter = s.parse::<EnvFilter>()?;
        Ok(Self(filter.into()))
    }
}

impl<S: Subscriber> Layer<S> for LogFilter {
    #[inline]
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        Layer::<S>::register_callsite(&*self.0, metadata)
    }

    #[inline]
    fn max_level_hint(&self) -> Option<LevelFilter> {
        self.0.max_level_hint()
    }

    #[inline]
    fn enabled(&self, metadata: &Metadata<'_>, ctx: Context<'_, S>) -> bool {
        self.0.enabled(metadata, ctx)
    }

    #[inline]
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        self.0.on_new_span(attrs, id, ctx)
    }

    #[inline]
    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        self.0.on_record(id, values, ctx);
    }

    #[inline]
    fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
        self.0.on_enter(id, ctx);
    }

    #[inline]
    fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
        self.0.on_exit(id, ctx);
    }

    #[inline]
    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        self.0.on_close(id, ctx);
    }
}

impl<S> Filter<S> for LogFilter {
    #[inline]
    fn enabled(&self, meta: &Metadata<'_>, ctx: &Context<'_, S>) -> bool {
        self.0.enabled(meta, ctx.clone())
    }

    #[inline]
    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> Interest {
        Filter::<S>::callsite_enabled(&*self.0, meta)
    }

    #[inline]
    fn max_level_hint(&self) -> Option<LevelFilter> {
        self.0.max_level_hint()
    }

    #[inline]
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        self.0.on_new_span(attrs, id, ctx)
    }

    #[inline]
    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        self.0.on_record(id, values, ctx);
    }

    #[inline]
    fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
        self.0.on_enter(id, ctx);
    }

    #[inline]
    fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
        self.0.on_exit(id, ctx);
    }

    #[inline]
    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        self.0.on_close(id, ctx);
    }
}

// === impl LogFormat ===

impl Default for LogFormat {
    fn default() -> Self {
        Self::Plain
    }
}

impl std::str::FromStr for LogFormat {
    type Err = InvalidLogFormat;

    fn from_str(s: &str) -> Result<Self, InvalidLogFormat> {
        match s {
            "json" => Ok(LogFormat::Json),
            "plain" => Ok(LogFormat::Plain),
            s => Err(InvalidLogFormat(s.to_string())),
        }
    }
}

impl LogFormat {
    /// Attempts to configure the global default tracing subscriber in the current scope, returning
    /// an error if one is already set
    ///
    /// This method returns an error if a global default subscriber has already been set, or if a
    /// `log` logger has already been set.
    pub fn try_init(self, filter: LogFilter) -> Result<(), LogInitError> {
        use tracing_subscriber::prelude::*;

        let registry = tracing_subscriber::registry().with(filter);

        match self {
            LogFormat::Plain => registry.with(tracing_subscriber::fmt::layer()).try_init()?,

            LogFormat::Json => {
                let event_fmt = tracing_subscriber::fmt::format()
                    // Configure the formatter to output JSON logs.
                    .json()
                    // Output the current span context as a JSON list.
                    .with_span_list(true)
                    // Don't output a field for the current span, since this
                    // would duplicate information already in the span list.
                    .with_current_span(false);

                // Use the JSON event formatter and the JSON field formatter.
                let fmt = tracing_subscriber::fmt::layer()
                    .event_format(event_fmt)
                    .fmt_fields(tracing_subscriber::fmt::format::JsonFields::default());

                registry.with(fmt).try_init()?
            }
        };

        Ok(())
    }
}
