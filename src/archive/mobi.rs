//! AZW3 / MOBI. The book names its cover record in the EXTH header (type 201).

use std::io::SeekFrom;

use super::{ReadSeek, be16, be32, read_at, read_some_at};
use crate::cover::MAX_IMAGE;
use crate::{Error, Result};

const EXTH_COVER: u32 = 201;
const EXTH_THUMB: u32 = 202;
const NONE: u32 = 0xFFFF_FFFF;

pub fn cover(r: &mut dyn ReadSeek) -> Result<Vec<u8>> {
    let len = r.seek(SeekFrom::End(0))?;
    let h = read_at(r, 0, 78)?;
    if &h[60..68] != b"BOOKMOBI" {
        return Err(Error::Format("mobi: not a MOBI book"));
    }
    let count = be16(&h, 76) as usize;
    let table = read_at(r, 78, count * 8)?;
    let offsets: Vec<u64> = (0..count).map(|i| be32(&table, i * 8) as u64).collect();
    let record = |i: usize| -> Option<(u64, u64)> {
        let start = *offsets.get(i)?;
        let end = offsets.get(i + 1).copied().unwrap_or(len);
        (start < end && end <= len).then_some((start, end - start))
    };

    let (start, size) = record(0).ok_or(Error::Format("mobi: no header record"))?;
    let r0 = read_some_at(r, start, size.min(1 << 16) as usize)?;
    if r0.len() < 132 || &r0[16..20] != b"MOBI" {
        return Err(Error::Format("mobi: no MOBI header"));
    }
    let first_image = be32(&r0, 108);
    if first_image == NONE {
        return Err(Error::NoCover);
    }

    let mut wanted = Vec::new();
    if be32(&r0, 128) & 0x40 != 0 {
        let exth = 16 + be32(&r0, 20) as usize;
        if r0.len() >= exth + 12 && &r0[exth..exth + 4] == b"EXTH" {
            let mut p = exth + 12;
            for _ in 0..be32(&r0, exth + 8) {
                if p + 12 > r0.len() {
                    break;
                }
                let (kind, n) = (be32(&r0, p), be32(&r0, p + 4) as usize);
                if (kind == EXTH_COVER || kind == EXTH_THUMB) && be32(&r0, p + 8) != NONE {
                    wanted.push((kind, be32(&r0, p + 8)));
                }
                p += n.max(8);
            }
        }
    }
    wanted.sort_by_key(|&(kind, _)| kind);

    // The named cover first, then the first image record if the book names none.
    let named = wanted.iter().map(|&(_, offset)| first_image as usize + offset as usize);
    for i in named.chain(first_image as usize..first_image as usize + 32) {
        let Some((start, size)) = record(i) else { continue };
        if size > MAX_IMAGE {
            continue;
        }
        let data = read_at(r, start, size as usize)?;
        if crate::imghdr::dims(&data).is_some() {
            return Ok(data);
        }
    }
    Err(Error::NoCover)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A minimal book: header record, a non-image record, two image records; EXTH points at the second image.
    fn book() -> Vec<u8> {
        let png = |w: u32, h: u32| {
            let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
            v.extend_from_slice(&w.to_be_bytes());
            v.extend_from_slice(&h.to_be_bytes());
            v
        };
        let mut r0 = vec![0u8; 16];
        r0.extend_from_slice(b"MOBI");
        r0.extend_from_slice(&232u32.to_be_bytes());
        r0.resize(108, 0);
        r0.extend_from_slice(&2u32.to_be_bytes()); // first image record
        r0.resize(128, 0);
        r0.extend_from_slice(&0x40u32.to_be_bytes());
        r0.resize(16 + 232, 0);
        r0.extend_from_slice(b"EXTH");
        r0.extend_from_slice(&24u32.to_be_bytes());
        r0.extend_from_slice(&1u32.to_be_bytes());
        r0.extend_from_slice(&EXTH_COVER.to_be_bytes());
        r0.extend_from_slice(&12u32.to_be_bytes());
        r0.extend_from_slice(&1u32.to_be_bytes());
        let records = [r0, b"FLIS....".to_vec(), png(10, 20), png(600, 900)];

        let mut out = vec![0u8; 78];
        out[60..68].copy_from_slice(b"BOOKMOBI");
        out[76..78].copy_from_slice(&(records.len() as u16).to_be_bytes());
        let mut offset = 78 + records.len() * 8 + 2;
        for r in &records {
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&[0; 4]);
            offset += r.len();
        }
        out.extend_from_slice(&[0, 0]);
        for r in &records {
            out.extend_from_slice(r);
        }
        out
    }

    #[test]
    fn uses_exth_cover() {
        let data = cover(&mut Cursor::new(book())).unwrap();
        assert_eq!(crate::imghdr::dims(&data), Some((600, 900)));
    }
}
