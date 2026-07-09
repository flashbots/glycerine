use std::{io, net::Shutdown};

use backoff::ExponentialBackoff;
use tokio_util::sync::CancellationToken;
use tracing::{trace, warn};

use super::super::Error;
use crate::{
    proxy::worker::debug::DumpPayload,
    system::{
        display_sock_addr,
        netfilter::new_queue_with_backoff,
        vsock::{new_client_socket_with_backoff, shutdown},
    },
};

// ---------------------------------------------------------------------

const SERVICE: &str = "nfq->vsock";

// NfqToVsockConfig ----------------------------------------------------

pub struct NfqToVsockConfig {
    pub netfilter_queue_num: u16,
    pub netfilter_queue_max_len: u32,
    pub forward_address: socket2::SockAddr,
}

// NfqToVsock ----------------------------------------------------------

pub struct NfqToVsock {
    cfg: NfqToVsockConfig,
    backoff: ExponentialBackoff,
}

impl NfqToVsock {
    pub fn new(cfg: NfqToVsockConfig, backoff: ExponentialBackoff) -> Self {
        Self { cfg, backoff }
    }

    pub fn run(&self, shutdown_signal: CancellationToken) -> Result<(), Error> {
        let new_src_netfilter_queue = || {
            new_queue_with_backoff(
                SERVICE,
                self.cfg.netfilter_queue_num,
                self.cfg.netfilter_queue_max_len,
                shutdown_signal.clone(),
                self.backoff.clone(),
            )
        };

        let new_dst_vsock_socket = || {
            let new_dst_vsock_socket = new_client_socket_with_backoff(
                SERVICE,
                &self.cfg.forward_address,
                shutdown_signal.clone(),
                self.backoff.clone(),
            )?;
            // we will only be pushing data over this socket => let's half-close
            shutdown(SERVICE, &new_dst_vsock_socket, Shutdown::Read)?;
            Ok(new_dst_vsock_socket)
        };

        let mut src_netfilter_queue = new_src_netfilter_queue()?;
        let mut dst_vsock_socket = new_dst_vsock_socket()?;

        while !shutdown_signal.is_cancelled() {
            match self.pump(
                &mut src_netfilter_queue,
                &mut dst_vsock_socket,
                shutdown_signal.clone(),
            ) {
                // recover nfq
                Err(Error::IoNfq(_)) => {
                    drop(src_netfilter_queue);
                    src_netfilter_queue = new_src_netfilter_queue()?;
                }

                Err(Error::IoVsock(_)) => dst_vsock_socket = new_dst_vsock_socket()?,

                // exit
                res => return res,
            }
        }

        Ok(())
    }

    fn pump(
        &self,
        src_netfilter_queue: &mut nfq::Queue,
        dst_vsock_socket: &mut socket2::Socket,
        shutdown_signal: CancellationToken,
    ) -> Result<(), Error> {
        while !shutdown_signal.is_cancelled() {
            let mut message = match src_netfilter_queue.recv() {
                Err(err) if err.raw_os_error() == Some(libc::ENOBUFS) => {
                    // TODO: when metrics are added, count occurrences of this
                    warn!(
                        service = SERVICE,
                        error = &err.to_string(),
                        netfilter_queue_num = self.cfg.netfilter_queue_num,
                        "Netfilter queue overflow, continuing..."
                    );
                    continue;
                }

                Err(err) => {
                    warn!(
                        service = SERVICE,
                        error = &err.to_string(),
                        netfilter_queue_num = self.cfg.netfilter_queue_num,
                        "Failed to read a message from netfilter queue"
                    );
                    return Err(Error::IoNfq(err));
                }

                Ok(message) => message,
            };
            message.set_verdict(nfq::Verdict::Drop);

            let payload = message.get_payload();
            let payload_size = payload.len();

            let mut sent = 0;
            while sent < payload_size {
                sent += dst_vsock_socket
                    .send(&payload[sent..payload_size])
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
                            forward_address = display_sock_addr(&self.cfg.forward_address),
                            "Failed to send a message to vsock connection"
                        )
                    })
                    .map_err(Error::IoVsock)?;
            }

            trace!("{}", DumpPayload(payload));

            src_netfilter_queue
                .verdict(message)
                .inspect_err(|err| {
                    warn!(
                        service = SERVICE,
                        error = &err.to_string(),
                        netfilter_queue_num = self.cfg.netfilter_queue_num,
                        "Failed to verdict a message from netfilter queue"
                    )
                })
                .map_err(Error::IoNfq)?;
        }

        Ok(())
    }
}
