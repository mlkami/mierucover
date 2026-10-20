//! Finding the cover image in a file.
//!
//! The pages are put in the order Explorer would show them and the first one is the cover,
//! with three corrections taken from real collections:
//! - an image named as a cover (`cover.jpg`, `01_hyoushi.jpg`, `表紙.jpg`) goes first,
//!   even when its name sorts after the page numbers;
//! - a small landscape store image (DLsite / FANZA, 560x420) in front of the real cover is skipped;
//! - whether a wide first image is a cover spread is decided later, at decode time
//!   (see `image`), using whether the following page is portrait.

use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};

use crate::archive::mobi;
use crate::archive::rar::Rar;
use crate::archive::sevenz::SevenZ;
use crate::archive::tar::Tar;
use crate::archive::zip::Zip;
use crate::archive::{self, Archive, Entry, Kind, ReadSeek};
use crate::text::{extension, file_name, stem};
use crate::{Error, Result, epub, imghdr, natsort};

/// Largest image we load.
pub const MAX_IMAGE: u64 = 96 << 20;
/// Largest archive inside an archive we load into memory (stored ones are read in place).
pub const MAX_NESTED: u64 = 512 << 20;
/// Bytes read from a page to learn its size.
const PEEK: usize = 256 << 10;
const MAX_DEPTH: u32 = 2;
/// In a solid archive, the most we decode just to get past the entries in front of the
/// cover. 7-Zip stores files by extension first, so the first page can sit at the very end.
const MAX_SOLID_SKIP: u64 = 32 << 20;

pub struct Cover {
    pub bytes: Vec<u8>,
    /// False when the image is known to be a single cover (EPUB, AZW3) and must not be cut.
    pub crop_ok: bool,
    /// Whether the page after the cover is portrait. A wide cover followed by portrait
    /// pages is a spread; followed by landscape pages it is just a landscape book.
    pub next_portrait: Option<bool>,
    /// Which entry was used (for diagnostics).
    pub source: String,
}

/// Finds the cover in the archive or book read by `r`.
pub fn find<R: Read + Seek + Send + 'static>(mut r: R) -> Result<Cover> {
    let kind = sniff(&mut r)?;
    if kind == Some(Kind::Rar) {
        // Small buffer: RAR is read header by header, each one just before its file's data.
        return choose(&mut Rar::open(BufReader::with_capacity(16 << 10, r))?, 0);
    }
    dispatch(&mut r, kind, 0)
}

