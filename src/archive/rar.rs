//! RAR (also cbr), RAR 1.5 to 7, with the pure-Rust unrar-rs. It reads from the stream
//! Explorer gives us, so the provider can stay in its isolated process.

use std::io::{self, Read, Seek, Write};

use unrar_rs::{DecodeMode, RarArchive, RarError};

use super::{Archive, Entry};
use crate::{Error, Result};

pub struct Rar {
    archive: RarArchive,
    entries: Vec<Entry>,
    index: Vec<usize>,
    costs: Vec<u64>,
}

impl Rar {
    pub fn open<R: Read + Seek + Send + 'static>(r: R) -> Result<Self> {
        let mut archive = RarArchive::open(r).map_err(Error::lib)?;
        // Decode on this thread only: the stream belongs to the caller's apartment.
        archive.set_decode_mode(DecodeMode::Serial);
        archive.set_verify(false);
        let solid = archive.is_solid();
        let mut entries = Vec::new();
        let mut index = Vec::new();
        let mut costs = Vec::new();
        let mut before = 0;
        for m in archive.indexed_member_infos() {
            let info = &m.info;
            let size = info.unpacked_size.unwrap_or(0);
            if !(info.is_directory || info.is_encrypted || info.is_symlink || info.is_hardlink || !m.extractable) {
                entries.push(Entry { name: info.name.replace('\\', "/"), size });
                index.push(m.index);
                costs.push(before);
            }
            if solid {
                before += size;
            }
        }
        Ok(Rar { archive, entries, index, costs })
    }

    fn copy(&mut self, i: usize, out: &mut Limited) -> std::result::Result<(), RarError> {
        self.archive.by_index(self.index[i])?.copy_to(out).map(|_| ())
    }
}

impl Archive for Rar {
    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn read(&mut self, i: usize, limit: u64) -> Result<Vec<u8>> {
        if self.entries[i].size > limit {
            return Err(Error::TooLarge);
        }
        let mut out = Limited { data: Vec::new(), limit };
        if let Err(e) = self.copy(i, &mut out) {
            // Solid archives decode in order; going back to an earlier entry needs a restart.
            if !matches!(e, RarError::SolidOrderViolation { .. } | RarError::SolidStatePoisoned { .. }) {
                return Err(Error::lib(e));
            }
            self.archive.reset_solid_state();
            out.data.clear();
            self.copy(i, &mut out).map_err(Error::lib)?;
        }
        Ok(out.data)
    }

    fn cost(&self, i: usize) -> u64 {
        self.costs[i]
    }
}

struct Limited {
    data: Vec<u8>,
    limit: u64,
}

impl Write for Limited {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if (self.data.len() + buf.len()) as u64 > self.limit {
            return Err(io::Error::other("entry too large"));
        }
        self.data.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
