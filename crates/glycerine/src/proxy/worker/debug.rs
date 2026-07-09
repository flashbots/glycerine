use std::{fmt, net::Ipv4Addr};

// DumpPayload ---------------------------------------------------------

pub(super) struct DumpPayload<'a>(pub(super) &'a [u8]);

impl fmt::Display for DumpPayload<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(packet) = Ipv4Packet::parse(self.0) else {
            return write!(f, "IP truncated, length {}", self.0.len());
        };

        if packet.version != 4 {
            return write!(f, "IP{} packet, length {}", packet.version, self.0.len());
        }

        if packet.total_len < packet.header_len {
            return write!(
                f,
                "IP {} > {}: invalid total length {}",
                packet.src, packet.dst, packet.total_len,
            );
        }

        if self.0.len() < packet.total_len {
            return write!(
                f,
                "IP {} > {}: truncated-ip - {} bytes missing",
                packet.src,
                packet.dst,
                packet.total_len - self.0.len(),
            );
        }

        let payload = &self.0[packet.header_len..packet.total_len];
        match packet.protocol {
            IPPROTO_TCP => write_tcp(f, &packet, payload),
            IPPROTO_UDP => write_udp(f, &packet, payload),
            protocol => write!(
                f,
                "IP {} > {}: ip-proto-{}, length {}",
                packet.src,
                packet.dst,
                protocol,
                payload.len(),
            ),
        }
    }
}

// Ipv4Packet -----------------------------------------------------------

struct Ipv4Packet {
    version: u8,
    header_len: usize,
    total_len: usize,
    protocol: u8,
    src: Ipv4Addr,
    dst: Ipv4Addr,
}

impl Ipv4Packet {
    fn parse(packet: &[u8]) -> Option<Self> {
        if packet.len() < IPV4_MIN_HEADER_LEN {
            return None;
        }

        let version = packet[0] >> 4;
        let header_len = 4 * (packet[0] & 0x0f) as usize;
        if header_len < IPV4_MIN_HEADER_LEN || packet.len() < header_len {
            return None;
        }

        Some(Self {
            version,
            header_len,
            total_len: u16::from_be_bytes(packet[2..4].try_into().expect("must always convert"))
                as usize,
            protocol: packet[9],
            src: Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]),
            dst: Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]),
        })
    }
}

// TCP ------------------------------------------------------------------

fn write_tcp(f: &mut fmt::Formatter<'_>, packet: &Ipv4Packet, payload: &[u8]) -> fmt::Result {
    if payload.len() < TCP_MIN_HEADER_LEN {
        return write!(
            f,
            "IP {} > {}: TCP, truncated, length {}",
            packet.src,
            packet.dst,
            payload.len(),
        );
    }

    let header_len = 4 * (payload[12] >> 4) as usize;
    if header_len < TCP_MIN_HEADER_LEN || payload.len() < header_len {
        return write!(
            f,
            "IP {} > {}: TCP, truncated, length {}",
            packet.src,
            packet.dst,
            payload.len(),
        );
    }

    let src_port = u16::from_be_bytes(payload[0..2].try_into().expect("must always convert"));
    let dst_port = u16::from_be_bytes(payload[2..4].try_into().expect("must always convert"));
    let seq = u32::from_be_bytes(payload[4..8].try_into().expect("must always convert"));
    let ack = u32::from_be_bytes(payload[8..12].try_into().expect("must always convert"));
    let flags = payload[13];
    let window = u16::from_be_bytes(payload[14..16].try_into().expect("must always convert"));
    let urgent = u16::from_be_bytes(payload[18..20].try_into().expect("must always convert"));
    let data_len = payload.len() - header_len;

    write!(
        f,
        "IP {}.{} > {}.{}: Flags [{}]",
        packet.src,
        src_port,
        packet.dst,
        dst_port,
        TcpFlags(flags),
    )?;

    if data_len > 0 {
        write!(f, ", seq {}:{}", seq, seq.wrapping_add(data_len as u32))?;
    } else if flags & (TCP_FIN | TCP_SYN | TCP_RST) != 0 {
        write!(f, ", seq {seq}")?;
    }

    if flags & TCP_ACK != 0 {
        write!(f, ", ack {ack}")?;
    }

    write!(f, ", win {window}")?;

    if flags & TCP_URG != 0 {
        write!(f, ", urg {urgent}")?;
    }

    if header_len > TCP_MIN_HEADER_LEN {
        write!(f, ", options [{}]", TcpOptions(&payload[TCP_MIN_HEADER_LEN..header_len]))?;
    }

    write!(f, ", length {data_len}")
}

