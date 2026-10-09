use std::fmt::Write;

use crate::source::{Ethernet, NetworkConfig, Route};

use super::RenderedFile;

pub const OUTPUT_DIR: &str = "/run/systemd/network";
pub const FILE_PREFIX: &str = "10-cloud-network-config-";

pub fn render(config: &NetworkConfig) -> Vec<RenderedFile> {
    config
        .ethernets
        .iter()
        .map(|eth| RenderedFile {
            name: format!("{FILE_PREFIX}{}.network", eth.name.replace('/', "_")),
            content: render_network(eth),
        })
        .collect()
}

fn render_network(eth: &Ethernet) -> String {
    let mut out = String::from("[Match]\n");
    match &eth.matches {
        Some(m) => {
            if let Some(name) = &m.name {
                writeln!(out, "Name={name}").unwrap();
            }
            if let Some(mac) = &m.macaddress {
                writeln!(out, "MACAddress={mac}").unwrap();
            }
        }
        None => writeln!(out, "Name={}", eth.name).unwrap(),
    }

    out.push_str("\n[Network]\n");
    let dhcp = match (eth.dhcp4.unwrap_or(false), eth.dhcp6.unwrap_or(false)) {
        (true, true) => Some("yes"),
        (true, false) => Some("ipv4"),
        (false, true) => Some("ipv6"),
        (false, false) => None,
    };
    if let Some(dhcp) = dhcp {
        writeln!(out, "DHCP={dhcp}").unwrap();
    }
    for addr in &eth.addresses {
        writeln!(out, "Address={addr}").unwrap();
    }
    if let Some(gw) = eth.gateway4 {
        writeln!(out, "Gateway={gw}").unwrap();
    }
    if let Some(gw) = eth.gateway6 {
        writeln!(out, "Gateway={gw}").unwrap();
    }
    if let Some(ns) = &eth.nameservers {
        for dns in &ns.addresses {
            writeln!(out, "DNS={dns}").unwrap();
        }
        if let Some(search) = ns.search.as_ref().filter(|s| !s.is_empty()) {
            writeln!(out, "Domains={}", search.join(" ")).unwrap();
        }
    }

    for route in &eth.routes {
        render_route(&mut out, route);
    }
    out
}

fn render_route(out: &mut String, route: &Route) {
    write!(out, "\n[Route]\nDestination={}\n", route.to).unwrap();
    if let Some(via) = route.via {
        writeln!(out, "Gateway={via}").unwrap();
    }
    if let Some(metric) = route.metric {
        writeln!(out, "Metric={metric}").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_str(yaml: &str) -> Vec<RenderedFile> {
        render(&NetworkConfig::parse(yaml).unwrap())
    }

    /// Compare against a raw string literal that starts with a newline.
    fn assert_content(file: &RenderedFile, expected: &str) {
        assert_eq!(file.content, expected.strip_prefix('\n').unwrap());
    }

    #[test]
    fn static_with_routes() {
        let files = render_str(
            r#"
version: 2
ethernets:
  eth0:
    addresses: [192.168.1.10/24]
    gateway4: 192.168.1.1
    nameservers:
      search: [a.example, b.example]
      addresses: [1.1.1.1, "2606:4700::1111"]
    routes:
      - to: 10.0.0.0/8
        via: 192.168.1.254
        metric: 100
"#,
        );
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "10-cloud-network-config-eth0.network");
        assert_content(
            &files[0],
            r#"
[Match]
Name=eth0

[Network]
Address=192.168.1.10/24
Gateway=192.168.1.1
DNS=1.1.1.1
DNS=2606:4700::1111
Domains=a.example b.example

[Route]
Destination=10.0.0.0/8
Gateway=192.168.1.254
Metric=100
"#,
        );
    }

    #[test]
    fn set_name_is_ignored() {
        let files = render_str(
            r#"
version: 2
ethernets:
  nic0:
    match:
      macaddress: "52:54:00:12:34:56"
    set-name: lan0
    dhcp4: true
    dhcp6: true
"#,
        );
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "10-cloud-network-config-nic0.network");
        assert_content(
            &files[0],
            r#"
[Match]
MACAddress=52:54:00:12:34:56

[Network]
DHCP=yes
"#,
        );
    }

    #[test]
    fn match_mac() {
        let files = render_str(
            r#"
version: 2
ethernets:
  any:
    match:
      macaddress: "52:54:00:12:34:56"
"#,
        );
        assert_content(
            &files[0],
            r#"
[Match]
MACAddress=52:54:00:12:34:56

[Network]
"#,
        );
    }

    #[test]
    fn match_name() {
        let files = render_str(
            r#"
version: 2
ethernets:
  any:
    match:
      name: "en*"
    dhcp4: true
"#,
        );
        assert_eq!(files.len(), 1);
        assert_content(
            &files[0],
            r#"
[Match]
Name=en*

[Network]
DHCP=ipv4
"#,
        );
    }
}
