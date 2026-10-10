use std::fs;
use std::io;
use std::path::Path;

use super::plan::Link;

pub const SYS_NET: &str = "/sys/class/net";

const IFF_UP: u32 = 0x1;

/// List every interface currently known to the kernel.
pub fn links() -> io::Result<Vec<Link>> {
    let mut links = Vec::new();
    for entry in fs::read_dir(SYS_NET)? {
        let entry = entry?;
        let path = entry.path();
        // The interface may disappear while we scan; skip it rather than fail.
        let Some(ifindex) = ifindex(&path) else {
            continue;
        };
        let flags = read_trimmed(&path.join("flags"))
            .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        links.push(Link {
            ifindex,
            name: entry.file_name().to_string_lossy().into_owned(),
            mac: read_trimmed(&path.join("address")).map(|s| s.to_lowercase()),
            physical: path.join("device").exists(),
            up: flags & IFF_UP != 0,
        });
    }
    links.sort_by_key(|l| l.ifindex);
    Ok(links)
}

fn ifindex(path: &Path) -> Option<u32> {
    read_trimmed(&path.join("ifindex"))?.parse().ok()
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}
