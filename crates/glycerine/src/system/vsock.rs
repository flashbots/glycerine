use backoff::ExponentialBackoff;
use socket2::{SockAddr, Socket};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::display_sock_addr;
use crate::proxy::Error;

// ---------------------------------------------------------------------

pub(crate) fn accept_connection(
    service: &'static str,
    socket: &Socket,
    peer_cid: u32,
) -> Result<(Socket, SockAddr), Error> {
    let listen_address = socket
        .local_addr()
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                "Failed to get a local address of vsock socket",
            )
        })
        .map_err(Error::IoVsock)?;

    debug!(
        service = service,
        listen_address = display_sock_addr(&listen_address),
        "Waiting for a connection on vsock socket"
    );

    let (connection_socket, peer_address) = socket
        .accept()
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                listen_address = display_sock_addr(&listen_address),
                "Failed to accept a connection on vsock socket",
            );
        })
        .map_err(Error::IoVsock)?;

    if peer_address.as_vsock_address().expect("peer_address is always a vsock").0 != peer_cid {
        warn!(
            service = service,
            expected_cid = peer_cid,
            peer_address = display_sock_addr(&peer_address),
            listen_address = display_sock_addr(&listen_address),
            "Rejecting connection attempt from unexpected CID"
        );
        let _ = connection_socket.shutdown(std::net::Shutdown::Both);
        return Err(Error::GlycerineInvalidConfig("unexpected cid"));
    }

    info!(
        service = service,
        peer_address = display_sock_addr(&peer_address),
        listen_address = display_sock_addr(&listen_address),
        "Accepted a connection on vsock socket"
    );

    Ok((connection_socket, peer_address))
}

pub(crate) fn accept_connection_with_backoff(
    service: &'static str,
    socket: &Socket,
    peer_cid: u32,
    shutdown_signal: CancellationToken,
    backoff: ExponentialBackoff,
) -> Result<(Socket, SockAddr), Error> {
    backoff::retry(backoff, || {
        if shutdown_signal.is_cancelled() {
            return Err(backoff::Error::permanent(Error::GlycerineShutdown));
        }
        accept_connection(service, socket, peer_cid).map_err(
            backoff::Error::transient, // TODO: classify errors?
        )
    })
    .map_err(Error::unwrap_backoff)
}

pub(crate) fn new_client_socket(
    service: &'static str,
    server_address: &SockAddr,
) -> Result<Socket, Error> {
    debug!(
        service = service,
        server_address = display_sock_addr(server_address),
        "Connecting to vsock socket...",
    );

    let socket = Socket::new(socket2::Domain::VSOCK, socket2::Type::STREAM, None)
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                server_address = display_sock_addr(server_address),
                "Failed to create a new vsock socket"
            )
        })
        .map_err(Error::IoVsock)?;

    socket
        .connect(server_address)
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                server_address = display_sock_addr(server_address),
                "Failed to connect over vsock socket"
            )
        })
        .map_err(Error::IoVsock)?;

    let local_address = socket
        .local_addr()
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                "Failed to get a local address of vsock socket",
            )
        })
        .map_err(Error::IoVsock)?;

    info!(
        service = service,
        server_address = display_sock_addr(server_address),
        local_address = display_sock_addr(&local_address),
        "Connected to vsock socket",
    );

    Ok(socket)
}

pub(crate) fn new_client_socket_with_backoff(
    service: &'static str,
    server_address: &SockAddr,
    shutdown_signal: CancellationToken,
    backoff: ExponentialBackoff,
) -> Result<Socket, Error> {
    backoff::retry(backoff, || {
        if shutdown_signal.is_cancelled() {
            return Err(backoff::Error::permanent(Error::GlycerineShutdown));
        }
        new_client_socket(service, server_address).map_err(
            backoff::Error::transient, // TODO: classify errors?
        )
    })
    .map_err(Error::unwrap_backoff)
}

pub(crate) fn new_listener_socket(
    service: &'static str,
    listen_address: &SockAddr,
) -> Result<Socket, Error> {
    debug!(
        service = service,
        listen_address = display_sock_addr(listen_address),
        "Setting up listening on vsock socket..."
    );

    let socket = Socket::new(socket2::Domain::VSOCK, socket2::Type::STREAM, None)
        .inspect_err(|err| {
            warn!(
                error = &err.to_string(),
                service = service,
                listen_address = display_sock_addr(listen_address),
                "Failed to create a new vsock socket"
            )
        })
        .map_err(Error::IoVsock)?;

    socket
        .bind(listen_address)
        .inspect_err(|err| {
            warn!(
                error = &err.to_string(),
                service = service,
                listen_address = display_sock_addr(listen_address),
                "Failed to bind vsock socket to an address"
            )
        })
        .map_err(Error::IoVsock)?;

    socket
        .listen(16)
        .inspect_err(|err| {
            warn!(
                error = &err.to_string(),
                service = service,
                listen_address = display_sock_addr(listen_address),
                "Failed to listen on vsock socket"
            )
        })
        .map_err(Error::IoVsock)?;

    info!(
        service = service,
        listen_address = display_sock_addr(listen_address),
        "Listening on vsock socket...",
    );

    Ok(socket)
}

pub(crate) fn new_listener_socket_with_backoff(
    service: &'static str,
    listen_address: &SockAddr,
    shutdown_signal: CancellationToken,
    backoff: ExponentialBackoff,
) -> Result<Socket, Error> {
    backoff::retry(backoff, || {
        if shutdown_signal.is_cancelled() {
            return Err(backoff::Error::permanent(Error::GlycerineShutdown));
        }
        new_listener_socket(service, listen_address).map_err(
            backoff::Error::transient, // TODO: classify errors?
        )
    })
    .map_err(Error::unwrap_backoff)
}

pub(crate) fn shutdown(
    service: &'static str,
    socket: &Socket,
    how: std::net::Shutdown,
) -> Result<(), Error> {
    let listen_address = socket
        .local_addr()
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                "Failed to get a local address of vsock socket",
            )
        })
        .map_err(Error::IoVsock)?;

    socket
        .shutdown(how)
        .inspect_err(|err| {
            warn!(
                service = service,
                error = &err.to_string(),
                how = ?how,
                listen_address = display_sock_addr(&listen_address),
                "Failed to shutdown vsock socket",
            );
        })
        .map_err(Error::IoVsock)
}
