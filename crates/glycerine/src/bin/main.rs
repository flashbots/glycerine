use std::{process, time::Duration};

use clap::Parser;
use glycerine::{
    cli::{Cli, CliCommands},
    proxy::{
        Error,
        {self},
    },
};
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

// main ----------------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    cli.logging.setup();

    let backoff = backoff::ExponentialBackoffBuilder::new()
        .with_initial_interval(Duration::from_millis(100))
        .with_multiplier(1.5)
        .with_max_interval(Duration::from_millis(1_000))
        .with_max_elapsed_time(Some(Duration::from_millis(60_000)))
        .build();

    let shutdown_signal = wait_for_shutdown_signal();

    let mut tasks = futures::future::join_all(
        match match &cli.command {
            CliCommands::CliEnclave(cfg) => {
                proxy::enclave::run(cfg, backoff, shutdown_signal.clone()).await
            }

            CliCommands::CliHost(cfg) => {
                proxy::host::run(cfg, backoff, shutdown_signal.clone()).await
            }
        } {
            Ok(res) => res,

            Err(Error::GlycerineShutdown) => process::exit(0),

            Err(err) => {
                error!(error = &err.to_string(), "General failure");
                process::exit(1);
            }
        },
    );

    let res = tokio::select! {
        res = &mut tasks => {
            shutdown_signal.cancel();
            res
        }

        _ = shutdown_signal.cancelled() => {
            match tokio::time::timeout(Duration::from_millis(5_000), tasks).await {
                Err(_) => {
                    error!("Graceful shutdown timed out, force-terminating...",);
                    process::exit(1);
                }

                Ok(res) => res,
            }
        },
    };

    log_errors(&res);
}

fn wait_for_shutdown_signal() -> CancellationToken {
    let shutdown_signal = tokio_util::sync::CancellationToken::new();

    let serving = shutdown_signal.clone();
    tokio::spawn(async move {
        let sigint = async {
            signal(SignalKind::interrupt()).expect("failed to install sigint handler").recv().await;
        };

        let sigterm = async {
            signal(SignalKind::terminate())
                .expect("failed to install sigterm handler")
                .recv()
                .await;
        };

        tokio::select! {
            _ = sigint => {},
            _ = sigterm => {},
        }

        info!("Shutdown signal received, stopping...");

        serving.cancel();
    });

    shutdown_signal
}

fn log_errors(res: &[Result<Result<(), Error>, tokio::task::JoinError>]) {
    for res in res.iter() {
        match res {
            Err(err) => error!(error = &err.to_string(), "General failure"),
            Ok(Err(err)) => error!(error = &err.to_string(), "General failure"),
            Ok(Ok(_)) => {}
        }
    }
}
