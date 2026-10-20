//! 7z (also cb7). The header is at the end; solid blocks are decoded only up to the entry read.

use std::io::{BufReader, SeekFrom};

use sevenz_rust2::{ArchiveReader, Password};

use super::{Archive, Entry, ReadSeek};
use crate::{Error, Result};

pub struct SevenZ<'a> {
    reader: ArchiveReader<BufReader<&'a mut dyn ReadSeek>>,
    entries: Vec<Entry>,
    names: Vec<String>,
    costs: Vec<u64>,
}

impl<'a> SevenZ<'a> {
    pub fn open(r: &'a mut dyn ReadSeek) -> Result<Self> {
        r.seek(SeekFrom::Start(0))?;
        let mut reader = ArchiveReader::new(BufReader::with_capacity(1 << 16, r), Password::empty()).map_err(Error::lib)?;
        reader.set_thread_count(1);
        let archive = reader.archive();
        let mut entries = Vec::new();
        let mut names = Vec::new();
        let mut costs = Vec::new();
        // Files of a block are stored one after another; in a solid block each costs the
        // sizes of the ones before it. (7-Zip orders them by extension, then by name.)
        let mut block = None;
        let mut before = 0;
        for (i, f) in archive.files.iter().enumerate() {
            let b = archive.stream_map.file_block_index.get(i).copied().flatten();
            if b != block {
                block = b;
                before = 0;
            }
            if f.is_directory() || !f.has_stream() {
                continue;
            }
            entries.push(Entry { name: f.name().replace('\\', "/"), size: f.size() });
            names.push(f.name().to_owned());
            costs.push(before);
            before += f.size();
        }
        Ok(SevenZ { reader, entries, names, costs })
    }
}

impl Archive for SevenZ<'_> {
    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn read(&mut self, i: usize, limit: u64) -> Result<Vec<u8>> {
        if self.entries[i].size > limit {
            return Err(Error::TooLarge);
        }
        self.reader.read_file(&self.names[i]).map_err(Error::lib)
    }

    fn cost(&self, i: usize) -> u64 {
        self.costs[i]
    }
}
