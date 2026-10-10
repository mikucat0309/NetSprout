use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use ipnet::IpNet;
use saphyr::{LoadableYamlNode, Yaml};

use super::error::Error;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub ethernets: Vec<Ethernet>,
}

#[derive(Debug, Clone)]
pub struct Ethernet {
    /// The key under `ethernets`; used as the interface name when `match` is absent.
    pub name: String,
    pub matches: Option<Match>,
    /// Rename the interface selected by `match.macaddress` to this name.
    pub set_name: Option<String>,
    pub dhcp4: Option<bool>,
    pub dhcp6: Option<bool>,
    pub dhcp4_overrides: DhcpOverrides,
    pub dhcp6_overrides: DhcpOverrides,
    pub addresses: Vec<IpNet>,
    pub gateway4: Option<Ipv4Addr>,
    pub gateway6: Option<Ipv6Addr>,
    pub nameservers: Option<Nameservers>,
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub name: Option<String>,
    pub macaddress: Option<String>,
}

/// `dhcp4-overrides` / `dhcp6-overrides`; unset fields keep the renderer's defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DhcpOverrides {
    pub use_dns: Option<bool>,
    pub use_ntp: Option<bool>,
    pub send_hostname: Option<bool>,
    pub use_hostname: Option<bool>,
    pub use_mtu: Option<bool>,
    pub hostname: Option<String>,
    pub use_routes: Option<bool>,
    pub route_metric: Option<u32>,
    pub use_domains: Option<UseDomains>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseDomains {
    Yes,
    No,
    /// Use the domains for routing DNS queries only, not for searching.
    Route,
}

#[derive(Debug, Clone)]
pub struct Nameservers {
    pub search: Option<Vec<String>>,
    pub addresses: Vec<IpAddr>,
}

#[derive(Debug, Clone)]
pub struct Route {
    pub to: IpNet,
    pub via: Option<IpAddr>,
    pub metric: Option<u32>,
}

fn invalid(msg: impl Into<String>) -> Error {
    Error::Parse(msg.into())
}

fn get_str<'a>(node: &'a Yaml, key: &str) -> Result<Option<&'a str>, Error> {
    match node.as_mapping_get(key) {
        None => Ok(None),
        Some(v) => v
            .as_str()
            .map(Some)
            .ok_or_else(|| invalid(format!("`{key}` must be a string"))),
    }
}

fn get_bool(node: &Yaml, key: &str) -> Result<Option<bool>, Error> {
    match node.as_mapping_get(key) {
        None => Ok(None),
        Some(v) => v
            .as_bool()
            .map(Some)
            .ok_or_else(|| invalid(format!("`{key}` must be a boolean"))),
    }
}

fn get_u32(node: &Yaml, key: &str) -> Result<Option<u32>, Error> {
    match node.as_mapping_get(key) {
        None => Ok(None),
        Some(v) => v
            .as_integer()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| invalid(format!("`{key}` must be a non-negative integer"))),
    }
}

fn get_parsed<T: FromStr>(node: &Yaml, key: &str) -> Result<Option<T>, Error> {
    get_str(node, key)?
        .map(|s| {
            s.parse()
                .map_err(|_| invalid(format!("invalid `{key}`: {s}")))
        })
        .transpose()
}

fn get_seq<'a>(node: &'a Yaml, key: &str) -> Result<&'a [Yaml<'a>], Error> {
    match node.as_mapping_get(key) {
        None => Ok(&[]),
        Some(v) => v
            .as_sequence()
            .map(Vec::as_slice)
            .ok_or_else(|| invalid(format!("`{key}` must be a list"))),
    }
}

fn get_str_seq<'a>(node: &'a Yaml, key: &str) -> Result<Vec<&'a str>, Error> {
    get_seq(node, key)?
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or_else(|| invalid(format!("`{key}` must be a list of strings")))
        })
        .collect()
}

/// Accept both `a.b.c.d/len` and a bare address (treated as a host route).
fn parse_net(s: &str) -> Option<IpNet> {
    IpNet::from_str(s)
        .ok()
        .or_else(|| IpAddr::from_str(s).ok().map(IpNet::from))
}

/// Mirror the kernel's `dev_valid_name()`.
fn is_valid_ifname(name: &str) -> bool {
    // IFNAMSIZ is 16 including the trailing NUL.
    !name.is_empty()
        && name.len() < 16
        && name != "."
        && name != ".."
        && !name.contains(|c: char| c == '/' || c == ':' || c.is_whitespace())
}

