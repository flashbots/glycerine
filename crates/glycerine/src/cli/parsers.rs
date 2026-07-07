use std::{
    ffi::OsStr,
    fmt::Display,
    net::{Ipv4Addr, SocketAddrV4},
};

use clap::{Arg, Command, builder::TypedValueParser, error::ErrorKind};
use pnet::datalink::{self};
use tracing_subscriber::EnvFilter;

// Ipv4AddressParser ---------------------------------------------------

#[derive(Clone)]
pub(crate) struct Ipv4AddressParser {}

impl TypedValueParser for Ipv4AddressParser {
    type Value = socket2::SockAddr;

    fn parse_ref(
        &self,
        cmd: &Command,
        arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value = value.to_str().ok_or_else(|| clap_error(cmd, arg, "", "invalid utf-8"))?;

        let (addr, port) =
            value.split_once(':').ok_or_else(|| clap_error(cmd, arg, value, "missing colon"))?;

        let addr: Ipv4Addr = addr
            .parse()
            .map_err(|err| clap_error(cmd, arg, value, format!("invalid ipv4 address: {err}")))?;

        let port = port
            .parse::<u16>()
            .map_err(|err| clap_error(cmd, arg, value, format!("invalid port: {err}")))?;

        Ok(SocketAddrV4::new(addr, port).into())
    }
}

// LogLevelParser ------------------------------------------------------

#[derive(Clone)]
pub(crate) struct LogLevelParser {}

impl TypedValueParser for LogLevelParser {
    type Value = EnvFilter;

    fn parse_ref(
        &self,
        cmd: &Command,
        arg: Option<&Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value = value.to_str().ok_or_else(|| clap_error(cmd, arg, "", "invalid utf-8"))?;

        EnvFilter::builder().parse(value).map_err(|err| clap_error(cmd, arg, value, err))
    }
}

// NetworkInterfaceParser ----------------------------------------------

#[derive(Clone)]
pub(crate) struct NetworkInterfaceParser {}

impl TypedValueParser for NetworkInterfaceParser {
    type Value = String;

    fn parse_ref(
        &self,
        cmd: &Command,
        arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value = value.to_str().ok_or_else(|| clap_error(cmd, arg, "", "invalid utf-8"))?;

        datalink::interfaces()
            .iter()
            .find(|iface| iface.name == value)
            .ok_or_else(|| clap_error(cmd, arg, value, "interface does't exist"))
            .map(|iface| iface.name.clone())
    }
}

// PortRangeParser -----------------------------------------------------

#[derive(Clone)]
pub(crate) struct PortRangeParser {}

impl TypedValueParser for PortRangeParser {
    type Value = (u16, u16);

    fn parse_ref(
        &self,
        cmd: &Command,
        arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value = value.to_str().ok_or_else(|| clap_error(cmd, arg, "", "invalid utf-8"))?;

        let parts: Vec<_> = value.split("-").collect();

        if parts.len() > 2 {
            return Err(clap_error(cmd, arg, value, "invalid port range"));
        }

        match parts.len() {
            1 => {
                let port = parts[0]
                    .parse::<u16>()
                    .map_err(|err| clap_error(cmd, arg, value, format!("invalid port: {err}")))?;
                Ok((port, port))
            }

            2 => {
                let port0 = parts[0]
                    .parse::<u16>()
                    .map_err(|err| clap_error(cmd, arg, value, format!("invalid port: {err}")))?;
                let port1 = parts[1]
                    .parse::<u16>()
                    .map_err(|err| clap_error(cmd, arg, value, format!("invalid port: {err}")))?;
                if port0 < port1 { Ok((port0, port1)) } else { Ok((port1, port0)) }
            }

            _ => Err(clap_error(cmd, arg, value, "invalid port range")),
        }
    }
}

// SocketAddressParser -------------------------------------------------

#[derive(Clone)]
pub(crate) struct SocketAddressParser {}

impl TypedValueParser for SocketAddressParser {
    type Value = socket2::SockAddr;

    fn parse_ref(
        &self,
        cmd: &Command,
        arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value = value.to_str().ok_or_else(|| clap_error(cmd, arg, "", "invalid utf-8"))?;

        let (cid, port) =
            value.split_once(':').ok_or_else(|| clap_error(cmd, arg, value, "missing colon"))?;

        let cid = {
            let cid = cid
                .parse::<u32>()
                .map_err(|err| clap_error(cmd, arg, value, format!("invalid cid: {err}")))?;
            if cid == 0 { 0xFFFFFFFF } else { cid }
        };

        let port = port
            .parse::<u32>()
            .map_err(|err| clap_error(cmd, arg, value, format!("invalid port: {err}")))?;

        Ok(socket2::SockAddr::vsock(cid, port))
    }
}

// helpers -------------------------------------------------------------

fn clap_error<E>(cmd: &Command, arg: Option<&Arg>, value: &str, err: E) -> clap::Error
where
    E: Display,
{
    match (value, arg) {
        ("", None) => clap::Error::raw(
            ErrorKind::ValueValidation,
            format!("invalid value for one of the arguments: {}\n", err),
        )
        .with_cmd(cmd),

        ("", Some(arg)) => clap::Error::raw(
            ErrorKind::ValueValidation,
            format!("invalid value for `{}`: {}\n", arg, err),
        )
        .with_cmd(cmd),

        (value, None) => clap::Error::raw(
            ErrorKind::ValueValidation,
            format!("invalid value `{}` for one of the arguments: {}\n", value, err),
        )
        .with_cmd(cmd),

        (value, Some(arg)) => clap::Error::raw(
            ErrorKind::ValueValidation,
            format!("invalid value `{}` for `{}`: {}\n", value, arg, err),
        )
        .with_cmd(cmd),
    }
}

pub(crate) fn get_default_network_interface() -> Option<String> {
    datalink::interfaces()
        .iter()
        .find(|iface| {
            !iface.is_loopback() &&
                iface.is_up() &&
                iface.ips.iter().any(|ip| !ip.ip().is_loopback())
        })
        .map(|iface| iface.name.clone())
}
