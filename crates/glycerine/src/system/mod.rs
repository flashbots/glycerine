pub(crate) mod interface;
pub(crate) mod ipv4;
pub(crate) mod netfilter;
pub(crate) mod vsock;

mod nftnl;

// ---------------------------------------------------------------------

use socket2::SockAddr;

// ---------------------------------------------------------------------

pub(crate) fn display_sock_addr(addr: &SockAddr) -> String {
    if let Some(inet) = addr.as_socket() {
        return inet.to_string();
    }

    if let Some((cid, port)) = addr.as_vsock_address() {
        return if cid == 0xFFFFFFFF { format!("0:{port}") } else { format!("{cid}:{port}") };
    }

    if let Some(unix) = addr.as_pathname() {
        return unix.to_string_lossy().to_string();
    }

    format!("{addr:?}")
}
