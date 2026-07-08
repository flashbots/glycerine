use std::{fs, io};

use backoff::ExponentialBackoff;
use pnet::datalink;
use socket2::{Protocol, Socket};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::proxy::Error;

// ---------------------------------------------------------------------

pub(crate) const MAX_PACKET_SIZE: usize = 0xFFFF;

const IP_LOCAL_PORT_RANGE: &str = "/proc/sys/net/ipv4/ip_local_port_range";

// ---------------------------------------------------------------------

pub(crate) fn get_interface_address(interface: &str) -> Result<String, Error> {
    Ok(datalink::interfaces()
        .iter()
        .find(|iface| iface.name == interface)
        .ok_or_else(|| {
            Error::IoIpv4Tcp(io::Error::other(format!("unknown interface: {interface}")))
        })?
        .ips
        .iter()
        .find(|addr| addr.is_ipv4())
        .ok_or_else(|| {
            Error::IoIpv4Tcp(io::Error::other(format!(
                "interface has no ipv4 address: {interface}"
            )))
        })?
        .to_string())
}

pub(crate) fn new_sink_socket(
    service: &'static str,
    interface: &str,
    protocol: Protocol,
) -> Result<Socket, Error> {
    debug!(
        service = service,
        interface = interface,
        protocol = ?protocol,
        "Will try to create a new ipv4 sink socket",
    );

    let socket = Socket::new(socket2::Domain::IPV4, socket2::Type::RAW, protocol.into())
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                interface = interface,
                protocol = ?protocol,
                "Failed to create a new ipv4 socket"
            )
        })
        .map_err(Error::IoIpv4Tcp)?;

    socket
        .bind_device(interface.as_bytes().into())
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                interface = interface,
                protocol = ?protocol,
                "Failed to bind ipv4 socket to a device"
            )
        })
        .map_err(Error::IoIpv4Tcp)?;

    // we will be pushing packets verbatim
    socket
        .set_header_included_v4(true)
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                interface = interface,
                protocol = ?protocol,
                "Failed to set IP_HDRINCL on ipv4 socket"
            )
        })
        .map_err(Error::IoIpv4Tcp)?;

    // since we can't connect over this socket, we can't half-close it.
    // therefore, let's at least set the receiving buffer to zero
    socket
        .set_recv_buffer_size(0)
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                interface = interface,
                protocol = ?protocol,
                "Failed to set recv buffer size to zero on ipv4 socket"
            )
        })
        .map_err(Error::IoIpv4Tcp)?;

    debug!(
        service = service,
        interface = interface,
        protocol = ?protocol,
        "Created new ipv4 sink socket",
    );

    Ok(socket)
}

pub(crate) fn new_sink_socket_with_backoff(
    service: &'static str,
    interface: &str,
    protocol: Protocol,
    shutdown_signal: CancellationToken,
    backoff: ExponentialBackoff,
) -> Result<Socket, Error> {
    backoff::retry(backoff, || {
        if shutdown_signal.is_cancelled() {
            return Err(backoff::Error::permanent(Error::GlycerineShutdown));
        }
        new_sink_socket(service, interface, protocol).map_err(
            backoff::Error::transient, // TODO: classify errors?
        )
    })
    .map_err(Error::unwrap_backoff)
}

pub(crate) fn set_ephemeral_ports(from: u16, to: u16) -> Result<bool, Error> {
    let ip_local_port_range = fs::read_to_string(IP_LOCAL_PORT_RANGE).map_err(Error::IoIpv4Tcp)?;

    let mut parts = ip_local_port_range.split_whitespace();

    let prev_from = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing start port"))
        .map_err(Error::IoIpv4Tcp)?
        .parse::<u16>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        .map_err(Error::IoIpv4Tcp)?;

    let prev_to = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing end port"))
        .map_err(Error::IoIpv4Tcp)?
        .parse::<u16>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        .map_err(Error::IoIpv4Tcp)?;

    if from == prev_from && to == prev_to {
        Ok(false)
    } else {
        fs::write(IP_LOCAL_PORT_RANGE, format!("{from} {to}\n"))
            .map_err(Error::IoIpv4Tcp)
            .map(|_| true)
    }
}
