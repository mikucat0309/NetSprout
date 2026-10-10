use std::fmt::Write;

use crate::source::{DhcpOverrides, Ethernet, NetworkConfig, Route, UseDomains};

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

/// Config used when there is no seed: DHCP on every physical Ethernet link.
///
/// `Kind=!*` leaves out virtual links (bridges, veth, tunnels) so that only
/// real NICs are configured.
pub fn fallback() -> Vec<RenderedFile> {
    vec![RenderedFile {
        name: format!("{FILE_PREFIX}fallback.network"),
        content: "\
[Match]
Type=ether
Kind=!*

[Network]
DHCP=yes
"
        .to_string(),
    }]
}

fn render_network(eth: &Ethernet) -> String {
    let mut out = String::from("[Match]\n");
    match &eth.matches {
        Some(m) => {
            // After `set-name` the original name no longer applies; the MAC
            // (required with `set-name`) still matches whether or not the
            // rename succeeded.
            if let Some(name) = m.name.as_ref().filter(|_| eth.set_name.is_none()) {
                writeln!(out, "Name={name}").unwrap();
            }
            if let Some(mac) = &m.macaddress {
                writeln!(out, "MACAddress={mac}").unwrap();
            }
        }
        None => writeln!(out, "Name={}", eth.name).unwrap(),
    }

    if let Some(mtu) = eth.mtu {
        write!(out, "\n[Link]\nMTUBytes={mtu}\n").unwrap();
    }

    out.push_str("\n[Network]\n");
    let dhcp4 = eth.dhcp4.unwrap_or(false);
    let dhcp6 = eth.dhcp6.unwrap_or(false);
    let dhcp = match (dhcp4, dhcp6) {
        (true, true) => Some("yes"),
        (true, false) => Some("ipv4"),
        (false, true) => Some("ipv6"),
        (false, false) => None,
    };
    if let Some(dhcp) = dhcp {
        writeln!(out, "DHCP={dhcp}").unwrap();
    }
    if let Some(accept_ra) = eth.accept_ra {
        writeln!(out, "IPv6AcceptRA={}", yes_no(accept_ra)).unwrap();
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

    if dhcp4 {
        render_dhcp4(&mut out, &eth.dhcp4_overrides);
    }
    if dhcp6 {
        render_dhcp6(&mut out, &eth.dhcp6_overrides, eth.accept_ra);
    }

    for route in &eth.routes {
        render_route(&mut out, route);
    }
    out
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// Keys shared by `[DHCPv4]` and `[DHCPv6]`.
fn common_dhcp_keys(o: &DhcpOverrides) -> Vec<String> {
    let mut keys = Vec::new();
    let mut flag = |key: &str, v: Option<bool>| {
        if let Some(v) = v {
            keys.push(format!("{key}={}", yes_no(v)));
        }
    };
    flag("UseDNS", o.use_dns);
    flag("UseNTP", o.use_ntp);
    flag("SendHostname", o.send_hostname);
    flag("UseHostname", o.use_hostname);
    if let Some(hostname) = &o.hostname {
        keys.push(format!("Hostname={hostname}"));
    }
    if let Some(d) = o.use_domains {
        let v = match d {
            UseDomains::Yes => "yes",
            UseDomains::No => "no",
            UseDomains::Route => "route",
        };
        keys.push(format!("UseDomains={v}"));
    }
    keys
}

fn render_section(out: &mut String, section: &str, keys: &[String]) {
    if keys.is_empty() {
        return;
    }
    write!(out, "\n[{section}]\n").unwrap();
    for key in keys {
        writeln!(out, "{key}").unwrap();
    }
}

fn render_dhcp4(out: &mut String, o: &DhcpOverrides) {
    let mut keys = common_dhcp_keys(o);
    if let Some(v) = o.use_mtu {
        keys.push(format!("UseMTU={}", yes_no(v)));
    }
    if let Some(v) = o.use_routes {
        keys.push(format!("UseRoutes={}", yes_no(v)));
    }
    if let Some(metric) = o.route_metric {
        keys.push(format!("RouteMetric={metric}"));
    }
    render_section(out, "DHCPv4", &keys);
}

fn render_dhcp6(out: &mut String, o: &DhcpOverrides, accept_ra: Option<bool>) {
    let mut keys = common_dhcp_keys(o);
    // networkd starts the DHCPv6 client on receiving an RA by default; with
    // RAs ignored it would never start.
    if accept_ra == Some(false) {
        keys.push("WithoutRA=solicit".to_string());
    }
    render_section(out, "DHCPv6", &keys);
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
    fn set_name_matches_by_mac() {
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

    #[test]
    fn set_name_drops_match_name() {
        let files = render_str(
            r#"
version: 2
ethernets:
  nic0:
    match:
      name: "ens*"
      macaddress: "52:54:00:12:34:56"
    set-name: lan0
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
    fn dhcp_overrides() {
        let files = render_str(
            r#"
version: 2
ethernets:
  eth0:
    dhcp4: true
    dhcp6: true
    dhcp4-overrides:
      use-dns: false
      use-ntp: true
      send-hostname: true
      use-hostname: false
      hostname: web1
      use-domains: route
      use-mtu: true
      use-routes: false
      route-metric: 200
    dhcp6-overrides:
      use-dns: false
      use-domains: true
      route-metric: 300
    routes:
      - to: 10.0.0.0/8
        via: 192.168.1.254
"#,
        );
        assert_content(
            &files[0],
            r#"
[Match]
Name=eth0

[Network]
DHCP=yes

[DHCPv4]
UseDNS=no
UseNTP=yes
SendHostname=yes
UseHostname=no
Hostname=web1
UseDomains=route
UseMTU=yes
UseRoutes=no
RouteMetric=200

[DHCPv6]
UseDNS=no
UseDomains=yes

[Route]
Destination=10.0.0.0/8
Gateway=192.168.1.254
"#,
        );
    }

    #[test]
    fn dhcp_overrides_need_dhcp_enabled() {
        let files = render_str(
            r#"
version: 2
ethernets:
  eth0:
    dhcp4: true
    dhcp4-overrides:
      use-dns: false
    dhcp6-overrides:
      use-dns: false
"#,
        );
        assert_content(
            &files[0],
            r#"
[Match]
Name=eth0

[Network]
DHCP=ipv4

[DHCPv4]
UseDNS=no
"#,
        );
    }

    #[test]
    fn mtu() {
        let files = render_str(
            r#"
version: 2
ethernets:
  eth0:
    mtu: 9000
    dhcp4: true
"#,
        );
        assert_content(
            &files[0],
            r#"
[Match]
Name=eth0

[Link]
MTUBytes=9000

[Network]
DHCP=ipv4
"#,
        );
    }

    #[test]
    fn accept_ra() {
        let files = render_str(
            r#"
version: 2
ethernets:
  eth0:
    accept-ra: true
  eth1:
    dhcp4: true
    accept-ra: false
"#,
        );
        assert_content(
            &files[0],
            r#"
[Match]
Name=eth0

[Network]
IPv6AcceptRA=yes
"#,
        );
        assert_content(
            &files[1],
            r#"
[Match]
Name=eth1

[Network]
DHCP=ipv4
IPv6AcceptRA=no
"#,
        );
    }

    #[test]
    fn dhcp6_without_ra() {
        let files = render_str(
            r#"
version: 2
ethernets:
  eth0:
    dhcp6: true
    accept-ra: false
    dhcp6-overrides:
      use-dns: false
"#,
        );
        assert_content(
            &files[0],
            r#"
[Match]
Name=eth0

[Network]
DHCP=ipv6
IPv6AcceptRA=no

[DHCPv6]
UseDNS=no
WithoutRA=solicit
"#,
        );
    }

    #[test]
    fn fallback_is_dhcp_on_physical_links() {
        let files = fallback();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "10-cloud-network-config-fallback.network");
        assert_content(
            &files[0],
            r#"
[Match]
Type=ether
Kind=!*

[Network]
DHCP=yes
"#,
        );
    }
}
