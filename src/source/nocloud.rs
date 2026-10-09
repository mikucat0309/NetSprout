use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use rustix::mount::{MountFlags, UnmountFlags, mount, unmount};

use super::error::Error;

const SEED_LABEL: &str = "cidata";
const NETWORK_CONFIG: &str = "network-config";

const SYS_BLOCK: &str = "/sys/class/block";
const MOUNTINFO: &str = "/proc/self/mountinfo";
/// /run is always a writable tmpfs this early in boot, unlike /tmp.
const MOUNT_BASE: &str = "/run/cloud-network-config";

/// ISO9660 Primary Volume Descriptor lives at sector 16 (2048-byte sectors).
const ISO9660_PVD_OFFSET: u64 = 16 * 2048;

#[derive(Debug, Clone, Copy)]
enum FsType {
    Iso9660,
    Vfat,
}

impl FsType {
    fn as_str(self) -> &'static str {
        match self {
            FsType::Iso9660 => "iso9660",
            FsType::Vfat => "vfat",
        }
    }
}

pub fn fetch() -> Result<String, Error> {
    for entry in fs::read_dir(SYS_BLOCK)? {
        let entry = entry?;
        let name = entry.file_name();
        let dev_path = Path::new("/dev").join(&name);

        // Devices without media (e.g. empty CD-ROM drives) or without permission
        // are not candidates; skip them instead of aborting the whole scan.
        let Ok(mut dev_file) = File::open(&dev_path) else {
            continue;
        };

        let detected = match try_parse_iso9660(&mut dev_file) {
            Some(label) => Some((FsType::Iso9660, label)),
            None => try_parse_vfat(&mut dev_file).map(|label| (FsType::Vfat, label)),
        };
        let Some((fs_type, label)) = detected else {
            continue;
        };
        if !label.eq_ignore_ascii_case(SEED_LABEL) {
            continue;
        }

        let dev_id = fs::read_to_string(entry.path().join("dev"))?;
        let content = match find_mount_path(dev_id.trim())? {
            Some(mount_path) => read_network_config(&mount_path)?,
            None => {
                let temp = TempMount::new(&dev_path, fs_type)?;
                read_network_config(&temp.path)?
            }
        };
        if let Some(content) = content {
            return Ok(content);
        }
    }
    Err(Error::NotFound)
}

fn read_network_config(mount_path: &Path) -> io::Result<Option<String>> {
    let file = mount_path.join(NETWORK_CONFIG);
    if !file.exists() {
        return Ok(None);
    }
    fs::read_to_string(file).map(Some)
}

fn read_at<R: Read + Seek>(file: &mut R, offset: u64, buf: &mut [u8]) -> Option<()> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    file.read_exact(buf).ok()
}

fn trim_label(raw: &[u8]) -> Option<String> {
    let label = std::str::from_utf8(raw).ok()?.trim_end_matches([' ', '\0']);
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

/// Read the volume identifier from the ISO9660 Primary Volume Descriptor.
fn try_parse_iso9660<R: Read + Seek>(file: &mut R) -> Option<String> {
    let mut pvd = [0u8; 72];
    read_at(file, ISO9660_PVD_OFFSET, &mut pvd)?;
    // type code 1 = primary volume descriptor, followed by "CD001"
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return None;
    }
    trim_label(&pvd[40..72])
}

/// Read the volume label from a FAT12/16/32 boot sector.
fn try_parse_vfat<R: Read + Seek>(file: &mut R) -> Option<String> {
    let mut bs = [0u8; 512];
    read_at(file, 0, &mut bs)?;
    if bs[510..512] != [0x55, 0xAA] {
        return None;
    }
    // FAT32 extended BPB: signature at 0x42, label at 0x47, fs type at 0x52.
    // FAT12/16 extended BPB: signature at 0x26, label at 0x2B, fs type at 0x36.
    let label = if bs[0x42] == 0x29 && &bs[0x52..0x57] == b"FAT32" {
        &bs[0x47..0x52]
    } else if bs[0x26] == 0x29 && &bs[0x36..0x39] == b"FAT" {
        &bs[0x2B..0x36]
    } else {
        return None;
    };
    let label = trim_label(label)?;
    if label == "NO NAME" {
        None
    } else {
        Some(label)
    }
}

/// Look up the mount point of a block device (by "major:minor") in mountinfo.
fn find_mount_path(dev_id: &str) -> io::Result<Option<PathBuf>> {
    let mountinfo = fs::read_to_string(MOUNTINFO)?;
    for line in mountinfo.lines() {
        // mount_id parent_id major:minor root mount_point ...
        let mut fields = line.split_whitespace();
        let (Some(_), Some(_), Some(id), Some(_), Some(mount_point)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        if id == dev_id {
            return Ok(Some(PathBuf::from(unescape_octal(mount_point))));
        }
    }
    Ok(None)
}

/// mountinfo escapes space, tab, newline and backslash as `\ooo`.
fn unescape_octal(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let is_escape = bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b));
        if is_escape {
            let v = (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0');
            out.push(v);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A read-only mount on a temporary directory; unmounted and removed on drop.
struct TempMount {
    path: PathBuf,
}

impl TempMount {
    fn new(device: &Path, fs_type: FsType) -> Result<Self, Error> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        fs::create_dir_all(MOUNT_BASE)?;
        let path = Path::new(MOUNT_BASE).join(format!(
            "{}-{}-{}",
            SEED_LABEL,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        if let Err(e) = mount(device, &path, fs_type.as_str(), MountFlags::RDONLY, None) {
            let _ = fs::remove_dir(&path);
            return Err(e.into());
        }
        Ok(TempMount { path })
    }
}

impl Drop for TempMount {
    fn drop(&mut self) {
        let _ = unmount(&self.path, UnmountFlags::empty());
        let _ = fs::remove_dir(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_mount_point() {
        assert_eq!(unescape_octal(r"/mnt/a\040b"), "/mnt/a b");
        assert_eq!(unescape_octal(r"/mnt/plain"), "/mnt/plain");
        assert_eq!(unescape_octal(r"/mnt/x\134"), r"/mnt/x\");
    }
}
