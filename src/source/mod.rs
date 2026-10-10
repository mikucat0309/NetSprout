mod config;
mod error;
mod nocloud;

pub use config::{DhcpOverrides, Ethernet, NetworkConfig, Route, UseDomains};
pub use error::Error;

/// Locate the NoCloud seed and parse its network-config.
pub fn fetch() -> Result<NetworkConfig, Error> {
    NetworkConfig::parse(&nocloud::fetch()?)
}
