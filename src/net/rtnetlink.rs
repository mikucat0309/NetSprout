//! Just enough rtnetlink to rename an interface by ifindex.

use std::io;
use std::os::fd::OwnedFd;

use rustix::net::netlink::SocketAddrNetlink;
use rustix::net::{
    AddressFamily, RecvFlags, SendFlags, SocketFlags, SocketType, recv, sendto, socket_with,
};

const NLMSG_HDRLEN: usize = 16;
const IFINFOMSG_LEN: usize = 16;
const RTA_HDRLEN: usize = 4;

const NLMSG_ERROR: u16 = 2;
const RTM_NEWLINK: u16 = 16;
const NLM_F_REQUEST: u16 = 0x1;
const NLM_F_ACK: u16 = 0x4;
const IFLA_IFNAME: u16 = 3;

pub struct Socket {
    fd: OwnedFd,
    seq: u32,
}

impl Socket {
    pub fn open() -> io::Result<Self> {
        // Protocol 0 is NETLINK_ROUTE.
        let fd = socket_with(
            AddressFamily::NETLINK,
            SocketType::RAW,
            SocketFlags::CLOEXEC,
            None,
        )?;
        Ok(Socket { fd, seq: 0 })
    }

    /// Rename the interface with `ifindex` to `name`.
    pub fn set_name(&mut self, ifindex: u32, name: &str) -> io::Result<()> {
        self.seq += 1;
        let msg = set_name_request(self.seq, ifindex, name)?;
        sendto(
            &self.fd,
            &msg,
            SendFlags::empty(),
            &SocketAddrNetlink::new(0, 0),
        )?;
        self.recv_ack(self.seq)
    }

    fn recv_ack(&self, seq: u32) -> io::Result<()> {
        let mut buf = [0u8; 4096];
        loop {
            let (len, _) = recv(&self.fd, &mut buf[..], RecvFlags::empty())?;
            if let Some(result) = parse_ack(&buf[..len], seq) {
                return result;
            }
        }
    }
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn set_name_request(seq: u32, ifindex: u32, name: &str) -> io::Result<Vec<u8>> {
    let ifindex = i32::try_from(ifindex)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "ifindex out of range"))?;
    // NUL-terminated name in an rtattr.
    let rta_len = RTA_HDRLEN + name.len() + 1;
    let total = NLMSG_HDRLEN + IFINFOMSG_LEN + align4(rta_len);

    let mut msg = Vec::with_capacity(total);
    // struct nlmsghdr
    msg.extend_from_slice(&(total as u32).to_ne_bytes());
    msg.extend_from_slice(&RTM_NEWLINK.to_ne_bytes());
    msg.extend_from_slice(&(NLM_F_REQUEST | NLM_F_ACK).to_ne_bytes());
    msg.extend_from_slice(&seq.to_ne_bytes());
    msg.extend_from_slice(&0u32.to_ne_bytes()); // pid: let the kernel fill it in
    // struct ifinfomsg
    msg.push(0); // ifi_family = AF_UNSPEC
    msg.push(0); // padding
    msg.extend_from_slice(&0u16.to_ne_bytes()); // ifi_type
    msg.extend_from_slice(&ifindex.to_ne_bytes());
    msg.extend_from_slice(&0u32.to_ne_bytes()); // ifi_flags
    msg.extend_from_slice(&0u32.to_ne_bytes()); // ifi_change
    // struct rtattr IFLA_IFNAME
    msg.extend_from_slice(&(rta_len as u16).to_ne_bytes());
    msg.extend_from_slice(&IFLA_IFNAME.to_ne_bytes());
    msg.extend_from_slice(name.as_bytes());
    msg.resize(total, 0);
    Ok(msg)
}

/// Find the NLMSG_ERROR reply for `seq` in a datagram: `Some(Ok)` for an ack,
/// `Some(Err)` for a failure, `None` if it is not there.
fn parse_ack(mut buf: &[u8], seq: u32) -> Option<io::Result<()>> {
    let u32_at = |b: &[u8], i: usize| u32::from_ne_bytes(b[i..i + 4].try_into().unwrap());
    while buf.len() >= NLMSG_HDRLEN {
        let len = u32_at(buf, 0) as usize;
        if len < NLMSG_HDRLEN || len > buf.len() {
            break;
        }
        let kind = u16::from_ne_bytes([buf[4], buf[5]]);
        if kind == NLMSG_ERROR && u32_at(buf, 8) == seq && len >= NLMSG_HDRLEN + 4 {
            let errno = u32_at(buf, NLMSG_HDRLEN) as i32;
            return Some(if errno == 0 {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(-errno))
            });
        }
        buf = &buf[align4(len).min(buf.len())..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_layout() {
        let msg = set_name_request(7, 3, "lan0").unwrap();
        // 16 + 16 + align4(4 + 5) = 44
        assert_eq!(msg.len(), 44);
        assert_eq!(u32::from_ne_bytes(msg[0..4].try_into().unwrap()), 44);
        assert_eq!(u16::from_ne_bytes([msg[4], msg[5]]), RTM_NEWLINK);
        assert_eq!(u32::from_ne_bytes(msg[8..12].try_into().unwrap()), 7);
        assert_eq!(i32::from_ne_bytes(msg[20..24].try_into().unwrap()), 3);
        assert_eq!(u16::from_ne_bytes([msg[32], msg[33]]), 9);
        assert_eq!(u16::from_ne_bytes([msg[34], msg[35]]), IFLA_IFNAME);
        assert_eq!(&msg[36..41], b"lan0\0");
    }

    fn error_reply(seq: u32, errno: i32) -> Vec<u8> {
        let mut m = Vec::new();
        m.extend_from_slice(&36u32.to_ne_bytes());
        m.extend_from_slice(&NLMSG_ERROR.to_ne_bytes());
        m.extend_from_slice(&0u16.to_ne_bytes());
        m.extend_from_slice(&seq.to_ne_bytes());
        m.extend_from_slice(&0u32.to_ne_bytes());
        m.extend_from_slice(&errno.to_ne_bytes());
        m.resize(36, 0); // echoed request header
        m
    }

    #[test]
    fn parses_ack_and_error() {
        assert!(matches!(parse_ack(&error_reply(1, 0), 1), Some(Ok(()))));
        let err = parse_ack(&error_reply(2, -16), 2).unwrap().unwrap_err();
        assert_eq!(err.raw_os_error(), Some(16)); // EBUSY
        assert!(parse_ack(&error_reply(3, 0), 4).is_none());
    }

    /// Needs CAP_NET_ADMIN in a throwaway network namespace, with sysfs
    /// remounted so it reflects that namespace:
    /// `unshare -rnm sh -c 'mount -t sysfs sysfs /sys && cargo test -- --ignored'`
    #[test]
    #[ignore]
    fn rename_in_netns() {
        let lo = std::fs::read_to_string("/sys/class/net/lo/ifindex").unwrap();
        let lo: u32 = lo.trim().parse().unwrap();
        let mut socket = Socket::open().unwrap();
        socket.set_name(lo, "renamed0").unwrap();
        assert!(std::path::Path::new("/sys/class/net/renamed0").exists());
        socket.set_name(lo, "lo").unwrap();
        let err = socket.set_name(u32::MAX >> 1, "x").unwrap_err();
        assert_eq!(err.raw_os_error(), Some(19)); // ENODEV
    }
}
