//! The cover an EPUB names for itself.

use crate::archive::{Archive, Entry};
use crate::text::extension;
use crate::{imghdr, xml};

const MAX_TEXT: u64 = 8 << 20;

struct Item {
    id: String,
    href: String,
    media: String,
    properties: String,
}

/// Index of the cover image of an EPUB, or `None` if the archive is not an EPUB
/// or names no cover we can find.
pub fn cover_index(ar: &mut dyn Archive) -> Option<usize> {
    let container = find(ar.entries(), "META-INF/container.xml")?;
    let container = read_text(ar, container)?;
    let opf = xml::tags(&container)
        .iter()
        .find(|t| t.is("rootfile"))
        .and_then(|t| t.attr("full-path"))
        .map(percent_decode)
        .and_then(|p| find(ar.entries(), &p))
        .or_else(|| ar.entries().iter().position(|e| e.name.to_ascii_lowercase().ends_with(".opf")))?;
    let base = parent(&ar.entries()[opf].name).to_owned();
    let package = read_text(ar, opf)?;

    let mut items = Vec::new();
    let mut meta_cover = None;
    let mut guide_cover = None;
    let mut first_page = None;
    for t in xml::tags(&package) {
        let attr = |name: &str| t.attr(name).unwrap_or("").to_owned();
        if t.is("item") {
            items.push(Item { id: attr("id"), href: attr("href"), media: attr("media-type"), properties: attr("properties") });
        } else if t.is("meta") && t.attr("name").is_some_and(|n| n.eq_ignore_ascii_case("cover")) {
            meta_cover = t.attr("content").map(str::to_owned);
        } else if t.is("reference") && guide_cover.is_none() && attr("type").to_ascii_lowercase().contains("cover") {
            guide_cover = t.attr("href").map(str::to_owned);
        } else if t.is("itemref") && first_page.is_none() {
            first_page = t.attr("idref").map(str::to_owned);
        }
    }
    let by_id = |id: &str| items.iter().find(|it| it.id == id).map(|it| it.href.clone());
    let called_cover = |it: &&Item| it.media.starts_with("image/") && (it.id.to_ascii_lowercase().contains("cover") || it.href.to_ascii_lowercase().contains("cover"));

    // In order of authority. Each may name an image, or an SVG or XHTML page that shows one.
    let candidates = [
        // EPUB 3: properties="cover-image".
        items.iter().find(|it| it.properties.split_whitespace().any(|p| p == "cover-image")).map(|it| it.href.clone()),
        // EPUB 2: <meta name="cover" content="item id"> (some books put a path there instead).
        meta_cover.as_deref().map(|id| by_id(id).unwrap_or_else(|| id.to_owned())),
        // An image item called "cover".
        items.iter().find(called_cover).map(|it| it.href.clone()),
        // The cover page, or else the first page of the book.
        guide_cover,
        first_page.as_deref().and_then(by_id),
    ];
    for href in candidates.into_iter().flatten() {
        if let Some(i) = find(ar.entries(), &join(&base, &href)).and_then(|i| raster(ar, i)) {
            return Some(i);
        }
    }
    None
}

/// The entry itself if it is a raster image (WIC cannot read SVG), else the first image
/// shown by it as an SVG or XHTML page.
fn raster(ar: &mut dyn Archive, i: usize) -> Option<usize> {
    let is_raster = |e: &Entry| imghdr::is_image_extension(&extension(&e.name));
    if is_raster(&ar.entries()[i]) {
        return Some(i);
    }
    let dir = parent(&ar.entries()[i].name).to_owned();
    let page = read_text(ar, i)?;
    let tags = xml::tags(&page);
    let src = tags.iter().find_map(|t| if t.is("img") { t.attr("src") } else if t.is("image") { t.attr("href") } else { None })?;
    find(ar.entries(), &join(&dir, src)).filter(|&j| is_raster(&ar.entries()[j]))
}

fn read_text(ar: &mut dyn Archive, i: usize) -> Option<String> {
    let bytes = ar.read(i, MAX_TEXT).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn find(entries: &[Entry], path: &str) -> Option<usize> {
    entries.iter().position(|e| e.name == path).or_else(|| entries.iter().position(|e| e.name.eq_ignore_ascii_case(path)))
}

fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// Resolves `href` (percent-encoded, maybe with a fragment) against the folder `base`.
fn join(base: &str, href: &str) -> String {
    let href = percent_decode(href.split('#').next().unwrap_or(href));
    let mut parts: Vec<&str> = if href.starts_with('/') { Vec::new() } else { base.split('/').filter(|s| !s.is_empty()).collect() };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{join, percent_decode};

    #[test]
    fn paths() {
        assert_eq!(join("OEBPS", "images/cover.jpg"), "OEBPS/images/cover.jpg");
        assert_eq!(join("OEBPS/text", "../images/c%20v.jpg#x"), "OEBPS/images/c v.jpg");
        assert_eq!(join("", "cover.jpg"), "cover.jpg");
        assert_eq!(percent_decode("%E8%A1%A8%E7%B4%99.jpg"), "表紙.jpg");
        assert_eq!(percent_decode("a%2"), "a%2");
    }
}
