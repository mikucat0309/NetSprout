use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    NotFound,
    Parse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::NotFound => write!(f, "no NoCloud seed with network-config found"),
            Error::Parse(msg) => write!(f, "invalid network-config: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<rustix::io::Errno> for Error {
    fn from(e: rustix::io::Errno) -> Self {
        Error::Io(e.into())
    }
}
