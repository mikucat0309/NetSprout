mod net;
mod render;
mod source;

use std::path::Path;
use std::process::ExitCode;

use render::networkd;

fn main() -> ExitCode {
    let (files, names_ok) = match source::fetch() {
        Ok(config) => {
            let renamed = net::apply_names(&config);
            if let Err(e) = &renamed {
                eprintln!("set-name: {e}");
            }
            (networkd::render(&config), renamed.is_ok())
        }
        // Without a seed, still bring the network up so the machine stays reachable.
        Err(e @ source::Error::NotFound) => {
            eprintln!("{e}, falling back to DHCP");
            (networkd::fallback(), true)
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

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
    if names_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
