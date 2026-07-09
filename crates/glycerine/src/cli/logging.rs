use std::fmt::Display;

use clap::Args;
use tracing_subscriber::{EnvFilter, fmt, prelude::*, util::SubscriberInitExt};

use crate::cli::{ENV, parsers::LogLevelParser};

// ---------------------------------------------------------------------

const SECTION: &str = "logging";

const DEFAULT_FORMAT: CliLoggingFormat = CliLoggingFormat::Json;
const DEFAULT_LEVEL: &str = "info";

// CliLogging ----------------------------------------------------------

#[derive(Args, Clone, Debug)]
pub struct CliLogging {
    /// logging format
    #[arg(
        default_value = DEFAULT_FORMAT.to_string(),
        env = format!("{ENV}_LOG_FORMAT"),
        help_heading = SECTION,
        long("log-format"),
        name("log_format"),
        value_name = "format"
    )]
    #[clap(value_enum)]
    pub(crate) format: CliLoggingFormat,

    /// logging level
    #[arg(
        help_heading = SECTION,
        default_value = DEFAULT_LEVEL,
        env = format!("{ENV}_LOG_LEVEL"),
        long(format!("log-level")),
        name("log_level"),
        value_name = "level",
        value_parser = LogLevelParser{},
    )]
    pub(crate) level: EnvFilter,
}

impl Default for CliLogging {
    fn default() -> Self {
        Self { format: DEFAULT_FORMAT, level: DEFAULT_LEVEL.into() }
    }
}

impl CliLogging {
    pub fn setup(&self) {
        match self.format {
            CliLoggingFormat::Json => tracing_subscriber::registry()
                .with(self.level.clone())
                .with(fmt::layer().json().flatten_event(true))
                .init(),

            CliLoggingFormat::Text => {
                tracing_subscriber::registry().with(self.level.clone()).with(fmt::layer()).init()
            }
        }
    }
}

// CliLoggingFormat ----------------------------------------------------

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum CliLoggingFormat {
    Json,
    Text,
}

impl Display for CliLoggingFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => f.write_str("json"),
            Self::Text => f.write_str("text"),
        }
    }
}

impl From<&str> for CliLoggingFormat {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "json" => Self::Json,
            "text" => Self::Text,
            _ => panic!("invalid logging format: {value}"),
        }
    }
}
