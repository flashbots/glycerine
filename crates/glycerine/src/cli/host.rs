use clap::{ArgAction, Args};
use socket2::SockAddr;

use crate::cli::{
    ENV,
    parsers::{
        Ipv4AddressParser,
        NetworkInterfaceParser,
        PortRangeParser,
        SocketAddressParser,
        get_default_network_interface,
    },
};

// CliHost -------------------------------------------------------------

#[derive(Args, Clone, Debug)]
pub struct CliHost {
    /// proxied network interface
    #[arg(
        default_value = get_default_network_interface().unwrap_or_default(),
        env = format!("{ENV}_INTERFACE"),
        help_heading = "network",
        long("interface"),
        name("interface"),
        value_name = "device",
        value_parser = NetworkInterfaceParser{},
    )]
    pub(crate) interface: String,

    /// ports used by enclave to accept the connections on
    ///
    /// must not overlap with --enclave-ephemeral-ports or
    /// --host-ephemeral-ports
    #[arg(
        action = ArgAction::Append,
        env = format!("{ENV}_ENCLAVE_PORTS"),
        help_heading = "network",
        long("enclave-ports"),
        name("enclave_ports"),
        value_name = "port ranges",
        value_parser = PortRangeParser{},
    )]
    pub(crate) enclave_ports: Vec<(u16, u16)>,

    /// ports used by enclave to initiate the connections from
    ///
    /// must not overlap with --enclave-ports or --host-ephemeral-ports
    #[arg(
        default_value = "40000-60999",
        env = format!("{ENV}_ENCLAVE_EPHEMERAL_PORTS"),
        help_heading = "network",
        long("enclave-ephemeral-ports"),
        name("enclave_ephemeral_ports"),
        value_name = "port ranges",
        value_parser = PortRangeParser{},
    )]
    pub(crate) enclave_ephemeral_ports: (u16, u16),

    /// ports used by host to initiate the connections from
    ///
    /// must not overlap with --enclave-ports or --enclave-ephemeral-ports
    #[arg(
        default_value = "32768-39999",
        env = format!("{ENV}_HOST_EPHEMERAL_PORTS"),
        help_heading = "network",
        long("host-ephemeral-ports"),
        name("host_ephemeral_ports"),
        value_name = "port ranges",
        value_parser = PortRangeParser{},
    )]
    pub(crate) host_ephemeral_ports: (u16, u16),

    /// vsock address to run enclave proxy bootstrap service on
    ///
    /// proxy will listen on this socket and communicate bootstrap
    /// information over to the enclave proxy when it connects
    #[arg(
        env = format!("{ENV}_BOOTSTRAP_VSOCK_ADDRESS"),
        help_heading = "bootstrap",
        long("bootstrap-vsock-address"),
        name("bootstrap_vsock_address"),
        value_name = "host-cid:port",
        value_parser = SocketAddressParser{},
    )]
    pub(crate) bootstrap_vsock_address: SockAddr,

    /// only configure networking (netfilter queues, rules, etc) and quit
    #[arg(
        default_value = "false",
        help_heading = "bootstrap",
        long("setup-only"),
        name("setup_only")
    )]
    pub(crate) setup_only: bool,

    /// netfilter queue number to use for ingress
    ///
    /// proxy will create a netfilter queue with this number, configure
    /// netfilter rules to route network packets with matching ports to this
    /// queue, subscribe to it, and forward packets received this way to
    /// the vsock connection with an enclave
    #[arg(
        env = format!("{ENV}_INGRESS_NETFILTER_QUEUE"),
        help_heading = "ingress",
        long("ingress_netfilter-queue-number"),
        name("ingress_netfilter_queue_number"),
        value_name = "number",
    )]
    pub(crate) ingress_netfilter_queue_num: u16,

    /// vsock address to use for ingress
    ///
    /// proxy will be sending packets received from netfilter queue to this
    /// socket
    #[arg(
        env = format!("{ENV}_INGRESS_VSOCK_ADDRESS"),
        help_heading = "ingress",
        long("ingress-vsock-address"),
        name("ingress_vsock_address"),
        value_name = "enclave-cid:port",
        value_parser = SocketAddressParser{},
    )]
    pub(crate) ingress_vsock_address: SockAddr,

    /// vsock address to receive egress IP packets from enclave on
    ///
    /// proxy will listen on this socket, receive IP packets sent there by
    /// an enclave proxy, and "dump" them into the configured network
    /// interface
    #[arg(
        env = format!("{ENV}_EGRESS_VSOCK_ADDRESS"),
        help_heading = "egress",
        long("egress-vsock-address"),
        name("egress_vsock_address"),
        value_name = "host-cid:port",
        value_parser = SocketAddressParser{},
    )]
    pub(crate) egress_vsock_address: SockAddr,

    /// egress sink ipv4 address
    ///
    /// this is an arbitrary IP address used with sendto() syscall; real
    /// destination IP addresses come from packets' headers
    #[arg(
        default_value = "1.1.1.1:1111",
        env = format!("{ENV}_EGRESS_IPV4_SINK_ADDRESS"),
        help_heading = "egress",
        long("egress-ipv4-sink-address"),
        name("egress_ipv4_sink_address"),
        value_name = "ipv4",
        value_parser = Ipv4AddressParser{},
    )]
    pub(crate) egress_ipv4_sink_address: SockAddr,
}
