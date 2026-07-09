use std::{io, io::Read, net::Shutdown};

use backoff::ExponentialBackoff;
use socket2::{Protocol, SockAddr};
use tokio_util::sync::CancellationToken;
use tracing::{trace, warn};

use super::super::Error;
use crate::{
    proxy::worker::debug::DumpPayload,
    system::{
        display_sock_addr,
        ipv4::{MAX_PACKET_SIZE, new_sink_socket_with_backoff},
        vsock::{accept_connection_with_backoff, new_listener_socket_with_backoff, shutdown},
    },
};

// ---------------------------------------------------------------------

const SERVICE: &str = "vsock->ipv4";

// VsockToIpv4Config ---------------------------------------------------

pub struct VsockToIpv4Config {
    pub listen_address: SockAddr,
    pub peer_cid: u32,
    pub ipv4_sink_address: SockAddr,
    pub ipv4_interface: String,
}

// VsockToIpv4 ---------------------------------------------------------

pub struct VsockToIpv4 {
    cfg: VsockToIpv4Config,
    backoff: ExponentialBackoff,
}

impl VsockToIpv4 {
    pub fn new(cfg: VsockToIpv4Config, backoff: ExponentialBackoff) -> Self {
        Self { cfg, backoff }
    }

    pub fn run(&self, shutdown_signal: CancellationToken) -> Result<(), Error> {
        let new_src_vsock_socket = {
            let shutdown_signal = shutdown_signal.clone();

            let vsock_listener_socket = new_listener_socket_with_backoff(
                SERVICE,
                &self.cfg.listen_address,
                shutdown_signal.clone(),
                self.backoff.clone(),
            )?;

            move || {
                let (new_src_vsock_socket, _) = accept_connection_with_backoff(
                    SERVICE,
                    &vsock_listener_socket,
                    self.cfg.peer_cid,
                    shutdown_signal.clone(),
                    self.backoff.clone(),
                )?;
                // we will only be reading data from this socket => let's half-close
                shutdown(SERVICE, &new_src_vsock_socket, Shutdown::Write)?;
                Ok(new_src_vsock_socket)
            }
        };

        let new_dst_ipv4_tcp_socket = || {
            new_sink_socket_with_backoff(
                SERVICE,
                &self.cfg.ipv4_interface,
                Protocol::TCP,
                shutdown_signal.clone(),
                self.backoff.clone(),
            )
        };

        let new_dst_ipv4_udp_socket = || {
            new_sink_socket_with_backoff(
                SERVICE,
                &self.cfg.ipv4_interface,
                Protocol::UDP,
                shutdown_signal.clone(),
                self.backoff.clone(),
            )
        };

        let mut src_vsock_socket = new_src_vsock_socket()?;
        let mut dst_ipv4_tcp_socket = new_dst_ipv4_tcp_socket()?;
        let mut dst_ipv4_udp_socket = new_dst_ipv4_udp_socket()?;

        while !shutdown_signal.is_cancelled() {
            match self.pump(
                &mut src_vsock_socket,
                &mut dst_ipv4_tcp_socket,
                &mut dst_ipv4_udp_socket,
                shutdown_signal.clone(),
            ) {
                // recover vsock socket
                Err(Error::IoVsock(_)) => {
                    drop(src_vsock_socket);
                    src_vsock_socket = new_src_vsock_socket()?;
                }

                // recover ipv4 tcp socket
                Err(Error::IoIpv4Tcp(_)) => {
                    drop(dst_ipv4_tcp_socket);
                    dst_ipv4_tcp_socket = new_dst_ipv4_tcp_socket()?;
                }

                // recover ipv4 udp socket
                Err(Error::IoIpv4Udp(_)) => {
                    drop(dst_ipv4_udp_socket);
                    dst_ipv4_udp_socket = new_dst_ipv4_udp_socket()?;
                }

                // exit
                res => return res,
            }
        }

        Ok(())
    }

