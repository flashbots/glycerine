use backoff::ExponentialBackoff;
use socket2::Protocol;
use tokio::{self, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::{
    bootstrap::Bootstrap,
    cli::host::CliHost,
    proxy::{
        Error,
        worker::{
            ipv4::{VsockToIpv4, VsockToIpv4Config},
            nfq::{NfqToVsock, NfqToVsockConfig},
        },
    },
    system::{ipv4::set_ephemeral_ports, netfilter::add_ingress_netfilter_packets_sink},
};

// ---------------------------------------------------------------------

const SERVICE: &str = "host";

// ---------------------------------------------------------------------

pub async fn run(
    cfg: &CliHost,
    backoff: ExponentialBackoff,
    shutdown_signal: CancellationToken,
) -> Result<Vec<JoinHandle<Result<(), Error>>>, Error> {
    setup(cfg).await?;

    if cfg.setup_only {
        info!(service = SERVICE, "Setup-only requested, terminating...");
        return Err(Error::GlycerineShutdown);
    }

    let bootstrap = {
        let server = Bootstrap::new(cfg, backoff.clone())?;
        let shutdown_signal = shutdown_signal.clone();
        tokio::task::spawn_blocking(move || -> Result<(), Error> {
            server.run(shutdown_signal.clone()).inspect_err(|err| {
                error!(
                    error = err.to_string(),
                    "Fatal error while running bootstrap service, terminating..."
                );
                shutdown_signal.cancel();
            })
        })
    };

    let ingress = {
        let proxy = NfqToVsock::new(
            NfqToVsockConfig {
                netfilter_queue_num: cfg.ingress_netfilter_queue_num,
                forward_address: cfg.ingress_vsock_address.clone(),
            },
            backoff.clone(),
        );
        let shutdown_signal = shutdown_signal.clone();
        tokio::task::spawn_blocking(move || -> Result<(), Error> {
            proxy.run(shutdown_signal.clone()).inspect_err(|err| {
                error!(
                    error = err.to_string(),
                    "Fatal error while running ingress proxy, terminating..."
                );
                shutdown_signal.cancel();
            })
        })
    };

    let egress = {
        let proxy = VsockToIpv4::new(
            VsockToIpv4Config {
                listen_address: cfg.egress_vsock_address.clone(),
                peer_cid: cfg
                    .ingress_vsock_address
                    .as_vsock_address()
                    .expect("ingress_vsock_address is always a vsock address")
                    .0,
                ipv4_sink_address: cfg.egress_ipv4_sink_address.clone(),
                ipv4_interface: cfg.interface.clone(),
            },
            backoff.clone(),
        );
        let shutdown_signal = shutdown_signal.clone();
        tokio::task::spawn_blocking(move || -> Result<(), Error> {
            proxy.run(shutdown_signal.clone()).inspect_err(|err| {
                error!(
                    error = err.to_string(),
                    "Fatal error while running egress proxy, terminating..."
                );
                shutdown_signal.cancel();
            })
        })
    };

    Ok(vec![bootstrap, ingress, egress])
}

async fn setup(cfg: &CliHost) -> Result<(), Error> {
    ensure_no_port_overlaps(cfg)?;

    set_ephemeral_ports(cfg.host_ephemeral_ports.0, cfg.host_ephemeral_ports.1)
        .inspect(|set| {
            if *set {
                info!(
                    service = SERVICE,
                    from = cfg.host_ephemeral_ports.0,
                    to = cfg.host_ephemeral_ports.1,
                    "Set ephemeral port range",
                );
            } else {
                debug!(
                    service = SERVICE,
                    from = cfg.host_ephemeral_ports.0,
                    to = cfg.host_ephemeral_ports.1,
                    "Ephemeral port range is already configured",
                );
            }
        })
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = err.to_string(),
                from = cfg.host_ephemeral_ports.0,
                to = cfg.host_ephemeral_ports.1,
                "Failed to set ephemeral port range",
            );
        })?;

    let mut dst_ports = cfg.enclave_ports.clone();
    dst_ports.push(cfg.enclave_ephemeral_ports);

    for (protocol, ports) in [(Protocol::TCP, &dst_ports), (Protocol::UDP, &dst_ports)] {
        add_ingress_netfilter_packets_sink(
            cfg.ingress_netfilter_queue_num,
            &cfg.interface,
            protocol,
            ports,
        )
        .inspect(|added| {
            if *added {
                info!(
                    service = SERVICE,
                    netfilter_queue_num = cfg.ingress_netfilter_queue_num,
                    interface = cfg.interface,
                    protocol = ?protocol,
                    ports = ?ports,
                    "Configured netfilter queue ruleset",
                );
            } else {
                debug!(
                    service = SERVICE,
                    netfilter_queue_num = cfg.ingress_netfilter_queue_num,
                    interface = cfg.interface,
                    protocol = ?protocol,
                    ports = ?ports,
                    "Netfilter queue ruleset is already configured",
                );
            }
        })
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = err.to_string(),
                netfilter_queue_num = cfg.ingress_netfilter_queue_num,
                interface = cfg.interface,
                protocol = ?protocol,
                ports = ?ports,
                "Failed to configure netfilter queue ruleset",
            );
        })?;
    }

    Ok(())
}

fn ensure_no_port_overlaps(cfg: &CliHost) -> Result<(), Error> {
    let host_ephemeral_ports = cfg.host_ephemeral_ports.0..=cfg.host_ephemeral_ports.1;

    if host_ephemeral_ports.contains(&cfg.enclave_ephemeral_ports.0) ||
        host_ephemeral_ports.contains(&cfg.enclave_ephemeral_ports.1)
    {
        return Err(Error::GlycerineInvalidConfig("host and enclave ephemeral ports overlap"));
    }

    if cfg
        .enclave_ports
        .iter()
        .any(|(from, to)| host_ephemeral_ports.contains(from) || host_ephemeral_ports.contains(to))
    {
        return Err(Error::GlycerineInvalidConfig(
            "enclave service ports overlap with host ephemeral ports",
        ));
    }

    Ok(())
}
