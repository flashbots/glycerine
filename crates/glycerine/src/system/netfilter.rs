use std::{ffi::CString, net::Ipv4Addr};

use backoff::ExponentialBackoff;
use nfq::Queue;
use nftnl::{
    Batch,
    Chain,
    ChainType,
    Hook,
    MsgType,
    Policy,
    ProtoFamily,
    Rule,
    Table,
    nft_expr,
    nftnl_sys::libc,
};
use socket2::Protocol;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::{
    proxy::Error,
    system::nftnl::{NfQueue, send_and_process},
};

// ---------------------------------------------------------------------

const OUTPUT: &std::ffi::CStr = c"OUTPUT";
const PREROUTING: &std::ffi::CStr = c"PREROUTING";

// ---------------------------------------------------------------------

pub(crate) fn add_ingress_netfilter_packets_sink(
    queue_num: u16,
    interface: &str,
    protocol: Protocol,
    dst_port_ranges: &[(u16, u16)],
) -> Result<bool, Error> {
    let table_cstr = match protocol {
        Protocol::TCP => Ok(CString::new(format!("glycerine-tcp-{queue_num}"))),
        Protocol::UDP => Ok(CString::new(format!("glycerine-udp-{queue_num}"))),
        _ => Err(Error::UnsupportedProtocol(protocol)),
    }?
    .map_err(Error::CStringNulError)?;

    /*
    TODO: when/if we can check the configuration of the table too:

    if get_tables().map_err(Error::IoNftnl)?.contains(&table_cstr) {
        return Ok(false); // such table already exists => didn't add
    };
    */

    let interface_cstr = CString::new(interface).map_err(Error::CStringNulError)?;

    let protocol = match protocol {
        Protocol::TCP => Ok(libc::IPPROTO_TCP),
        Protocol::UDP => Ok(libc::IPPROTO_UDP),
        _ => unreachable!(), // safety: already checked above
    }
    .map_err(Error::IoNftnl)?;

    let ruleset = {
        let mut batch = Batch::new();

        let table = Table::new(table_cstr.as_c_str(), ProtoFamily::Ipv4);
        batch.add(&table, MsgType::Add); // add-remove to reset whatever pre-exists
        batch.add(&table, MsgType::Del);
        batch.add(&table, MsgType::Add); // add a fresh one

        let mut chain = Chain::new(PREROUTING, &table);
        chain.set_type(ChainType::Filter);
        chain.set_hook(Hook::PreRouting, libc::NF_IP_PRI_MANGLE);
        chain.set_policy(Policy::Accept);
        batch.add(&chain, MsgType::Add);

        // `nftnl` does not expose interval-set literals
        // therefore, add separate rule per range
        for (from, to) in dst_port_ranges.iter() {
            let mut rule = Rule::new(&chain);

            // interface name
            rule.add_expr(&nft_expr!(meta iifname));
            rule.add_expr(&nft_expr!(cmp == interface_cstr.as_c_str()));

            // protocol
            rule.add_expr(&nft_expr!(meta l4proto));
            rule.add_expr(&nft_expr!(cmp == protocol as u8));

            // protocol + destination ports
            match protocol {
                libc::IPPROTO_TCP => {
                    rule.add_expr(&nft_expr!(payload tcp dport));
                    rule.add_expr(&nftnl::expr::Cmp::new(
                        nftnl::expr::CmpOp::Gte,
                        &from.to_be_bytes()[..],
                    ));
                    rule.add_expr(&nft_expr!(payload tcp dport));
                    rule.add_expr(&nftnl::expr::Cmp::new(
                        nftnl::expr::CmpOp::Lte,
                        &to.to_be_bytes()[..],
                    ));
                }

                libc::IPPROTO_UDP => {
                    rule.add_expr(&nft_expr!(payload udp dport));
                    rule.add_expr(&nftnl::expr::Cmp::new(
                        nftnl::expr::CmpOp::Gte,
                        &from.to_be_bytes()[..],
                    ));
                    rule.add_expr(&nft_expr!(payload udp dport));
                    rule.add_expr(&nftnl::expr::Cmp::new(
                        nftnl::expr::CmpOp::Lte,
                        &to.to_be_bytes()[..],
                    ));
                }

                _ => unreachable!(), // safety: caller only uses tcp & udp
            }

            // destination queue number
            rule.add_expr(&NfQueue { num: queue_num });

            batch.add(&rule, MsgType::Add);
        }

        batch.finalize()
    };

    send_and_process(&ruleset).map_err(Error::IoNftnl).map(|_| true) // such table didn't exist => added
}

