//! Configures the global default tracing subscriber

use thiserror::Error;

pub use tracing_subscriber::{util::TryInitError as LogInitError, EnvFilter as LogFilter};

#[cfg(feature = "clap")]
use clap::{Arg, ArgEnum, ArgMatches, Args, Command, FromArgMatches};

/// Configures logging settings.
#[derive(Debug, Default)]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
pub struct LogArgs {
    /// The log format to use.
    #[cfg_attr(
        feature = "clap",
        clap(long, default_value = "plain", possible_values = ["plain", "json"])
    )]
    pub log_format: LogFormat,

    /// The filter that determines what tracing spans and events are enabled.
    #[cfg_attr(feature = "clap", clap(flatten))]
    pub log_level: LogFilter,

    /// Enables tokio-console support.
    ///
    /// If this is set, `kubert` must be compiled with the `tokio-console` cargo
    /// feature enabled and `RUSTFLAGS="--cfg tokio_unstable"` must be set.
    #[cfg(all(tokio_unstable, feature = "tokio-console"))]
    #[cfg_attr(docsrs, doc(cfg(all(tokio_unstable, feature = "tokio-console"))))]
    #[cfg_attr(feature = "clap", clap(long, parse = validate_console))]
    pub tokio_console: bool,
}

impl LogArgs {
    /// Returns the log level filter.
    pub fn log_level(&self) -> LogFilter {
        self.log_level.0.clone()
    }
}
/// Configures whether logs should be emitted in plaintext (the default) or as JSON-encoded
/// messages
#[derive(Clone, Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
#[cfg_attr(feature = "clap", derive(ArgEnum))]
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

// This hand-implements `Args` in order to generate some values based
// on the command's name.

impl Args for LogArgs {
    fn augment_args(cmd: Command<'_>) -> Command<'_> {
        let level = Arg::new("log_level")
            .long("log_level")
            .takes_value(true)
            .env("KUBERT_LOG")
            .help("Configures the log level filter.")
            .default_value(default_log_filter(&cmd));
        let format = Arg::new("log_format")
            .long("log_format")
            .takes_value(true)
            .help("Which log format to use.")
            .possible_values(LogFormat::value_variants)
            .default_value("plain");

        cmd.arg(arg).arg(format)
    }
    fn augment_args_for_update(cmd: Command<'_>) -> Command<'_> {
        Self::augment_args(cmd)
    }
}
impl FromArgMatches for LogArgs {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        use clap::error::{Error, ErrorKind};
        let filter = matches
            .value_of("log_level")
            .expect("arg with default value is always present");
        LogFilter::try_new(filter)
            .map_err(|error| Error::raw(ErrorKind::InvalidValue, error))
            .map(Self)
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        use clap::error::{Error, ErrorKind};
        if let Some(filter) = matches.value_of("log_level") {
            self.0 = LogFilter::try_new(filter)
                .map_err(|error| Error::raw(ErrorKind::InvalidValue, error))?;
        }
        Ok(())
    }
}

fn default_log_filter(cmd: &Command<'_>) -> &'static str {
    use once_cell::sync::OnceCell;

    static DEFAULT_FILTER: OnceCell<String> = OnceCell::new();
    DEFAULT_FILTER
        .get_or_init(|| match cmd.get_bin_name() {
            Some(name) => {
                let mut filter = name.replace('-', "_");
                filter.push_str("=info,warn");
                filter
            }
            None => String::from("warn"),
        })
        .as_str()
}
