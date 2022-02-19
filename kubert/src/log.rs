use thiserror::Error;
use tracing_subscriber::util::TryInitError;

pub use tracing::*;
pub use tracing_subscriber::EnvFilter;

#[derive(Clone, Debug)]
pub enum LogFormat {
    Json,
    Plain,
}

#[derive(Debug, Error)]
#[error("invalid log level: {0} must be 'plain' or 'json'")]
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
