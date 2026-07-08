use std::{
    io,
    io::{Read, Write},
    sync::Arc,
    time::Duration,
};

use backoff::ExponentialBackoff;
use serde::{Deserialize, Serialize};
use socket2::SockAddr;
use tokio_util::{bytes::Bytes, sync::CancellationToken};
use tracing::{debug, error, info, warn};

use crate::{
    cli::host::CliHost,
    proxy::Error,
    system::{
        display_sock_addr,
        interface::get_mtu,
        ipv4::get_interface_address,
        vsock::{
            accept_connection_with_backoff,
            new_client_socket_with_backoff,
            new_listener_socket_with_backoff,
            shutdown,
        },
    },
};

// ---------------------------------------------------------------------

const SERVICE: &str = "bootstrap";
const MAX_RECORD_SIZE: usize = 1 << 20; // 1Mb

// Bootstrap -----------------------------------------------------------

pub(crate) struct Bootstrap {
    listen_address: SockAddr,
    peer_cid: u32,
    backoff: ExponentialBackoff,
    info: Arc<Bytes>,
}

impl Bootstrap {
    pub fn new(cfg: &CliHost, backoff: ExponentialBackoff) -> Result<Self, Error> {
        let info = Arc::new(Bytes::from_owner(
            serde_json::to_string(&Record::new(cfg)?).map_err(Error::Json)?,
        ));

        let (peer_cid, _) = cfg
            .ingress_vsock_address
            .as_vsock_address()
            .expect("ingress_vsock_address is always a vsock address");

        Ok(Self { listen_address: cfg.bootstrap_vsock_address.clone(), peer_cid, backoff, info })
    }

    pub fn run(&self, shutdown_signal: CancellationToken) -> Result<(), Error> {
        let vsock_listener_socket = new_listener_socket_with_backoff(
            SERVICE,
            &self.listen_address,
            shutdown_signal.clone(),
            self.backoff.clone(),
        )?;

        let new_client_vsock_socket = || {
            debug!(
                service = SERVICE,
                listen_address = display_sock_addr(&self.listen_address),
                "Waiting for new connection...",
            );

            let (socket, peer_address) = accept_connection_with_backoff(
                SERVICE,
                &vsock_listener_socket,
                self.peer_cid,
                shutdown_signal.clone(),
                self.backoff.clone(),
            )?;

            // we will only be writing data into this socket => let's half-close
            shutdown(SERVICE, &socket, std::net::Shutdown::Read)?;

            socket
                .set_write_timeout(Some(Duration::from_millis(1000)))
                .inspect_err(|err| {
                    error!(
                        service = SERVICE,
                        error = &err.to_string(),
                        listen_address = display_sock_addr(&self.listen_address),
                        peer_address = display_sock_addr(&peer_address),
                        "Failed to set write-timeout on vsock socket",
                    )
                })
                .map_err(Error::IoVsock)?;

            Ok((socket, peer_address))
        };

        while !shutdown_signal.is_cancelled() {
            let (mut client_vsock_socket, client_peer_address) = new_client_vsock_socket()?;

            match self.serve(
                &mut client_vsock_socket,
                &client_peer_address,
                shutdown_signal.clone(),
            ) {
                // keep serving
                Ok(_) | Err(Error::IoVsock(_)) => {}

                // exit
                res => return res,
            }
        }

        Ok(())
    }

    fn serve(
        &self,
        socket: &mut socket2::Socket,
        peer_address: &SockAddr,
        shutdown_signal: CancellationToken,
    ) -> Result<(), Error> {
        debug!(
            service = SERVICE,
            peer_address = display_sock_addr(peer_address),
            listen_address = display_sock_addr(&self.listen_address),
            bytes = self.info.len(),
            "Sending bootstrap info...",
        );

        let mut pos: usize = 0;

        while pos < self.info.len() && !shutdown_signal.is_cancelled() {
            pos += socket
                .write(&self.info[pos..])
                .and_then(|wrote| {
                    if wrote > 0 {
                        Ok(wrote)
                    } else {
                        Err(io::Error::new(io::ErrorKind::WriteZero, "wrote zero bytes"))
                    }
                })
                .inspect_err(|err| {
                    warn!(
                        service = SERVICE,
                        error = &err.to_string(),
                        peer_address = display_sock_addr(peer_address),
                        listen_address = display_sock_addr(&self.listen_address),
                        "Failed to write bootstrap info into the vsock stream",
                    );
                })
                .map_err(Error::IoVsock)?;
        }

        info!(
            service = SERVICE,
            peer_address = display_sock_addr(peer_address),
            listen_address = display_sock_addr(&self.listen_address),
            bytes = pos,
            "Sent bootstrap record",
        );

        Ok(())
    }
}

// Record --------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub interface: RecordInterface,
    pub(crate) ports: RecordPorts,
}