    fn pump(
        &self,
        src_vsock_socket: &mut socket2::Socket,
        dst_ipv4_tcp_socket: &mut socket2::Socket,
        dst_ipv4_udp_socket: &mut socket2::Socket,
        shutdown_signal: CancellationToken,
    ) -> Result<(), Error> {
        let mut payload = vec![0u8; MAX_PACKET_SIZE].into_boxed_slice();

        while !shutdown_signal.is_cancelled() {
            // read fixed-sized portion of the header
            src_vsock_socket
                .read_exact(&mut payload[0..20])
                .inspect_err(|err| {
                    warn!(
                        service = SERVICE,
                        error = &err.to_string(),
                        listen_address = display_sock_addr(&self.cfg.listen_address),
                        "Failed to read a packet from vsock connection",
                    )
                })
                .map_err(Error::IoVsock)?;

            let (payload_size, protocol) = ipv4_total_len(
                payload[0..20]
                    .try_into()
                    .map_err(|_| Error::IoVsock(io::Error::from(io::ErrorKind::InvalidData)))?,
            )?;

            // read the rest
            src_vsock_socket
                .read_exact(&mut payload[20..payload_size])
                .inspect_err(|err| {
                    warn!(
                        service = SERVICE,
                        error = &err.to_string(),
                        listen_address = display_sock_addr(&self.cfg.listen_address),
                        "Failed to read a packet from vsock connection",
                    )
                })
                .map_err(Error::IoVsock)?;

            let dst_ipv4_socket = match protocol {
                Protocol::TCP => &dst_ipv4_tcp_socket,
                Protocol::UDP => &dst_ipv4_udp_socket,
                _ => {
                    warn!(
                        service = SERVICE,
                        protocol = ?protocol,
                        listen_address = display_sock_addr(&self.cfg.listen_address),
                        "Received a packet for unsupported protocol",
                    );
                    continue;
                }
            };

            let err_wrapper = match protocol {
                Protocol::TCP => |err: io::Error| Error::IoIpv4Tcp(err),
                Protocol::UDP => |err: io::Error| Error::IoIpv4Udp(err),
                _ => unreachable!(), // safety: already checked above
            };

            let mut sent = 0;
            while sent < payload_size {
                sent += dst_ipv4_socket
                    .send_to(&payload[sent..payload_size], &self.cfg.ipv4_sink_address)
                    .and_then(|sent| {
                        if sent > 0 {
                            Ok(sent)
                        } else {
                            Err(io::Error::new(io::ErrorKind::WriteZero, "sent zero bytes"))
                        }
                    })
                    .inspect_err(|err| {
                        warn!(
                            service = SERVICE,
                            error = &err.to_string(),
                            ipv4_sink_address = display_sock_addr(&self.cfg.ipv4_sink_address),
                            ipv4_interface = self.cfg.ipv4_interface,
                            ipv4_protocol = ?protocol,
                            "Failed to send a payload to ipv4 socket"
                        )
                    })
                    .map_err(err_wrapper)?;
            }

            trace!("{}", DumpPayload(&payload[..payload_size]));
        }

        Ok(())
    }
}

fn ipv4_total_len(header: &[u8; 20]) -> Result<(usize, Protocol), Error> {
    if header[0] >> 4 != 4 {
        return Err(Error::GlycerineRuntime("invalid ipv4 packet"));
    }

    let header_len = 4 * (header[0] & 0x0f) as usize;
    if header_len < 20 {
        return Err(Error::GlycerineRuntime("invalid ipv4 packet"));
    }

    let packet_len =
        u16::from_be_bytes(header[2..4].try_into().expect("must always convert")) as usize;
    if packet_len < header_len || MAX_PACKET_SIZE < packet_len {
        return Err(Error::GlycerineRuntime("invalid ipv4 packet"));
    }

    let protocol = Protocol::from(header[9] as i32);

    Ok((packet_len, protocol))
}
