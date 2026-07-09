use clap::Args;
use socket2::SockAddr;

use crate::cli::{
    ENV,
    parsers::{NetworkInterfaceParser, SocketAddressParser},
};

// CliEnclave ----------------------------------------------------------

#[derive(Args, Clone, Debug)]
pub struct CliEnclave {
    /// proxied network interface
    #[arg(
        default_value = "lo",
        env = format!("{ENV}_INTERFACE"),
        help_heading = "network",
        long("interface"),
        name("interface"),
        value_name = "device",
        value_parser = NetworkInterfaceParser{},
    )]
    pub(crate) interface: String,

    /// vsock address of the bootstrap service on host vm
    ///
    /// proxy will query bootstrap information from this address
    #[arg(
        env = format!("{ENV}_BOOTSTRAP_VSOCK_ADDRESS"),
        help_heading = "bootstrap",
        long("bootstrap-vsock-address"),
        name("bootstrap_vsock_address"),
        value_name = "host-cid:port",
        value_parser = SocketAddressParser{},
    )]
    pub(crate) bootstrap_vsock_address: SockAddr,

    /// vsock address to use for ingress
    ///
    /// proxy will listen on this socket, receive IP packets sent there by
    /// the host proxy, and "dump" them into the configured network
    /// interface

    #[arg(
        env = format!("{ENV}_INGRESS_VSOCK_ADDRESS"),
        help_heading = "ingress",
        long("ingress-vsock-address"),
        name("ingress_vsock_address"),
        value_name = "enclave-cid:port",
        value_parser = SocketAddressParser{},
    )]
    pub(crate) ingress_vsock_address: SockAddr,

    /// netfilter queue number to use for egress
    ///
    /// proxy will create a netfilter queue with this number, configure
    /// netfilter rules to route network packets to this queue, subscribe to
    /// it, and forward packets received this way to the vsock connection
    /// with the host vm
    #[arg(
        env = format!("{ENV}_EGRESS_NETFILTER_QUEUE"),
        help_heading = "egress",
        long("egress_netfilter-queue-number"),
        name("egress_netfilter_queue_number"),
        value_name = "number",
    )]
    pub(crate) egress_netfilter_queue_num: u16,

    /// max length for egress netfilter queue
    #[arg(
        default_value = "16384",
        env = format!("{ENV}_EGRESS_NETFILTER_QUEUE_MAX_LEN"),
        help_heading = "egress",
        long("egress_netfilter-queue-max-len"),
        name("egress_netfilter_queue_max_len"),
        value_name = "number",
    )]
    pub(crate) egress_netfilter_queue_max_len: u32,

    /// vsock address to send the egress IP packets from an enclave to
    ///
    /// proxy will be sending packets received from netfilter queue to this
    /// socket
    #[arg(
        env = format!("{ENV}_EGRESS_VSOCK_ADDRESS"),
        help_heading = "egress",
        long("egress-vsock-address"),
        name("egress_vsock_address"),
        value_name = "host-cid:port",
        value_parser = SocketAddressParser{},
    )]
    pub(crate) egress_vsock_address: SockAddr,

    /// only configure networking (netfilter queues, rules, etc) and quit
    #[arg(
        default_value = "false",
        help_heading = "bootstrap",
        long("setup-only"),
        name("setup_only")
    )]
    pub(crate) setup_only: bool,
}
