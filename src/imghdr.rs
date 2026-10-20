//! Image size from the first bytes of a file, without decoding it.

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "webp", "avif", "gif", "bmp", "tif", "tiff", "heic", "heif", "jxl",
];

pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext)
}

/// Width and height of a JPEG, PNG, GIF, BMP, WebP, AVIF or HEIF image, if the header is in `b`.
pub fn dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.starts_with(&[0xFF, 0xD8]) {
        return jpeg(b);
    }
    if b.starts_with(b"\x89PNG") && b.len() >= 24 {
        return nonzero(be32(b, 16), be32(b, 20));
    }
    if b.starts_with(b"GIF8") && b.len() >= 10 {
        return nonzero(le16(b, 6), le16(b, 8));
    }
    if b.starts_with(b"BM") && b.len() >= 26 {
        let w = i32::from_le_bytes(b[18..22].try_into().ok()?);
        let h = i32::from_le_bytes(b[22..26].try_into().ok()?);
        return nonzero(w.unsigned_abs(), h.unsigned_abs());
    }
    if b.len() >= 30 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        return webp(b);
    }
    if b.len() >= 12 && &b[4..8] == b"ftyp" {
        return ispe(b);
    }
    None
}

fn jpeg(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        match m {
            0xFF => i += 1,
            0xD0..=0xD9 | 0x01 => i += 2,
            0xC0..=0xCF if m != 0xC4 && m != 0xC8 && m != 0xCC => {
                return nonzero(be16(b, i + 7), be16(b, i + 5));
            }
            _ => i += 2 + be16(b, i + 2) as usize,
        }
    }
    None
}

fn webp(b: &[u8]) -> Option<(u32, u32)> {
    match &b[12..16] {
        b"VP8X" => nonzero(1 + le24(b, 24), 1 + le24(b, 27)),
        b"VP8 " => nonzero(le16(b, 26) & 0x3FFF, le16(b, 28) & 0x3FFF),
        b"VP8L" => {
            let v = u32::from_le_bytes(b[21..25].try_into().ok()?);
            nonzero(1 + (v & 0x3FFF), 1 + ((v >> 14) & 0x3FFF))
        }
        _ => None,
    }
}

/// AVIF and HEIF keep the size in `ispe` boxes; the largest one is the main image.
fn ispe(b: &[u8]) -> Option<(u32, u32)> {
    let mut best: Option<(u32, u32)> = None;
    let mut i = 0;
    while let Some(p) = find(&b[i..], b"ispe") {
        let at = i + p + 8;
        if at + 8 <= b.len() {
            let (w, h) = (be32(b, at), be32(b, at + 4));
            if w > 0 && h > 0 && best.is_none_or(|(bw, bh)| (w as u64 * h as u64) > (bw as u64 * bh as u64)) {
                best = Some((w, h));
            }
        }
        i += p + 4;
    }
    best
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn nonzero(w: u32, h: u32) -> Option<(u32, u32)> {
    (w > 0 && h > 0).then_some((w, h))
}

fn be16(b: &[u8], i: usize) -> u32 {
    u16::from_be_bytes([b[i], b[i + 1]]) as u32
}

fn be32(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn le16(b: &[u8], i: usize) -> u32 {
    u16::from_le_bytes([b[i], b[i + 1]]) as u32
}

fn le24(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], 0])
}

#[cfg(test)]
mod tests {
    use super::dims;

    #[test]
    fn png() {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&800u32.to_be_bytes());
        b.extend_from_slice(&1200u32.to_be_bytes());
        assert_eq!(dims(&b), Some((800, 1200)));
    }

    #[test]
    fn jpeg_skips_segments() {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x06, 1, 2, 3, 4];
        b.extend_from_slice(&[0xFF, 0xC2, 0x00, 0x11, 0x08, 0x0B, 0xB8, 0x07, 0xD0, 0x03, 0, 0, 0, 0]);
        assert_eq!(dims(&b), Some((2000, 3000)));
    }

    #[test]
    fn avif_ispe() {
        let mut b = b"\0\0\0\x1cftypavif".to_vec();
        b.extend_from_slice(&[0; 20]);
        b.extend_from_slice(b"\0\0\0\x14ispe\0\0\0\0");
        b.extend_from_slice(&1280u32.to_be_bytes());
        b.extend_from_slice(&1810u32.to_be_bytes());
        assert_eq!(dims(&b), Some((1280, 1810)));
    }
}