pub(crate) fn find_in(r: &mut dyn ReadSeek, depth: u32) -> Result<Cover> {
    let kind = sniff(r)?;
    if kind == Some(Kind::Rar) {
        // The RAR reader needs to own its input.
        r.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        r.take(MAX_NESTED + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_NESTED {
            return Err(Error::TooLarge);
        }
        return choose(&mut Rar::open(Cursor::new(bytes))?, depth);
    }
    dispatch(r, kind, depth)
}

pub(crate) fn find_in_bytes(bytes: Vec<u8>, depth: u32) -> Result<Cover> {
    let kind = archive::sniff(&bytes);
    if kind == Some(Kind::Rar) {
        return choose(&mut Rar::open(Cursor::new(bytes))?, depth);
    }
    dispatch(&mut Cursor::new(bytes), kind, depth)
}

fn sniff(r: &mut dyn ReadSeek) -> Result<Option<Kind>> {
    r.seek(SeekFrom::Start(0))?;
    let mut head = Vec::with_capacity(512);
    r.take(512).read_to_end(&mut head)?;
    Ok(archive::sniff(&head))
}

fn dispatch(r: &mut dyn ReadSeek, kind: Option<Kind>, depth: u32) -> Result<Cover> {
    match kind {
        Some(Kind::Mobi) => {
            Ok(Cover { bytes: mobi::cover(r)?, crop_ok: false, next_portrait: None, source: "(azw3 cover)".into() })
        }
        Some(Kind::SevenZ) => choose(&mut SevenZ::open(r)?, depth),
        Some(Kind::Tar) => choose(&mut Tar::open(r)?, depth),
        Some(Kind::Rar) => Err(Error::Format("rar: unexpected")),
        Some(Kind::Zip) | None => {
            let mut zip = Zip::open(r)?;
            if let Some(i) = epub::cover_index(&mut zip) {
                if let Ok(bytes) = zip.read(i, MAX_IMAGE) {
                    let source = zip.entries()[i].name.clone();
                    return Ok(Cover { bytes, crop_ok: false, next_portrait: None, source });
                }
            }
            choose(&mut zip, depth)
        }
    }
}

fn choose(ar: &mut dyn Archive, depth: u32) -> Result<Cover> {
    let mut pages: Vec<usize> = (0..ar.entries().len()).filter(|&i| is_page(&ar.entries()[i])).collect();
    if pages.is_empty() {
        return nested(ar, depth);
    }
    let entries = ar.entries();
    pages.sort_by(|&a, &b| natsort::compare(&entries[a].name, &entries[b].name));
    if let Some(p) = pages.iter().position(|&i| is_cover_name(&entries[i].name)) {
        let i = pages.remove(p);
        pages.insert(0, i);
    }
    cheap_first(ar, &mut pages);

    // The first page that can be read (a few may be encrypted or use odd compression).
    let mut k = 0;
    let mut bytes = loop {
        match ar.read(pages[k], MAX_IMAGE) {
            Ok(b) => break b,
            Err(Error::TooLarge | Error::Unsupported(_)) if k + 1 < pages.len().min(3) => k += 1,
            Err(e) => return Err(e),
        }
    };
    let mut size = imghdr::dims(&bytes);

    // A store sample image (small and landscape) in front of a portrait cover.
    if let (Some((w, h)), Some(&next)) = (size, pages.get(k + 1)) {
        if w > h && h <= 600 {
            if let Some((nw, nh)) = peek_size(ar, next) {
                if nh > nw && nh as u64 * 2 >= h as u64 * 3 {
                    if let Ok(b) = ar.read(next, MAX_IMAGE) {
                        bytes = b;
                        size = Some((nw, nh));
                        k += 1;
                    }
                }
            }
        }
    }

    // Only a wide cover needs to know the shape of the next page.
    let wide = size.is_none_or(|(w, h)| w as u64 * 5 >= h as u64 * 6);
    let next_portrait = if wide { pages.get(k + 1).and_then(|&n| peek_size(ar, n)).map(|(w, h)| h > w) } else { None };
    let source = ar.entries()[pages[k]].name.clone();
    Ok(Cover { bytes, crop_ok: true, next_portrait, source })
}

/// No pages, but maybe books: an archive of EPUBs or of volumes.
fn nested(ar: &mut dyn Archive, depth: u32) -> Result<Cover> {
    if depth >= MAX_DEPTH {
        return Err(Error::NoCover);
    }
    let mut books: Vec<usize> = (0..ar.entries().len()).filter(|&i| is_book(&ar.entries()[i])).collect();
    let entries = ar.entries();
    books.sort_by(|&a, &b| natsort::compare(&entries[a].name, &entries[b].name));
    cheap_first(ar, &mut books);
    let mut last = Error::NoCover;
    for &i in books.iter().take(3) {
        match ar.nested_cover(i, depth + 1) {
            Ok(c) => return Ok(c),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// If the first choice sits deep in a solid block, the first one in order that is cheap
/// to reach takes its place (in a pack of volumes: another volume's cover).
fn cheap_first(ar: &dyn Archive, list: &mut Vec<usize>) {
    if list.first().is_some_and(|&i| ar.cost(i) > MAX_SOLID_SKIP) {
        if let Some(p) = list.iter().position(|&i| ar.cost(i) <= MAX_SOLID_SKIP) {
            let i = list.remove(p);
            list.insert(0, i);
        }
    }
}

fn peek_size(ar: &mut dyn Archive, i: usize) -> Option<(u32, u32)> {
    if ar.cost(i) > MAX_SOLID_SKIP {
        return None;
    }
    ar.read_prefix(i, PEEK).ok().and_then(|b| imghdr::dims(&b))
}

fn is_hidden(e: &Entry) -> bool {
    let name = file_name(&e.name);
    e.size == 0 || e.name.starts_with("__MACOSX/") || e.name.contains("/__MACOSX/") || name.starts_with("._")
}

fn is_page(e: &Entry) -> bool {
    !is_hidden(e) && imghdr::is_image_extension(&extension(&e.name))
}

fn is_book(e: &Entry) -> bool {
    const BOOKS: &[&str] = &["epub", "zip", "cbz", "rar", "cbr", "7z", "cb7", "tar", "cbt", "azw3", "azw", "mobi"];
    !is_hidden(e) && BOOKS.contains(&extension(&e.name).as_str())
}

/// Whether a page is named as the front cover.
///
/// Words are the runs of ASCII letters in the file name, so `001_AB01234567_cover`,
/// `cover00001` and `01_hyoushi` match. Back covers (`urahyoushi`, `back_cover`, `裏表紙`)
/// and pages that only show a cover (`mag04-41cover(page37)`) do not.
pub fn is_cover_name(path: &str) -> bool {
    let name = stem(path).to_lowercase();
    if name.contains("表紙") {
        return !name.contains('裏');
    }
    let words: Vec<&str> = name.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).collect();
    let cover = words.iter().any(|w| matches!(*w, "cover" | "hyoushi" | "hyousi" | "hyoshi" | "hyosi" | "front"));
    let not = words.iter().any(|w| matches!(*w, "back" | "ura" | "rear" | "inside" | "inner" | "page"));
    cover && !not
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Solid(Vec<Entry>, Vec<u64>);

    impl Archive for Solid {
        fn entries(&self) -> &[Entry] {
            &self.0
        }
        fn read(&mut self, _: usize, _: u64) -> Result<Vec<u8>> {
            Err(Error::NoCover)
        }
        fn cost(&self, i: usize) -> u64 {
            self.1[i]
        }
    }

    #[test]
    fn deep_solid_entry_gives_way_to_a_cheap_one() {
        let entries = ["01/0000.jpg", "03/00001.jpeg", "04/00001.jpeg"].map(|n| Entry { name: n.into(), size: 1 }).into();
        let ar = Solid(entries, vec![600 << 20, 0, 1 << 20]);
        let mut list = vec![0, 1, 2];
        cheap_first(&ar, &mut list);
        assert_eq!(list, [1, 0, 2]);
        let mut list = vec![2, 1];
        cheap_first(&ar, &mut list);
        assert_eq!(list, [2, 1]);
    }

    #[test]
    fn cover_names() {
        for yes in ["cover.jpg", "cover00001.jpg", "00_cover.jpg", "001_AB01234567_cover.jpg", "01_hyoushi.jpg", "hyousi.jpg", "01_02_0_hyoshi.jpg", "Title_05_000_Cover.jpg", "vol/表紙.png", "cover.avif"] {
            assert!(is_cover_name(yes), "{yes}");
        }
        for no in ["001.jpg", "180_urahyoushi.jpg", "back_cover.jpg", "裏表紙.jpg", "Title#05_210_mag04-41cover(page37).jpg", "discover.jpg", "covered.jpg"] {
            assert!(!is_cover_name(no), "{no}");
        }
    }
}