impl NetworkConfig {
    /// Parse a netplan-style (version 2) network-config document.
    pub fn parse(input: &str) -> Result<Self, Error> {
        let docs = Yaml::load_from_str(input).map_err(|e| invalid(e.to_string()))?;
        let doc = docs.first().ok_or_else(|| invalid("empty document"))?;
        // Both `network: {version: 2, ...}` and a bare `{version: 2, ...}` are accepted.
        let root = doc.as_mapping_get("network").unwrap_or(doc);

        match root.as_mapping_get("version").and_then(Yaml::as_integer) {
            Some(2) => {}
            Some(v) => return Err(invalid(format!("unsupported version: {v}"))),
            None => return Err(invalid("missing `version`")),
        }

        let mut ethernets = Vec::new();
        if let Some(node) = root.as_mapping_get("ethernets") {
            let mapping = node
                .as_mapping()
                .ok_or_else(|| invalid("`ethernets` must be a mapping"))?;
            for (key, value) in mapping {
                let name = key
                    .as_str()
                    .ok_or_else(|| invalid("interface id must be a string"))?;
                ethernets.push(Ethernet::parse(name, value)?);
            }
        }
        Ok(NetworkConfig { ethernets })
    }
}

impl Ethernet {
    fn parse(name: &str, node: &Yaml) -> Result<Self, Error> {
        let ctx = |e: Error| match e {
            Error::Parse(msg) => invalid(format!("ethernets.{name}: {msg}")),
            e => e,
        };
        Self::parse_inner(name, node).map_err(ctx)
    }

    fn parse_inner(name: &str, node: &Yaml) -> Result<Self, Error> {
        if !node.is_mapping() {
            return Err(invalid("must be a mapping"));
        }

        let matches = match node.as_mapping_get("match") {
            None => None,
            Some(m) => Some(Match {
                name: get_str(m, "name")?.map(str::to_string),
                macaddress: get_str(m, "macaddress")?.map(str::to_lowercase),
            }),
        };
        if let Some(m) = &matches
            && m.name.is_none()
            && m.macaddress.is_none()
        {
            return Err(invalid("`match` needs `name` or `macaddress`"));
        }
        let set_name = get_str(node, "set-name")?.map(str::to_string);
        if let Some(set_name) = &set_name {
            // Renaming identifies the interface by MAC only.
            if matches
                .as_ref()
                .and_then(|m| m.macaddress.as_ref())
                .is_none()
            {
                return Err(invalid("`set-name` requires `match.macaddress`"));
            }
            if !is_valid_ifname(set_name) {
                return Err(invalid(format!("invalid `set-name`: {set_name}")));
            }
        }

        let addresses = get_str_seq(node, "addresses")?
            .into_iter()
            .map(|s| IpNet::from_str(s).map_err(|_| invalid(format!("invalid address: {s}"))))
            .collect::<Result<_, _>>()?;

        let nameservers = match node.as_mapping_get("nameservers") {
            None => None,
            Some(ns) => {
                let search = match ns.as_mapping_get("search") {
                    None => None,
                    Some(_) => Some(
                        get_str_seq(ns, "search")?
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    ),
                };
                let addresses = get_str_seq(ns, "addresses")?
                    .into_iter()
                    .map(|s| {
                        IpAddr::from_str(s).map_err(|_| invalid(format!("invalid nameserver: {s}")))
                    })
                    .collect::<Result<_, _>>()?;
                Some(Nameservers { search, addresses })
            }
        };

        let routes = get_seq(node, "routes")?
            .iter()
            .map(Route::parse)
            .collect::<Result<_, _>>()?;

        Ok(Ethernet {
            name: name.to_string(),
            matches,
            set_name,
            dhcp4: get_bool(node, "dhcp4")?,
            dhcp6: get_bool(node, "dhcp6")?,
            dhcp4_overrides: DhcpOverrides::parse(node, "dhcp4-overrides")?,
            dhcp6_overrides: DhcpOverrides::parse(node, "dhcp6-overrides")?,
            addresses,
            gateway4: get_parsed(node, "gateway4")?,
            gateway6: get_parsed(node, "gateway6")?,
            nameservers,
            routes,
        })
    }
}

impl DhcpOverrides {
    fn parse(parent: &Yaml, key: &str) -> Result<Self, Error> {
        let Some(node) = parent.as_mapping_get(key) else {
            return Ok(Self::default());
        };
        if !node.is_mapping() {
            return Err(invalid(format!("`{key}` must be a mapping")));
        }
        let ctx = |e: Error| match e {
            Error::Parse(msg) => invalid(format!("{key}: {msg}")),
            e => e,
        };
        Self::parse_inner(node).map_err(ctx)
    }

    fn parse_inner(node: &Yaml) -> Result<Self, Error> {
        let use_domains = match node.as_mapping_get("use-domains") {
            None => None,
            Some(v) => Some(match (v.as_bool(), v.as_str()) {
                (Some(true), _) => UseDomains::Yes,
                (Some(false), _) => UseDomains::No,
                (_, Some("route")) => UseDomains::Route,
                _ => return Err(invalid("`use-domains` must be a boolean or `route`")),
            }),
        };
        Ok(DhcpOverrides {
            use_dns: get_bool(node, "use-dns")?,
            use_ntp: get_bool(node, "use-ntp")?,
            send_hostname: get_bool(node, "send-hostname")?,
            use_hostname: get_bool(node, "use-hostname")?,
            use_mtu: get_bool(node, "use-mtu")?,
            hostname: get_str(node, "hostname")?.map(str::to_string),
            use_routes: get_bool(node, "use-routes")?,
            route_metric: get_u32(node, "route-metric")?,
            use_domains,
        })
    }
}