pub(crate) fn add_egress_netfilter_packets_sink(
    queue_num: u16,
    protocol: Protocol,
    src_address: Ipv4Addr,
    src_port_ranges: &[(u16, u16)],
) -> Result<bool, Error> {
    let table_cstr = match protocol {
        Protocol::TCP => Ok(CString::new(format!("glycerine-tcp-{queue_num}"))),
        Protocol::UDP => Ok(CString::new(format!("glycerine-udp-{queue_num}"))),
        _ => Err(Error::UnsupportedProtocol(protocol)),
    }?
    .map_err(Error::CStringNulError)?;

    let protocol = match protocol {
        Protocol::TCP => Ok(libc::IPPROTO_TCP),
        Protocol::UDP => Ok(libc::IPPROTO_UDP),
        _ => unreachable!(), // safety: already checked above
    }
    .map_err(Error::IoNftnl)?;

    let ruleset = {
        let mut batch = Batch::new();

        let table = Table::new(table_cstr.as_c_str(), ProtoFamily::Ipv4);
        batch.add(&table, MsgType::Add); // add-remove to reset whatever pre-exists
        batch.add(&table, MsgType::Del);
        batch.add(&table, MsgType::Add); // add a fresh one

        let mut chain = Chain::new(OUTPUT, &table);
        chain.set_type(ChainType::Filter);
        chain.set_hook(Hook::Out, libc::NF_IP_PRI_FILTER);
        chain.set_policy(Policy::Accept);
        batch.add(&chain, MsgType::Add);

        // `nftnl` does not expose interval-set literals
        // therefore, add separate rule per range
        for (from, to) in src_port_ranges.iter() {
            let mut rule = Rule::new(&chain);

            // protocol
            rule.add_expr(&nft_expr!(meta l4proto));
            rule.add_expr(&nft_expr!(cmp == protocol));

            // source address
            rule.add_expr(&nft_expr!(payload ipv4 saddr));
            rule.add_expr(&nft_expr!(cmp == src_address));

            // protocol + source port
            match protocol {
                libc::IPPROTO_TCP => {
                    rule.add_expr(&nft_expr!(payload tcp sport));
                    rule.add_expr(&nftnl::expr::Cmp::new(
                        nftnl::expr::CmpOp::Gte,
                        &from.to_be_bytes()[..],
                    ));
                    rule.add_expr(&nft_expr!(payload tcp sport));
                    rule.add_expr(&nftnl::expr::Cmp::new(
                        nftnl::expr::CmpOp::Lte,
                        &to.to_be_bytes()[..],
                    ));
                }

                libc::IPPROTO_UDP => {
                    rule.add_expr(&nft_expr!(payload udp sport));
                    rule.add_expr(&nftnl::expr::Cmp::new(
                        nftnl::expr::CmpOp::Gte,
                        &from.to_be_bytes()[..],
                    ));
                    rule.add_expr(&nft_expr!(payload udp sport));
                    rule.add_expr(&nftnl::expr::Cmp::new(
                        nftnl::expr::CmpOp::Lte,
                        &to.to_be_bytes()[..],
                    ));
                }

                _ => unreachable!(), // safety: already checked above
            }

            // queue number
            rule.add_expr(&NfQueue { num: queue_num });

            batch.add(&rule, MsgType::Add);
        }

        batch.finalize()
    };

    send_and_process(&ruleset).map_err(Error::IoNftnl).map(|_| true) // such table didn't exist => added
}

pub(crate) fn new_queue(service: &'static str, queue_num: u16) -> Result<Queue, Error> {
    debug!(service = service, queue_num = queue_num, "Subscribing to netfilter queue...");

    let mut queue = Queue::open()
        .inspect_err(|err| {
            warn!(
                error = &err.to_string(),
                service = service,
                queue_num = queue_num,
                "Failed to open a netfilter queue"
            )
        })
        .map_err(Error::IoNfq)?;

    // TODO: check if we need to set queue.set_queue_max_len

    queue
        .bind(queue_num)
        .inspect_err(|err| {
            warn!(
                error = &err.to_string(),
                service = service,
                queue_num = queue_num,
                "Failed to bind to a netfilter queue"
            )
        })
        .map_err(Error::IoNfq)?;

    queue
        .set_recv_enobufs(true)
        .inspect_err(|err| {
            warn!(
                error = &err.to_string(),
                service = service,
                queue_num = queue_num,
                "Failed to unset NETLINK_NO_ENOBUFS on a netfilter queue"
            )
        })
        .map_err(Error::IoNfq)?;

    debug!(service = service, queue_num = queue_num, "Subscribed to netfilter queue");

    Ok(queue)
}

pub(crate) fn new_queue_with_backoff(
    service: &'static str,
    queue_num: u16,
    shutdown_signal: CancellationToken,
    backoff: ExponentialBackoff,
) -> Result<Queue, Error> {
    backoff::retry(backoff, || {
        if shutdown_signal.is_cancelled() {
            return Err(backoff::Error::permanent(Error::GlycerineShutdown));
        }
        new_queue(service, queue_num).map_err(
            backoff::Error::transient, // TODO: classify errors?
        )
    })
    .map_err(Error::unwrap_backoff)
}
