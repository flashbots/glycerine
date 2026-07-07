use std::{ffi::CString, mem, net::Ipv4Addr, os::unix::io::AsRawFd};

use futures::TryStreamExt;
use nix::{
    errno::Errno,
    sys::socket::{AddressFamily, SockFlag, SockType},
};
use pnet::ipnetwork::Ipv4Network;
use rtnetlink::{
    RouteMessageBuilder,
    new_connection,
    packet_route::route::{RouteProtocol, RouteScope, RouteType},
};
use tracing::debug;

use crate::proxy::Error;

// ---------------------------------------------------------------------

const ROUTE_PROTOCOL: RouteProtocol = RouteProtocol::Other(66);

nix::ioctl_read_bad!(ioctl_get_flags, libc::SIOCGIFFLAGS, libc::ifreq);
nix::ioctl_read_bad!(ioctl_get_mtu, libc::SIOCGIFMTU, libc::ifreq);
nix::ioctl_write_ptr_bad!(ioctl_set_flags, libc::SIOCSIFFLAGS, libc::ifreq);
nix::ioctl_write_ptr_bad!(ioctl_set_mtu, libc::SIOCSIFMTU, libc::ifreq);

// ---------------------------------------------------------------------

pub(crate) async fn ensure_default_ipv4_route(
    interface: &str,
    address: Ipv4Network,
) -> Result<(), Error> {
    let (connection, handle, _) = new_connection().map_err(Error::IoNetlink)?;
    tokio::spawn(connection);

    let interface = handle
        .link()
        .get()
        .match_name(interface.into())
        .execute()
        .try_next()
        .await
        .map_err(Error::Rtnetlink)?
        .ok_or(Error::GlycerineUnknownInterface)?
        .header
        .index;

    {
        let mut routes = handle
            .route()
            .get(RouteMessageBuilder::<Ipv4Addr>::new().output_interface(interface).build())
            .execute();
        while let Some(route) = routes.try_next().await.map_err(Error::Rtnetlink)? &&
            route.header.table == libc::RT_TABLE_MAIN &&
            route.header.destination_prefix_length == 0
        {
            debug!(
                table = route.header.table,
                scope = ?route.header.scope,
                kind = ?route.header.kind,
                protocol = ?route.header.protocol,
                address_family = ?route.header.address_family,
                destination_prefix_length = route.header.destination_prefix_length,
                source_prefix_length = route.header.source_prefix_length,
                tos = route.header.tos,
                attributes = ?route.attributes,
                "Deleting a route",
            );
            handle.route().del(route).execute().await.map_err(Error::Rtnetlink)?;
        }
    }

    handle
        .route()
        .add(
            RouteMessageBuilder::<Ipv4Addr>::new()
                .table_id(libc::RT_TABLE_MAIN as u32)
                .scope(RouteScope::Universe)
                .kind(RouteType::Unicast)
                .protocol(ROUTE_PROTOCOL)
                .destination_prefix(Ipv4Addr::UNSPECIFIED, 0)
                .output_interface(interface)
                .pref_source(address.ip())
                .build(),
        )
        .execute()
        .await
        .map_err(Error::Rtnetlink)
}

pub(crate) async fn add_ipv4_interface_address(
    interface: &str,
    address: Ipv4Network,
) -> Result<bool, Error> {
    let (connection, handle, _) = new_connection().map_err(Error::IoNetlink)?;
    tokio::spawn(connection);

    let interface_index = handle
        .link()
        .get()
        .match_name(interface.into())
        .execute()
        .try_next()
        .await
        .map_err(Error::Rtnetlink)?
        .ok_or(Error::GlycerineUnknownInterface)?
        .header
        .index;

    match handle
        .address()
        .add(interface_index, address.ip().into(), address.prefix())
        .execute()
        .await
    {
        Err(rtnetlink::Error::NetlinkError(err)) => match err.code {
            Some(code) => {
                if code.get() == -libc::EEXIST {
                    Ok(false)
                } else {
                    Err(Error::Rtnetlink(rtnetlink::Error::NetlinkError(err)))
                }
            }

            None => Err(Error::Rtnetlink(rtnetlink::Error::NetlinkError(err))),
        },

        Err(err) => Err(Error::Rtnetlink(err)),

        Ok(_) => Ok(true),
    }
}

pub(crate) fn bring_interface_up(interface: &str) -> Result<bool, Error> {
    let interface = CString::new(interface).map_err(|_| Error::Nix(Errno::EINVAL))?;

    let socket =
        nix::sys::socket::socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None)
            .map_err(Error::Nix)?;

    let mut req = new_ifreq(&interface);
    let prev_flags = unsafe {
        ioctl_get_flags(socket.as_raw_fd(), &mut req).map_err(Error::Nix)?;
        req.ifr_ifru.ifru_flags
    };

    if prev_flags & libc::IFF_UP as libc::c_short != 0 {
        return Ok(false); // interface is already up => didn't change
    }

    unsafe {
        req.ifr_ifru.ifru_flags = prev_flags | libc::IFF_UP as libc::c_short;
        ioctl_set_flags(socket.as_raw_fd(), &req).map_err(Error::Nix)?;
    }

    Ok(true) // interface was down => set it up
}

pub(crate) fn get_mtu(interface: &str) -> Result<i32, Error> {
    let interface = CString::new(interface).map_err(|_| Error::Nix(Errno::EINVAL))?;

    let socket =
        nix::sys::socket::socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None)
            .map_err(Error::Nix)?;

    let mut req = new_ifreq(&interface);

    unsafe {
        ioctl_get_mtu(socket.as_raw_fd(), &mut req).map_err(Error::Nix)?;
        Ok(req.ifr_ifru.ifru_mtu)
    }
}

pub(crate) fn set_mtu(interface: &str, mtu: i32) -> Result<bool, Error> {
    let interface = CString::new(interface).map_err(|_| Error::Nix(Errno::EINVAL))?;

    let socket =
        nix::sys::socket::socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None)
            .map_err(Error::Nix)?;

    let prev_mtu = {
        let mut req = new_ifreq(&interface);
        unsafe {
            ioctl_get_mtu(socket.as_raw_fd(), &mut req).map_err(Error::Nix)?;
            req.ifr_ifru.ifru_mtu
        }
    };

    if mtu == prev_mtu {
        return Ok(false); // such MTU is already set => didn't change
    }

    let mut req = new_ifreq(&interface);
    unsafe {
        req.ifr_ifru.ifru_mtu = mtu as libc::c_int;
        ioctl_set_mtu(socket.as_raw_fd(), &req).map_err(Error::Nix)?;
    }

    Ok(true) // MTU was different => set a new value
}

fn new_ifreq(interface: &CString) -> libc::ifreq {
    let mut req: libc::ifreq = unsafe { mem::zeroed() };
    for (dst, src) in req.ifr_name.iter_mut().zip(interface.as_bytes()) {
        *dst = *src as libc::c_char;
    }
    req
}
