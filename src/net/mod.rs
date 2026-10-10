//! Renderer-independent interface renaming (`set-name`).
//!
//! Interfaces are renamed directly through rtnetlink, by ifindex, before any
//! renderer output is written. Renderers only have to match the final names
//! (or MACs); nothing here depends on networkd or on udev.

mod plan;
mod rtnetlink;
mod sysfs;

use std::io;

use crate::source::NetworkConfig;
use plan::Want;

/// Rename interfaces as requested by `set-name`.
///
/// Requests that cannot be applied are reported and skipped; an `Err` means
/// renaming could not run at all or stopped part-way.
pub fn apply_names(config: &NetworkConfig) -> io::Result<()> {
    let wants: Vec<Want> = config
        .ethernets
        .iter()
        .filter_map(|eth| {
            let name = eth.set_name.clone()?;
            // Config parsing guarantees a MAC whenever `set-name` is present.
            let mac = eth.matches.as_ref()?.macaddress.clone()?;
            Some(Want { mac, name })
        })
        .collect();
    if wants.is_empty() {
        return Ok(());
    }

    let plan = plan::plan(&sysfs::links()?, &wants);
    for warning in &plan.warnings {
        eprintln!("set-name: {warning}");
    }
    if plan.renames.is_empty() {
        return Ok(());
    }

    let mut socket = rtnetlink::Socket::open()?;
    for rename in &plan.renames {
        socket.set_name(rename.ifindex, &rename.to).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("renaming {} to {}: {e}", rename.from, rename.to),
            )
        })?;
        eprintln!("renamed {} to {}", rename.from, rename.to);
    }
    Ok(())
}
