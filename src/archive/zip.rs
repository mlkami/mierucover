//! Zip (also cbz and epub). Only the central directory at the end and the chosen entries
//! are read, so the cost does not depend on the size of the archive.

use std::io::SeekFrom;

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZFlush};

use super::{Archive, Entry, ReadSeek, Sub, le16, le32, le64, read_at, read_some_at};
use crate::cover::{self, Cover};
use crate::text::decode_name;
use crate::{Error, Result};

const EOCD: u32 = 0x0605_4b50;
const EOCD64: u32 = 0x0606_4b50;
const EOCD64_LOCATOR: u32 = 0x0706_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;

const MAX_DIRECTORY: u64 = 64 << 20;
const MAX_ENTRIES: usize = 200_000;

struct Info {
    method: u16,
    flags: u16,
    csize: u64,
    offset: u64,
}

pub struct Zip<'a> {
    r: &'a mut dyn ReadSeek,
    entries: Vec<Entry>,
    info: Vec<Info>,
    /// Bytes of other data in front of the archive (self-extractors, joined files).
    base: u64,
}

impl<'a> Zip<'a> {
    pub fn open(r: &'a mut dyn ReadSeek) -> Result<Self> {
        let len = r.seek(SeekFrom::End(0))?;
        if len < 30 {
            return Err(Error::Format("zip: too small"));
        }
        match central_directory(r, len)? {
            Some((entries, info, base)) => Ok(Zip { r, entries, info, base }),
            // Cut short (an unfinished download): list what the local headers still describe.
            None => {
                let (entries, info) = local_headers(r, len)?;
                Ok(Zip { r, entries, info, base: 0 })
            }
        }
    }

    fn check(&self, i: usize) -> Result<()> {
        let info = &self.info[i];
        if info.flags & 1 != 0 {
            Err(Error::Unsupported("zip: encrypted"))
        } else if info.method != 0 && info.method != 8 {
            Err(Error::Unsupported("zip: compression method"))
        } else {
            Ok(())
        }
    }

    fn data_start(&mut self, i: usize) -> Result<u64> {
        let pos = self.base + self.info[i].offset;
        let h = read_at(self.r, pos, 30)?;
        if le32(&h, 0) != LOCAL {
            return Err(Error::Format("zip: bad local header"));
        }
        Ok(pos + 30 + le16(&h, 26) as u64 + le16(&h, 28) as u64)
    }
}

/// Reads the central directory; `None` if the file has no end record.
fn central_directory(r: &mut dyn ReadSeek, len: u64) -> Result<Option<(Vec<Entry>, Vec<Info>, u64)>> {
    // The end record is at most a 64 KiB comment away from the end.
    let tail_len = len.min(22 + 0xFFFF + 20) as usize;
    let tail_pos = len - tail_len as u64;
    let tail = read_at(r, tail_pos, tail_len)?;
    let Some(i) = (0..=tail_len - 22).rev().find(|&i| le32(&tail, i) == EOCD) else {
        return Ok(None);
    };

    let mut cd_size = le32(&tail, i + 12) as u64;
    let mut cd_offset = le32(&tail, i + 16) as u64;
    let mut base = 0;
    if le16(&tail, i + 10) == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        if i < 20 || le32(&tail, i - 20) != EOCD64_LOCATOR {
            return Err(Error::Format("zip64: no locator"));
        }
        let record = read_at(r, le64(&tail, i - 12), 56)?;
        if le32(&record, 0) != EOCD64 {
            return Err(Error::Format("zip64: bad end record"));
        }
        cd_size = le64(&record, 40);
        cd_offset = le64(&record, 48);
    } else {
        // Data in front of the archive (self-extractors, joined files) shifts every offset.
        base = (tail_pos + i as u64).saturating_sub(cd_offset + cd_size);
    }
    if cd_size > MAX_DIRECTORY {
        return Err(Error::TooLarge);
    }
    let cd = read_at(r, base + cd_offset, cd_size as usize)?;

    let mut entries = Vec::new();
    let mut info = Vec::new();
    let mut p = 0;
    while p + 46 <= cd.len() && le32(&cd, p) == CENTRAL && entries.len() < MAX_ENTRIES {
        let flags = le16(&cd, p + 8);
        let method = le16(&cd, p + 10);
        let mut csize = le32(&cd, p + 20) as u64;
        let mut size = le32(&cd, p + 24) as u64;
        let name_end = p + 46 + le16(&cd, p + 28) as usize;
        let extra_end = name_end + le16(&cd, p + 30) as usize;
        let next = extra_end + le16(&cd, p + 32) as usize;
        let mut offset = le32(&cd, p + 42) as u64;
        if extra_end > cd.len() {
            break;
        }
        let mut name = decode_name(&cd[p + 46..name_end], flags & 0x800 != 0);
        read_extra(&cd[name_end..extra_end], &mut name, [&mut size, &mut csize, &mut offset]);
        p = next;
        push(&mut entries, &mut info, name, size, Info { method, flags, csize, offset });
    }
    Ok(Some((entries, info, base)))
}

