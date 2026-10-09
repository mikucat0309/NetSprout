mod config;
mod error;
mod nocloud;

pub use config::{Ethernet, NetworkConfig, Route};
pub use error::Error;

/// Locate the NoCloud seed and parse its network-config.
pub fn fetch() -> Result<NetworkConfig, Error> {
    NetworkConfig::parse(&nocloud::fetch()?)
}