struct TcpFlags(u8);

impl fmt::Display for TcpFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let flags = [
            (TCP_FIN, "F"),
            (TCP_SYN, "S"),
            (TCP_RST, "R"),
            (TCP_PSH, "P"),
            (TCP_ACK, "."),
            (TCP_URG, "U"),
            (TCP_ECE, "E"),
            (TCP_CWR, "W"),
        ];

        let mut rendered = false;
        for (flag, label) in flags {
            if self.0 & flag != 0 {
                f.write_str(label)?;
                rendered = true;
            }
        }

        if !rendered {
            f.write_str("none")?;
        }

        Ok(())
    }
}

struct TcpOptions<'a>(&'a [u8]);

impl fmt::Display for TcpOptions<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        let mut options = self.0;

        while let Some(kind) = options.first().copied() {
            if !first {
                f.write_str(",")?;
            }
            first = false;

            match kind {
                TCPOPT_EOL => {
                    f.write_str("eol")?;
                    break;
                }
                TCPOPT_NOP => {
                    f.write_str("nop")?;
                    options = &options[1..];
                }
                _ if options.len() < 2 => {
                    write!(f, "unknown-{} malformed", kind)?;
                    break;
                }
                _ => {
                    let option_len = options[1] as usize;
                    if option_len < 2 || options.len() < option_len {
                        write!(f, "unknown-{} malformed", kind)?;
                        break;
                    }

                    let data = &options[2..option_len];
                    match kind {
                        TCPOPT_MSS if data.len() == 2 => {
                            let value =
                                u16::from_be_bytes(data.try_into().expect("must always convert"));
                            write!(f, "mss {value}")?;
                        }
                        TCPOPT_WSCALE if data.len() == 1 => write!(f, "wscale {}", data[0])?,
                        TCPOPT_SACK_PERMITTED if data.is_empty() => f.write_str("sackOK")?,
                        TCPOPT_SACK if data.len().is_multiple_of(8) => {
                            f.write_str("sack")?;
                            for chunk in data.chunks_exact(8) {
                                let left = u32::from_be_bytes(
                                    chunk[0..4].try_into().expect("must always convert"),
                                );
                                let right = u32::from_be_bytes(
                                    chunk[4..8].try_into().expect("must always convert"),
                                );
                                write!(f, " {left}:{right}")?;
                            }
                        }
                        TCPOPT_TIMESTAMP if data.len() == 8 => {
                            let value = u32::from_be_bytes(
                                data[0..4].try_into().expect("must always convert"),
                            );
                            let echo = u32::from_be_bytes(
                                data[4..8].try_into().expect("must always convert"),
                            );
                            write!(f, "TS val {value} ecr {echo}")?;
                        }
                        _ => write!(f, "unknown-{} {}", kind, option_len)?,
                    }

                    options = &options[option_len..];
                }
            }
        }

        Ok(())
    }
}

// UDP ------------------------------------------------------------------