/// Walks the local headers from the start, for files without a central directory.
/// Stops where the data ends or where a size is only given after the data.
fn local_headers(r: &mut dyn ReadSeek, len: u64) -> Result<(Vec<Entry>, Vec<Info>)> {
    let mut entries = Vec::new();
    let mut info = Vec::new();
    let mut pos = 0u64;
    while pos + 30 <= len && entries.len() < MAX_ENTRIES {
        let h = read_at(r, pos, 30)?;
        if le32(&h, 0) != LOCAL {
            break;
        }
        let flags = le16(&h, 6);
        let method = le16(&h, 8);
        let mut csize = le32(&h, 18) as u64;
        let mut size = le32(&h, 22) as u64;
        let (nlen, elen) = (le16(&h, 26) as usize, le16(&h, 28) as usize);
        let data = pos + 30 + (nlen + elen) as u64;
        if data > len || (flags & 8 != 0 && csize == 0) {
            break;
        }
        let ne = read_at(r, pos + 30, nlen + elen)?;
        let mut name = decode_name(&ne[..nlen], flags & 0x800 != 0);
        let mut unused = 0;
        read_extra(&ne[nlen..], &mut name, [&mut size, &mut csize, &mut unused]);
        if data + csize > len {
            break;
        }
        push(&mut entries, &mut info, name, size, Info { method, flags, csize, offset: pos });
        pos = data + csize;
        if flags & 8 != 0 {
            // A data descriptor follows: 12 to 24 bytes, with or without a signature.
            pos = [16, 12, 24, 20]
                .iter()
                .map(|&n| pos + n)
                .find(|&p| p + 4 <= len && read_at(r, p, 4).is_ok_and(|b| le32(&b, 0) == LOCAL))
                .unwrap_or(len);
        }
    }
    if entries.is_empty() {
        return Err(Error::Format("zip: no end record"));
    }
    Ok((entries, info))
}

/// Applies the extra fields: Zip64 sizes (present only for the fields that overflowed,
/// in the order size, compressed size, offset) and the Info-ZIP UTF-8 name.
fn read_extra(extra: &[u8], name: &mut String, fields: [&mut u64; 3]) {
    let mut fields = fields;
    let mut e = 0;
    while e + 4 <= extra.len() {
        let id = le16(extra, e);
        let data = e + 4;
        let end = (data + le16(extra, e + 2) as usize).min(extra.len());
        match id {
            0x0001 => {
                let mut k = data;
                for field in fields.iter_mut() {
                    if **field == 0xFFFF_FFFF && k + 8 <= end {
                        **field = le64(extra, k);
                        k += 8;
                    }
                }
            }
            0x7075 if end > data + 5 => {
                if let Ok(s) = std::str::from_utf8(&extra[data + 5..end]) {
                    *name = s.to_owned();
                }
            }
            _ => {}
        }
        e = end;
    }
}

fn push(entries: &mut Vec<Entry>, info: &mut Vec<Info>, name: String, size: u64, i: Info) {
    let name = name.replace('\\', "/");
    if !name.ends_with('/') {
        entries.push(Entry { name, size });
        info.push(i);
    }
}

impl Archive for Zip<'_> {
    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn read(&mut self, i: usize, limit: u64) -> Result<Vec<u8>> {
        self.check(i)?;
        let (size, csize, method) = (self.entries[i].size, self.info[i].csize, self.info[i].method);
        if size > limit || csize > limit {
            return Err(Error::TooLarge);
        }
        let start = self.data_start(i)?;
        let data = read_at(self.r, start, csize as usize)?;
        if method == 0 {
            return Ok(data);
        }
        miniz_oxide::inflate::decompress_to_vec_with_limit(&data, size as usize)
            .map_err(|_| Error::Format("zip: bad deflate data"))
    }

    fn read_prefix(&mut self, i: usize, n: usize) -> Result<Vec<u8>> {
        self.check(i)?;
        let (size, csize, method) = (self.entries[i].size, self.info[i].csize, self.info[i].method);
        let start = self.data_start(i)?;
        let n = n.min(size as usize);
        if method == 0 {
            return Ok(read_some_at(self.r, start, n)?);
        }
        // Images barely compress, so a little more than `n` compressed bytes is enough.
        let take = (n as u64 + n as u64 / 4 + 4096).min(csize) as usize;
        let data = read_some_at(self.r, start, take)?;
        let mut state = InflateState::new_boxed(DataFormat::Raw);
        let mut out = vec![0u8; n];
        let written = inflate(&mut state, &data, &mut out, MZFlush::None).bytes_written;
        out.truncate(written);
        Ok(out)
    }

    fn nested_cover(&mut self, i: usize, depth: u32) -> Result<Cover> {
        self.check(i)?;
        if self.info[i].method == 0 {
            // Stored: read the inner archive in place instead of loading it.
            let start = self.data_start(i)?;
            let mut sub = Sub::new(&mut *self.r, start, self.entries[i].size);
            return cover::find_in(&mut sub, depth);
        }
        let bytes = self.read(i, cover::MAX_NESTED)?;
        cover::find_in_bytes(bytes, depth)
    }
}
