//! Tar (also cbt). There is no index, so listing walks the headers; file data is skipped.

use std::io::SeekFrom;

use super::{Archive, Entry, ReadSeek, read_at};
use crate::text::decode_name;
use crate::{Error, Result};

const MAX_ENTRIES: usize = 200_000;
const MAX_META: u64 = 1 << 20;

pub struct Tar<'a> {
    r: &'a mut dyn ReadSeek,
    entries: Vec<Entry>,
    offsets: Vec<u64>,
}

impl<'a> Tar<'a> {
    pub fn open(r: &'a mut dyn ReadSeek) -> Result<Self> {
        let len = r.seek(SeekFrom::End(0))?;
        let mut entries = Vec::new();
        let mut offsets = Vec::new();
        let mut long_name: Option<String> = None;
        let mut pax_path: Option<String> = None;
        let mut pos = 0u64;
        while pos + 512 <= len && entries.len() < MAX_ENTRIES {
            let h = read_at(r, pos, 512)?;
            if h.iter().all(|&b| b == 0) {
                break;
            }
            if !checksum_ok(&h) {
                if pos == 0 {
                    return Err(Error::Format("tar: bad header"));
                }
                break;
            }
            let size = parse_size(&h[124..136]);
            let data = pos + 512;
            match h[156] {
                b'L' if size <= MAX_META => {
                    let v = read_at(r, data, size as usize)?;
                    long_name = Some(decode_name(cstr(&v), false));
                }
                b'x' if size <= MAX_META => {
                    let v = read_at(r, data, size as usize)?;
                    pax_path = pax_value(&v, "path");
                }
                b'0' | 0 | b'7' => {
                    let name = pax_path.take().or(long_name.take()).unwrap_or_else(|| header_name(&h));
                    let name = name.replace('\\', "/");
                    if size > 0 && !name.ends_with('/') {
                        entries.push(Entry { name, size });
                        offsets.push(data);
                    }
                }
                b'g' => {}
                _ => {
                    long_name = None;
                    pax_path = None;
                }
            }
            pos = data + size.div_ceil(512) * 512;
        }
        Ok(Tar { r, entries, offsets })
    }
}

impl Archive for Tar<'_> {
    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn read(&mut self, i: usize, limit: u64) -> Result<Vec<u8>> {
        let size = self.entries[i].size;
        if size > limit {
            return Err(Error::TooLarge);
        }
        Ok(read_at(self.r, self.offsets[i], size as usize)?)
    }

    fn read_prefix(&mut self, i: usize, n: usize) -> Result<Vec<u8>> {
        let n = n.min(self.entries[i].size as usize);
        Ok(read_at(self.r, self.offsets[i], n)?)
    }
}

fn header_name(h: &[u8]) -> String {
    let name = decode_name(cstr(&h[0..100]), false);
    if &h[257..262] == b"ustar" {
        let prefix = cstr(&h[345..500]);
        if !prefix.is_empty() {
            return format!("{}/{}", decode_name(prefix, false), name);
        }
    }
    name
}

fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

fn checksum_ok(h: &[u8]) -> bool {
    let stored = parse_octal(&h[148..156]);
    let sum: u64 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u64 }).sum();
    stored == sum
}

fn parse_size(f: &[u8]) -> u64 {
    if f[0] & 0x80 != 0 {
        // GNU base-256 for sizes over 8 GiB.
        f[1..].iter().fold(0u64, |acc, &b| acc.saturating_mul(256).saturating_add(b as u64))
    } else {
        parse_octal(f)
    }
}

fn parse_octal(f: &[u8]) -> u64 {
    f.iter()
        .skip_while(|&&b| b == b' ')
        .take_while(|&&b| (b'0'..=b'7').contains(&b))
        .fold(0u64, |acc, &b| acc.saturating_mul(8).saturating_add((b - b'0') as u64))
}

/// Pax records are "<len> <key>=<value>\n".
fn pax_value(data: &[u8], key: &str) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    text.lines().find_map(|line| {
        let (_, kv) = line.split_once(' ')?;
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn header(name: &str, size: u64, kind: u8) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[148..156].copy_from_slice(b"        ");
        let sum: u64 = h.iter().map(|&b| b as u64).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        h
    }

    #[test]
    fn lists_and_reads() {
        let mut t = header("book/002.jpg", 3, b'0');
        t.extend_from_slice(b"two");
        t.resize(1024, 0);
        t.extend(header("book/001.jpg", 3, b'0'));
        t.extend_from_slice(b"one");
        t.resize(2048 + 1024, 0);
        let mut c = Cursor::new(t);
        let mut tar = Tar::open(&mut c).unwrap();
        let names: Vec<_> = tar.entries().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["book/002.jpg", "book/001.jpg"]);
        assert_eq!(tar.read(1, 100).unwrap(), b"one");
    }
}
