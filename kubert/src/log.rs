//! Configures the global default tracing subscriber

use thiserror::Error;

pub use tracing_subscriber::{util::TryInitError as LogInitError, EnvFilter as LogFilter};

#[cfg(feature = "clap")]
use clap::{Arg, ArgEnum, ArgMatches, Args, Command, FromArgMatches};
#[cfg(feature = "clap")]
use once_cell::sync::OnceCell;

/// Configures logging settings.
///
/// This type may be parsed from the command line using `clap`, or configured
/// manually. In some cases, it may be preferable to not use the default `clap`
/// implementation for `LogArgs`, so that environment variables, default log
/// targets, etc, may be overridden.
///
/// # Examples
///
/// If the default environment variable (`KUBERT_LOG`) and default value for the
/// log filter (`<BINARY_NAME>=info,warn`) are desired, the
/// `clap::FromArgMatches` impl for this type can be used directly:
///
/// ```rust
/// use clap::Parser;
///
/// #[derive(Parser)]
/// struct MyAppArgs {
///     #[clap(flatten)]
///     log: kubert::LogArgs,
///     // ...
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let args = MyAppArgs::parse();
///
///     let rt = kubert::Runtime::builder()
///         .with_log_args(args.log)
///         // ...
///         .build()
///         .await
///         .unwrap();
///     # drop(rt);
/// }
/// ```
///
/// Alternatively, a `LogArgs` instance may be constructed from user-defined values:
///
/// ```rust
/// use clap::Parser;
/// use kubert::log::{LogFilter, LogFormat};
///
/// #[derive(Parser)]
/// struct MyAppArgs {
///     // Use a different environment variable and default value than
///     // those provided by the `FromArgMatches` impl for `LogArgs`
///     #[clap(long, env = "MY_APP_LOG", default_value = "trace")]
///     log_filter: LogFilter,
///
///     #[clap(long, env = "MY_APP_LOG_FORMAT", default_value = "json")]
///     log_format: LogFormat,
///     
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let args = MyAppArgs::parse();
///
///      // Construct a `LogArgs` from the values we parsed using our
///      // custom configuration:
///     let log_args = kubert::LogArgs {
///         log_level: args.log_filter,
///         log_format: args.log_format,
///         ..Default::default()
///     };
///
///     let rt = kubert::Runtime::builder()
///         .with_log_args(log_args)
///         // ...
///         .build()
///         .await
///         .unwrap();
///     # drop(rt);
/// }
/// ```
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "log")))]
pub struct LogArgs {
    /// The log format to use.
    pub log_format: LogFormat,

    /// The filter that determines what tracing spans and events are enabled.
    pub log_level: LogFilter,

    /// Enables tokio-console support.
    ///
    /// If this is set, `kubert` must be compiled with the `tokio-console` cargo
    /// feature enabled and `RUSTFLAGS="--cfg tokio_unstable"` must be set.
    #[cfg(feature = "tokio-console")]
    pub tokio_console: bool,

    pub(crate) _p: (),
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
    /// Attempts to configure the global default `tracing` subscriber in the current scope, returning
    /// an error if one is already set
    ///
    /// This method returns an error if [a global default subscriber has already been set][set], or if a
    /// `log` logger has already been set.
    ///
    /// [set]: https://docs.rs/tracing-subscriber
    #[deprecated(since = "0.6.1", note = "use `LogArgs::try_init` instead")]
    pub fn try_init(self, filter: LogFilter) -> Result<(), LogInitError> {
        LogArgs {
            log_format: self,
            log_level: filter,
            tokio_console: false,
            _p: (),
        }
        .try_init()
    }
}

// === impl LogArgs ===

