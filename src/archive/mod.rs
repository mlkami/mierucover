//! Readers for the containers a cover can live in.

pub mod mobi;
pub mod rar;
pub mod sevenz;
pub mod tar;
pub mod zip;

use std::io::{self, Read, Seek, SeekFrom};

use crate::Result;
use crate::cover::{self, Cover};

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek + ?Sized> ReadSeek for T {}

pub struct Entry {
    /// Path inside the archive, with `/` separators.
    pub name: String,
    /// Uncompressed size.
    pub size: u64,
}

pub trait Archive {
    fn entries(&self) -> &[Entry];

    /// Reads a whole entry, failing with `TooLarge` if it is bigger than `limit`.
    fn read(&mut self, index: usize, limit: u64) -> Result<Vec<u8>>;

    /// Reads the start of an entry (at most `n` bytes); formats that can stop early do.
    fn read_prefix(&mut self, index: usize, n: usize) -> Result<Vec<u8>> {
        let mut v = self.read(index, cover::MAX_IMAGE)?;
        v.truncate(n);
        Ok(v)
    }

    /// Bytes that must be decoded before this entry can be read: non-zero only in
    /// solid archives, where entries share one compressed stream.
    fn cost(&self, _index: usize) -> u64 {
        0
    }

    /// Finds the cover of an archive or book stored inside this one.
    fn nested_cover(&mut self, index: usize, depth: u32) -> Result<Cover> {
        let bytes = self.read(index, cover::MAX_NESTED)?;
        cover::find_in_bytes(bytes, depth)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Zip,
    Rar,
    SevenZ,
    Tar,
    Mobi,
}

/// Tells the format from the first bytes. Zip is also tried when nothing matches,
/// since a zip can have other data in front of it.
pub fn sniff(head: &[u8]) -> Option<Kind> {
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") || head.starts_with(b"PK\x07\x08") {
        Some(Kind::Zip)
    } else if head.starts_with(b"Rar!\x1a\x07") {
        Some(Kind::Rar)
    } else if head.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Some(Kind::SevenZ)
    } else if head.len() >= 68 && &head[60..68] == b"BOOKMOBI" {
        Some(Kind::Mobi)
    } else if head.len() >= 262 && &head[257..262] == b"ustar" {
        Some(Kind::Tar)
    } else {
        None
    }
}

/// A byte range of another reader, read in place.
pub struct Sub<'a> {
    inner: &'a mut dyn ReadSeek,
    start: u64,
    len: u64,
    pos: u64,
}

impl<'a> Sub<'a> {
    pub fn new(inner: &'a mut dyn ReadSeek, start: u64, len: u64) -> Self {
        Sub { inner, start, len, pos: 0 }
    }
}

impl Read for Sub<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(self.len - self.pos) as usize;
        self.inner.seek(SeekFrom::Start(self.start + self.pos))?;
        let k = self.inner.read(&mut buf[..n])?;
        self.pos += k as u64;
        Ok(k)
    }
}

impl Seek for Sub<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(n) => self.len as i128 + n as i128,
            SeekFrom::Current(n) => self.pos as i128 + n as i128,
        };
        if pos < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

pub(crate) fn read_at(r: &mut dyn ReadSeek, pos: u64, len: usize) -> io::Result<Vec<u8>> {
    r.seek(SeekFrom::Start(pos))?;
    let mut v = vec![0u8; len];
    r.read_exact(&mut v)?;
    Ok(v)
}

/// Reads up to `len` bytes, stopping early at the end of the data.
pub(crate) fn read_some_at(r: &mut dyn ReadSeek, pos: u64, len: usize) -> io::Result<Vec<u8>> {
    r.seek(SeekFrom::Start(pos))?;
    let mut v = Vec::with_capacity(len);
    r.take(len as u64).read_to_end(&mut v)?;
    Ok(v)
}

pub(crate) fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

pub(crate) fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

pub(crate) fn le64(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

pub(crate) fn be16(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([b[i], b[i + 1]])
}

pub(crate) fn be32(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}
