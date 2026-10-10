mod net;
mod render;
mod source;

use std::path::Path;
use std::process::ExitCode;

use render::networkd;

fn main() -> ExitCode {
    let config = match source::fetch() {
        Ok(config) => config,
        Err(source::Error::NotFound) => {
            eprintln!("{}", source::Error::NotFound);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let renamed = net::apply_names(&config);
    if let Err(e) = &renamed {
        eprintln!("set-name: {e}");
    }

    let files = networkd::render(&config);
    if let Err(e) = render::replace(
        Path::new(networkd::OUTPUT_DIR),
        networkd::FILE_PREFIX,
        &files,
    ) {
        eprintln!("failed to write {}: {e}", networkd::OUTPUT_DIR);
        return ExitCode::FAILURE;
    }
    for file in &files {
        eprintln!("wrote {}/{}", networkd::OUTPUT_DIR, file.name);
    }
    if renamed.is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