impl LogArgs {
    /// Attempts to configure the global default `tracing` subscriber in the current scope, returning
    /// an error if one is already set
    ///
    /// This method returns an error if [a global default subscriber has already been set][set], or if a
    /// `log` logger has already been set.
    pub fn try_init(self) -> Result<(), LogInitError> {
        use tracing_subscriber::prelude::*;

        let registry = tracing_subscriber::registry();

        // TODO(eliza): can we serve the tokio console server on the Admin server?
        #[cfg(feature = "tokio-console")]
        let registry = registry.with(self.tokio_console.then(console_subscriber::spawn));

        match self.log_format {
            LogFormat::Plain => registry
                .with(tracing_subscriber::fmt::layer().with_filter(self.log_level))
                .try_init()?,

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
                    .fmt_fields(tracing_subscriber::fmt::format::JsonFields::default())
                    .with_filter(self.log_level);

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
        let level = Arg::new("log-level")
            .long("log-level")
            .takes_value(true)
            .env("KUBERT_LOG")
            .help("The filter that determines what tracing spans and events are enabled")
            .long_help(
                // XXX(eliza): had to use tinyurl because `clap` would line-wrap the docs.rs URL :(
                "The filter that determines what tracing spans and events are enabled.\n\n\
                See here for details on the accepted syntax for tracing filters:\n\
                https://tinyurl.com/envfilter-directives",
            )
            .default_value(default_log_filter(&cmd));
        let format = Arg::new("log-format")
            .long("log-format")
            .takes_value(true)
            .help("Which log format to use.")
            .possible_values(
                LogFormat::value_variants()
                    .iter()
                    .filter_map(LogFormat::to_possible_value),
            )
            .default_value("plain");

        let cmd = cmd.arg(level).arg(format);
        #[cfg(feature = "tokio-console")]
        let cmd = cmd.arg(
            Arg::new("tokio-console")
                .long("tokio-console")
                .takes_value(false)
                .help("Enables `tokio-console` instrumentation")
                .long_help(
                    "Enables `tokio-console` instrumentation.\n\n\
            If this is set, `kubert` must be compiled with the \
            `tokio-console` cargo feature enabled, and \
            `RUSTFLAGS=\"--cfg tokio_unstable\"` must be set.",
                ),
        );
        cmd
    }
    fn augment_args_for_update(cmd: Command<'_>) -> Command<'_> {
        Self::augment_args(cmd)
    }
}

impl FromArgMatches for LogArgs {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        // The `log_level` and `log_format` arguments both have default values,
        // so we expect they will always be present.
        let log_level = matches.value_of_t::<LogFilter>("log-level")?;
        let log_format = matches.value_of_t::<LogFormat>("log-format")?;

        #[cfg(feature = "tokio-console")]
        let tokio_console = {
            use clap::error::{Error, ErrorKind};

            let enabled = matches.is_present("tokio-console");
            if !cfg!(tokio_unstable) {
                return Err(Error::raw(
                    ErrorKind::InvalidValue,
                    "The `--tokio-console` flag requires that `kubert` be \
                    compiled with RUSTFLAGS=\"--cfg tokio_unstable\".",
                ));
            }
            enabled
        };

        Ok(Self {
            log_level,
            log_format,
            #[cfg(feature = "tokio-console")]
            tokio_console,
            _p: (),
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        self.log_level = matches.value_of_t::<LogFilter>("log-level")?;
        self.log_format = matches.value_of_t::<LogFormat>("log-format")?;

        #[cfg(feature = "tokio-console")]
        {
            use clap::error::{Error, ErrorKind};

            let enabled = matches.is_present("tokio-console");
            if !cfg!(tokio_unstable) {
                return Err(Error::raw(
                    ErrorKind::InvalidValue,
                    "The `--tokio-console` flag requires that `kubert` be \
                    compiled with RUSTFLAGS=\"--cfg tokio_unstable\".",
                ));
            }
            self.tokio_console = enabled;
        }

        Ok(())
    }
}

impl Default for LogArgs {
    fn default() -> Self {
        Self {
            log_format: LogFormat::Plain,
            log_level: DEFAULT_FILTER
                .get()
                .and_then(|default| default.parse().ok())
                .unwrap_or_else(|| {
                    LogFilter::default()
                        .add_directive(tracing_subscriber::filter::LevelFilter::WARN.into())
                }),
            #[cfg(feature = "tokio-console")]
            tokio_console: false,
            _p: (),
        }
    }
}

static DEFAULT_FILTER: OnceCell<String> = OnceCell::new();

fn default_log_filter(cmd: &Command<'_>) -> &'static str {
    DEFAULT_FILTER.get_or_init(|| {
        let name = cmd.get_bin_name().unwrap_or_else(|| cmd.get_name());
        let mut filter = name.replace('-', "_");
        filter.push_str("=info,warn");
        filter
    })
}
