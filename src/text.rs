use windows::Win32::Globalization::{MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};

/// Decodes a file name from an archive header.
///
/// Names flagged as UTF-8, and names that happen to be valid UTF-8, are taken as they are.
/// Anything else is read as CP932 (Shift_JIS), which is what Japanese archivers on Windows write.
pub fn decode_name(bytes: &[u8], utf8: bool) -> String {
    if utf8 || bytes.is_ascii() {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => cp932(bytes),
    }
}

fn cp932(bytes: &[u8]) -> String {
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    let n = unsafe { MultiByteToWideChar(932, flags, bytes, None) };
    if n <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; n as usize];
    let n = unsafe { MultiByteToWideChar(932, flags, bytes, Some(&mut wide)) };
    String::from_utf16_lossy(&wide[..n.max(0) as usize])
}

/// The last component of a `/`-separated path.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The lowercase extension of the last path component, without the dot.
pub fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// The file name without its extension.
pub fn stem(path: &str) -> &str {
    let name = file_name(path);
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}
