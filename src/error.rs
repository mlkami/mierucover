use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Win(windows_core::Error),
    /// Not a format we read, or damaged.
    Format(&'static str),
    /// Encrypted entries, unknown compression methods and the like.
    Unsupported(&'static str),
    /// An entry or header is larger than we are willing to load.
    TooLarge,
    /// Nothing in the file can serve as a cover.
    NoCover,
    /// An error from one of the archive libraries.
    Lib(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn lib(e: impl fmt::Display) -> Self {
        Error::Lib(e.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Win(e) => write!(f, "windows: {e}"),
            Error::Format(s) => write!(f, "format: {s}"),
            Error::Unsupported(s) => write!(f, "unsupported: {s}"),
            Error::TooLarge => f.write_str("too large"),
            Error::NoCover => f.write_str("no cover"),
            Error::Lib(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<windows_core::Error> for Error {
    fn from(e: windows_core::Error) -> Self {
        Error::Win(e)
    }
}
