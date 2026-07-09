pub mod logging;

pub(crate) mod enclave;
pub(crate) mod host;
pub(crate) mod parsers;

// ---------------------------------------------------------------------

use clap::{Parser, Subcommand};

use crate::cli::{enclave::CliEnclave, host::CliHost, logging::CliLogging};

// ---------------------------------------------------------------------

pub(crate) const APP: &str = "nitro-network-proxy";
pub(crate) const ENV: &str = "GLYCERINE";

// Cli -----------------------------------------------------------------

#[derive(Clone, Debug, Parser)]
#[command(about, author, long_about = None, name = APP, term_width = 90, version)]
pub struct Cli {
    #[command(flatten)]
    pub logging: CliLogging,

    #[command(subcommand)]
    pub command: CliCommands,
}

// CliCommands ---------------------------------------------------------

#[derive(Clone, Debug, Subcommand)]
pub enum CliCommands {
    /// Run enclave proxy
    #[command(name = "enclave")]
    CliEnclave(Box<CliEnclave>),

    /// Run host (parent VM) proxy
    #[command(name = "host")]
    CliHost(Box<CliHost>),
}