impl Record {
    fn new(cfg: &CliHost) -> Result<Self, Error> {
        Ok(Self {
            interface: RecordInterface::new(&cfg.interface)?,

            ports: RecordPorts {
                ephemeral: RecordPortRange {
                    from: cfg.enclave_ephemeral_ports.0,
                    to: cfg.enclave_ephemeral_ports.1,
                },

                service: cfg
                    .enclave_ports
                    .iter()
                    .map(|p| RecordPortRange { from: p.0, to: p.1 })
                    .collect(),
            },
        })
    }
}

// RecordInterface -----------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RecordInterface {
    pub ipv4_network_address: String,
    pub mtu: i32,
}

impl RecordInterface {
    fn new(interface: &str) -> Result<Self, Error> {
        debug!(interface = interface, "Resolving bootstrap info for interface...");

        let ipv4_network_address = get_interface_address(interface)
            .inspect(|ipv4_address| {
                debug!(
                    ipv4_address = ipv4_address,
                    interface = interface,
                    "Resolved interface's IPv4 address"
                )
            })
            .inspect_err(|err| {
                error!(
                    error = err.to_string(),
                    interface = interface,
                    "Failed to get interface's IPv4 address",
                )
            })?;

        let mtu = get_mtu(interface)
            .inspect(|mtu| {
                debug!(mtu = mtu, interface = interface, "Resolved interface's MTU size")
            })
            .inspect_err(|err| {
                error!(
                    error = err.to_string(),
                    interface = interface,
                    "Failed to get interface's MTU size"
                )
            })?;

        Ok(Self { ipv4_network_address, mtu })
    }
}

// RecordPorts ---------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RecordPorts {
    pub(crate) ephemeral: RecordPortRange,
    pub(crate) service: Vec<RecordPortRange>,
}

// RecordPortRange -----------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RecordPortRange {
    pub(crate) from: u16,
    pub(crate) to: u16,
}

// utils ---------------------------------------------------------------

pub(crate) fn get(
    bootstrap_address: &SockAddr,
    shutdown_signal: CancellationToken,
    backoff: ExponentialBackoff,
) -> Result<Record, Error> {
    debug!(
        service = SERVICE,
        bootstrap_address = display_sock_addr(bootstrap_address),
        "Receiving bootstrap record...",
    );

    let mut bootstrap_socket = new_client_socket_with_backoff(
        SERVICE,
        bootstrap_address,
        shutdown_signal.clone(),
        backoff,
    )?;

    // we will only be reading data from this socket => let's half-close
    shutdown(SERVICE, &bootstrap_socket, std::net::Shutdown::Write)?;

    let local_address = bootstrap_socket
        .local_addr()
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = &err.to_string(),
                bootstrap_address = display_sock_addr(bootstrap_address),
                "Failed to get a local address of vsock socket",
            )
        })
        .map_err(Error::IoVsock)?;

    bootstrap_socket
        .set_read_timeout(Some(Duration::from_millis(1000)))
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = &err.to_string(),
                bootstrap_address = display_sock_addr(bootstrap_address),
                local_address = display_sock_addr(&local_address),
                "Failed to set read-timeout on vsock socket",
            )
        })
        .map_err(Error::IoVsock)?;

    let mut buf = vec![0u8; 4096];
    let mut read = 0;

    while !shutdown_signal.is_cancelled() {
        // ensure there's always room to read into; otherwise a full
        // buffer makes read() return Ok(0), which is indistinguishable
        // from a genuine EOF and would silently truncate the record
        if read == buf.len() {
            if buf.len() >= MAX_RECORD_SIZE {
                return Err(Error::IoVsock(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "bootstrap record exceeds maximum size",
                )));
            }
            buf.resize((buf.len() * 2).min(MAX_RECORD_SIZE), 0);
        }

        let count = bootstrap_socket
            .read(&mut buf[read..])
            .inspect_err(|err| {
                error!(
                    service = SERVICE,
                    error = err.to_string(),
                    bootstrap_address = display_sock_addr(bootstrap_address),
                    local_address = display_sock_addr(&local_address),
                    "Failed to read bootstrap record",
                );
            })
            .map_err(Error::IoVsock)?;

        if count == 0 {
            break; // real EOF: peer half-closed after sending the full record
        }
        read += count;
    }

    serde_json::from_slice(&buf[..read])
        .inspect_err(|err| {
            error!(
                service = SERVICE,
                error = err.to_string(),
                bootstrap_address = display_sock_addr(bootstrap_address),
                local_address = display_sock_addr(&local_address),
                bytes = read,
                "Failed to parse bootstrap record",
            );
        })
        .map_err(Error::Json)
        .inspect(|_: &Record| {
            info!(
                service = SERVICE,
                bootstrap_address = display_sock_addr(bootstrap_address),
                bytes = read,
                "Received bootstrap record",
            );
        })
}
