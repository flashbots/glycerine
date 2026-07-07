use std::net::SocketAddrV4;

use backoff::ExponentialBackoff;
use pnet::ipnetwork::Ipv4Network;
use socket2::Protocol;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::{
    bootstrap,
    cli::enclave::CliEnclave,
    proxy::{
        Error,
        worker::{
            ipv4::{VsockToIpv4, VsockToIpv4Config},
            nfq::{NfqToVsock, NfqToVsockConfig},
        },
    },
    system::{
        interface::{
            add_ipv4_interface_address,
            bring_interface_up,
            ensure_default_ipv4_route,
            set_mtu,
        },
        ipv4::set_ephemeral_ports,
        netfilter::add_egress_netfilter_packets_sink,
    },
};

// ---------------------------------------------------------------------

const SERVICE: &str = "enclave";

// ---------------------------------------------------------------------

pub async fn run(
    cfg: &CliEnclave,
    backoff: ExponentialBackoff,
    shutdown_signal: CancellationToken,
) -> Result<Vec<JoinHandle<Result<(), Error>>>, Error> {
    let bootstrap = bootstrap::get_record(
        &cfg.bootstrap_vsock_address,
        shutdown_signal.clone(),
        backoff.clone(),
    )?;

    setup(cfg, &bootstrap).await?;

    if cfg.setup_only {
        info!(service = SERVICE, "Setup-only requested, terminating...");
        return Err(Error::GlycerineShutdown);
    }

    let ingress = {
        let proxy = VsockToIpv4::new(
            VsockToIpv4Config {
                listen_address: cfg.ingress_vsock_address.clone(),
                ipv4_sink_address: SocketAddrV4::new(
                    // ip address
                    bootstrap
                        .interface
                        .ipv4_network_address
                        .parse::<Ipv4Network>()
                        .map_err(Error::IpNetworkError)?
                        .ip(),
                    // port
                    0,
                )
                .into(),
                peer_cid: cfg
                    .egress_vsock_address
                    .as_vsock_address()
                    .expect("egress_vsock_address is always a vsock address")
                    .0,
                ipv4_interface: cfg.interface.clone(),
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
        let proxy = NfqToVsock::new(
            NfqToVsockConfig {
                netfilter_queue_num: cfg.egress_netfilter_queue_num,
                forward_address: cfg.egress_vsock_address.clone(),
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

    Ok(vec![ingress, egress])
}

async fn setup(cfg: &CliEnclave, bootstrap: &bootstrap::Record) -> Result<(), Error> {
    let ipv4_network_address = bootstrap
        .interface
        .ipv4_network_address
        .parse::<Ipv4Network>()
        .map_err(Error::IpNetworkError)?;

    bring_interface_up(&cfg.interface)
        .inspect(|set| {
            if *set {
                info!(
                    service = SERVICE,
                    interface = &cfg.interface,
                    "Brought network interface up"
                );
            } else {
                debug!(
                    service = SERVICE,
                    interface = &cfg.interface,
                    "Network interface is already up"
                );
            }
        })
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = err.to_string(),
                interface = &cfg.interface,
                "Failed to bring network interface up"
            );
        })?;

    add_ipv4_interface_address(&cfg.interface, ipv4_network_address)
        .await
        .inspect(|added| {
            if *added {
                info!(
                    service = SERVICE,
                    ipv4_network_address = ipv4_network_address.to_string(),
                    interface = &cfg.interface,
                    "Added IPv4 address to an interface",
                );
            } else {
                debug!(
                    service = SERVICE,
                    ipv4_network_address = ipv4_network_address.to_string(),
                    interface = &cfg.interface,
                    "IPv4 address is already assigned to the interface",
                );
            }
        })
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = err.to_string(),
                ipv4_network_address = ipv4_network_address.to_string(),
                interface = &cfg.interface,
                "Failed to add IPv4 address to an interface",
            );
        })?;

    ensure_default_ipv4_route(&cfg.interface, ipv4_network_address)
        .await
        .inspect(|_| {
            info!(
                service = SERVICE,
                ipv4_network_address = ipv4_network_address.to_string(),
                interface = &cfg.interface,
                "Ensured default IPv4 route",
            );
        })
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = err.to_string(),
                ipv4_network_address = ipv4_network_address.to_string(),
                interface = &cfg.interface,
                "Failed to add default IPv4 route",
            );
        })?;

    set_mtu(&cfg.interface, bootstrap.interface.mtu)
        .inspect(|set| {
            if *set {
                info!(
                    service = SERVICE,
                    mtu = bootstrap.interface.mtu,
                    interface = &cfg.interface,
                    "Set network interface MTU size"
                );
            } else {
                debug!(
                    service = SERVICE,
                    mtu = bootstrap.interface.mtu,
                    interface = &cfg.interface,
                    "Network interface MTU size is already set"
                );
            }
        })
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = err.to_string(),
                mtu = bootstrap.interface.mtu,
                interface = &cfg.interface,
                "Failed to set network interface MTU size"
            );
        })?;

    set_ephemeral_ports(bootstrap.ports.ephemeral.from, bootstrap.ports.ephemeral.to)
        .inspect(|set| {
            if *set {
                info!(
                    service = SERVICE,
                    from = bootstrap.ports.ephemeral.from,
                    to = bootstrap.ports.ephemeral.to,
                    "Set ephemeral port range",
                );
            } else {
                debug!(
                    service = SERVICE,
                    from = bootstrap.ports.ephemeral.from,
                    to = bootstrap.ports.ephemeral.to,
                    "Ephemeral port range is already configured",
                );
            }
        })
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = err.to_string(),
                from = bootstrap.ports.ephemeral.from,
                to = bootstrap.ports.ephemeral.to,
                "Failed to set ephemeral port range",
            );
        })?;

    let src_ports = {
        let mut src_ports: Vec<_> =
            bootstrap.ports.service.iter().map(|range| (range.from, range.to)).collect();
        src_ports.push((bootstrap.ports.ephemeral.from, bootstrap.ports.ephemeral.to));
        src_ports
    };

    for (protocol, ports) in [(Protocol::TCP, &src_ports), (Protocol::UDP, &src_ports)] {
        add_egress_netfilter_packets_sink(
            cfg.egress_netfilter_queue_num,
            protocol,
            ipv4_network_address.ip(),
            ports,
        )
        .inspect(|added| {
            if *added {
                info!(
                    service = SERVICE,
                    netfilter_queue_num = cfg.egress_netfilter_queue_num,
                    protocol = ?protocol,
                    ports = ?ports,
                    "Configured netfilter queue ruleset",
                );
            } else {
                debug!(
                    service = SERVICE,
                    netfilter_queue_num = cfg.egress_netfilter_queue_num,
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
                netfilter_queue_num = cfg.egress_netfilter_queue_num,
                protocol = ?protocol,
                ports = ?ports,
                "Failed to configure netfilter queue ruleset",
            );
        })?;
    }

    Ok(())
}
