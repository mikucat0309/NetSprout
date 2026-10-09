pub mod networkd;

use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub name: String,
    pub content: String,
}

/// Make `files` the only files in `dir` whose names start with `prefix`.
pub fn replace(dir: &Path, prefix: &str, files: &[RenderedFile]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    remove_prefixed(dir, prefix)?;
    for file in files {
        fs::write(dir.join(&file.name), &file.content)?;
    }
    Ok(())
}

fn remove_prefixed(dir: &Path, prefix: &str) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