impl Route {
    fn parse(node: &Yaml) -> Result<Self, Error> {
        let via: Option<IpAddr> = get_parsed(node, "via")?;
        let to = match get_str(node, "to")?.ok_or_else(|| invalid("route missing `to`"))? {
            // `default` takes its address family from `via`.
            "default" => match via {
                Some(IpAddr::V6(_)) => IpNet::from_str("::/0").unwrap(),
                _ => IpNet::from_str("0.0.0.0/0").unwrap(),
            },
            s => parse_net(s).ok_or_else(|| invalid(format!("invalid route `to`: {s}")))?,
        };
        let metric = get_u32(node, "metric")?;
        Ok(Route { to, via, metric })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full() {
        let cfg = NetworkConfig::parse(
            r#"
network:
  version: 2
  ethernets:
    id0:
      match:
        macaddress: "52:54:00:AB:CD:EF"
      set-name: lan0
      dhcp4: false
      addresses: [192.168.1.10/24, "2001:db8::10/64"]
      gateway4: 192.168.1.1
      nameservers:
        search: [example.com]
        addresses: [1.1.1.1]
      routes:
        - to: default
          via: "2001:db8::1"
        - to: 10.0.0.0/8
          via: 192.168.1.254
          metric: 100
    eth1:
      dhcp4: true
"#,
        )
        .unwrap();
        assert_eq!(cfg.ethernets.len(), 2);
        let e = &cfg.ethernets[0];
        assert_eq!(e.name, "id0");
        assert_eq!(
            e.matches.as_ref().unwrap().macaddress.as_deref(),
            Some("52:54:00:ab:cd:ef")
        );
        assert_eq!(e.set_name.as_deref(), Some("lan0"));
        assert_eq!(e.addresses.len(), 2);
        assert_eq!(e.routes[0].to.to_string(), "::/0");
        assert_eq!(e.routes[1].metric, Some(100));
        assert_eq!(cfg.ethernets[1].dhcp4, Some(true));
    }

    #[test]
    fn parse_dhcp_overrides() {
        let cfg = NetworkConfig::parse(
            r#"
version: 2
ethernets:
  eth0:
    dhcp4: true
    dhcp4-overrides:
      use-dns: false
      use-ntp: true
      send-hostname: true
      use-hostname: false
      use-mtu: true
      hostname: web1
      use-routes: false
      route-metric: 200
      use-domains: route
    dhcp6-overrides:
      use-domains: true
"#,
        )
        .unwrap();
        let e = &cfg.ethernets[0];
        assert_eq!(
            e.dhcp4_overrides,
            DhcpOverrides {
                use_dns: Some(false),
                use_ntp: Some(true),
                send_hostname: Some(true),
                use_hostname: Some(false),
                use_mtu: Some(true),
                hostname: Some("web1".into()),
                use_routes: Some(false),
                route_metric: Some(200),
                use_domains: Some(UseDomains::Route),
            }
        );
        assert_eq!(e.dhcp6_overrides.use_domains, Some(UseDomains::Yes));
    }

    #[test]
    fn rejects_bad_dhcp_overrides() {
        for bad in [
            "dhcp4-overrides: true",
            "dhcp4-overrides:\n      use-dns: maybe",
            "dhcp6-overrides:\n      route-metric: -1",
            "dhcp4-overrides:\n      use-domains: search",
        ] {
            let yaml = format!("version: 2\nethernets:\n  eth0:\n    {bad}\n");
            assert!(NetworkConfig::parse(&yaml).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_v1() {
        assert!(NetworkConfig::parse("version: 1\nconfig: []\n").is_err());
    }

    #[test]
    fn set_name_requires_match_macaddress() {
        let without_match = "version: 2\nethernets:\n  eth0:\n    set-name: foo\n";
        assert!(NetworkConfig::parse(without_match).is_err());

        let match_name =
            "version: 2\nethernets:\n  eth0:\n    match:\n      name: eth0\n    set-name: foo\n";
        assert!(NetworkConfig::parse(match_name).is_err());
    }

    #[test]
    fn rejects_invalid_set_name() {
        for bad in ["\"\"", "a/b", "a:b", "\"a b\"", "..", "sixteen-chars-xx"] {
            let yaml = format!(
                "version: 2\nethernets:\n  e:\n    match:\n      macaddress: \"52:54:00:00:00:01\"\n    set-name: {bad}\n"
            );
            assert!(NetworkConfig::parse(&yaml).is_err(), "{bad}");
        }
    }
}