fn write_udp(f: &mut fmt::Formatter<'_>, packet: &Ipv4Packet, payload: &[u8]) -> fmt::Result {
    if payload.len() < UDP_HEADER_LEN {
        return write!(
            f,
            "IP {} > {}: UDP, truncated, length {}",
            packet.src,
            packet.dst,
            payload.len(),
        );
    }

    let src_port = u16::from_be_bytes(payload[0..2].try_into().expect("must always convert"));
    let dst_port = u16::from_be_bytes(payload[2..4].try_into().expect("must always convert"));
    let length = u16::from_be_bytes(payload[4..6].try_into().expect("must always convert"));
    let data_len = length.saturating_sub(UDP_HEADER_LEN as u16);

    write!(
        f,
        "IP {}.{} > {}.{}: UDP, length {}",
        packet.src, src_port, packet.dst, dst_port, data_len,
    )
}

// Constants ------------------------------------------------------------

const IPV4_MIN_HEADER_LEN: usize = 20;
const TCP_MIN_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;

const IPPROTO_TCP: u8 = 6;
const IPPROTO_UDP: u8 = 17;

const TCP_FIN: u8 = 0x01;
const TCP_SYN: u8 = 0x02;
const TCP_RST: u8 = 0x04;
const TCP_PSH: u8 = 0x08;
const TCP_ACK: u8 = 0x10;
const TCP_URG: u8 = 0x20;
const TCP_ECE: u8 = 0x40;
const TCP_CWR: u8 = 0x80;

const TCPOPT_EOL: u8 = 0;
const TCPOPT_NOP: u8 = 1;
const TCPOPT_MSS: u8 = 2;
const TCPOPT_WSCALE: u8 = 3;
const TCPOPT_SACK_PERMITTED: u8 = 4;
const TCPOPT_SACK: u8 = 5;
const TCPOPT_TIMESTAMP: u8 = 8;

// Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_tcp_packet() {
        let payload = [
            0x45, 0x00, 0x00, 0x2c, 0x12, 0x34, 0x40, 0x00, 0x40, 0x06, 0x00, 0x00, 0xc0, 0x00,
            0x02, 0x01, 0xc6, 0x33, 0x64, 0x02, 0x30, 0x39, 0x01, 0xbb, 0x01, 0x02, 0x03, 0x04,
            0x05, 0x06, 0x07, 0x08, 0x50, 0x18, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, b't', b'e',
            b's', b't',
        ];

        assert_eq!(
            DumpPayload(&payload).to_string(),
            "IP 192.0.2.1.12345 > 198.51.100.2.443: Flags [P.], seq 16909060:16909064, ack 84281096, win 16384, length 4",
        );
    }

    #[test]
    fn renders_tcp_options() {
        let payload = [
            0x45, 0x00, 0x00, 0x38, 0x12, 0x34, 0x40, 0x00, 0x40, 0x06, 0x00, 0x00, 0xc0, 0x00,
            0x02, 0x01, 0xc6, 0x33, 0x64, 0x02, 0x30, 0x39, 0x01, 0xbb, 0x01, 0x02, 0x03, 0x04,
            0x00, 0x00, 0x00, 0x00, 0x90, 0x02, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x04,
            0x05, 0xb4, 0x04, 0x02, 0x08, 0x0a, 0x00, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x00,
        ];

        assert_eq!(
            DumpPayload(&payload).to_string(),
            "IP 192.0.2.1.12345 > 198.51.100.2.443: Flags [S], seq 16909060, win 16384, options [mss 1460,sackOK,TS val 42 ecr 0], length 0",
        );
    }

    #[test]
    fn renders_udp_packet() {
        let payload = [
            0x45, 0x00, 0x00, 0x20, 0x12, 0x34, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0x00,
            0x02, 0x01, 0xc6, 0x33, 0x64, 0x02, 0x30, 0x39, 0x00, 0x35, 0x00, 0x0c, 0x00, 0x00,
            b't', b'e', b's', b't',
        ];

        assert_eq!(
            DumpPayload(&payload).to_string(),
            "IP 192.0.2.1.12345 > 198.51.100.2.53: UDP, length 4",
        );
    }

    #[test]
    fn renders_truncated_packet() {
        assert_eq!(DumpPayload(&[0x45, 0x00]).to_string(), "IP truncated, length 2",);
    }
}
