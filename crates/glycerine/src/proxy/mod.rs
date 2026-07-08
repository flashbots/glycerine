use socket2::Protocol;

pub mod enclave;
pub mod host;

pub(crate) mod worker;

// Error ---------------------------------------------------------------

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("cstring-nul-error: {0}")]
    CStringNulError(#[source] std::ffi::NulError),

    #[error("glycerine: invalid ipv4 packet")]
    GlycerineInvalidIpv4Packet,

    #[error("glycerine: shutdown")]
    GlycerineShutdown,

    #[error("glycerine: unexpected peer cid")]
    GlycerineUnexpectedCid,

    #[error("glycerine: unknown interface")]
    GlycerineUnknownInterface,

    #[error("io-ipv4-tcp: {0}")]
    IoIpv4Tcp(#[source] std::io::Error),

    #[error("io-ipv4-udp: {0}")]
    IoIpv4Udp(#[source] std::io::Error),

    #[error("io-netlink: {0}")]
    IoNetlink(#[source] std::io::Error),

    #[error("io-nfq: {0}")]
    IoNfq(#[source] std::io::Error),

    #[error("io-nftnl: {0}")]
    IoNftnl(#[source] std::io::Error),

    #[error("io-vsock: {0}")]
    IoVsock(#[source] std::io::Error),

    #[error("network: {0}")]
    IpNetworkError(#[source] pnet::ipnetwork::IpNetworkError),

    #[error("json: {0}")]
    Json(#[source] serde_json::Error),

    #[error("nix: {0}")]
    Nix(#[source] nix::Error),

    #[error("netlink: {0}")]
    Rtnetlink(#[source] rtnetlink::Error),

    #[error("unsupported-protocol: {0:?}")]
    UnsupportedProtocol(Protocol),
}

impl Error {
    pub(crate) fn unwrap_backoff<E>(err: backoff::Error<E>) -> E {
        match err {
            backoff::Error::Permanent(err) => err,
            backoff::Error::Transient { err, retry_after: _ } => err,
        }
    }
}
