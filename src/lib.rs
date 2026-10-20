//! mierucover: Explorer thumbnails for comic and book archives.
//!
//! The DLL is a thumbnail provider. Explorer hands it a stream of the file; it finds the
//! cover inside (reading as little of the file as it can), decodes it with WIC at the size
//! Explorer asked for, and returns a bitmap.

pub mod archive;
pub mod com;
pub mod cover;
pub mod epub;
pub mod error;
pub mod image;
pub mod imghdr;
pub mod natsort;
mod text;
mod xml;

pub use error::{Error, Result};
