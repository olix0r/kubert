//! Configures the global default tracing subscriber

use thiserror::Error;
use tracing_subscriber::util::TryInitError;

pub use tracing_subscriber::EnvFilter;

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

/// Indicates that an invalid log format was specified
#[derive(Debug, Error)]
#[error("invalid log level: {0} must be 'plain' or 'json'")]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
pub struct InvalidLogFormat(String);

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
    pub fn try_init(self, filter: EnvFilter) -> Result<(), TryInitError> {
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
