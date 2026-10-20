//! Runs against the dummy archives made by tools/make_fixtures.ps1; skipped when they are missing.

use std::fs::File;
use std::path::{Path, PathBuf};

use mierucover::com::{as_thumbnail_provider, create_provider};
use mierucover::{cover, image};
use windows::Win32::Graphics::Gdi::{DeleteObject, HBITMAP};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, STGM_READ, STGM_SHARE_DENY_NONE};
use windows::Win32::UI::Shell::{SHCreateStreamOnFileEx, WTS_ALPHATYPE};
use windows_core::HSTRING;

fn fixtures() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("fixtures");
    if dir.exists() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        Some(dir)
    } else {
        eprintln!("no fixtures: run tools/make_fixtures.ps1");
        None
    }
}

#[test]
fn picks_the_cover_and_layout() {
    let Some(dir) = fixtures() else { return };
    let cases = [
        ("zip_stored.zip", "001.jpg", "plain"),
        ("zip_deflate.cbz", "001.jpg", "plain"),
        ("tar_basic.tar", "001.jpg", "plain"),
        ("sevenz_solid.7z", "001.jpg", "plain"),
        ("zip_order.zip", "1.jpg", "plain"),
        ("zip_covername_cp932.zip", "表紙.jpg", "plain"),
        ("zip_store_sample.zip", "01.jpg", "plain"),
        ("zip_spread_left.zip", "000.jpg", "spread:left"),
        ("zip_spread_right.zip", "000.jpg", "spread:right"),
        ("zip_cgset.zip", "001.jpg", "plain"),
        ("zip_jacket.zip", "000a.jpg", "jacket:left"),
        ("zip_macosx.zip", "001.jpg", "plain"),
        ("zip_alpha.zip", "001.png", "plain"),
        ("epub2.epub", "OEBPS/img/front page.jpg", "plain"),
        ("epub3.epub", "OEBPS/img/main.jpg", "plain"),
        ("epub_svg.epub", "OEBPS/img/c.jpg", "plain"),
        ("zip_truncated.zip", "001.jpg", "plain"),
        ("zip_of_epubs.zip", "OEBPS/img/front page.jpg", "plain"),
        ("zip_of_zips.zip", "001.jpg", "plain"),
    ];
    for (file, source, layout) in cases {
        let c = cover::find(File::open(dir.join(file)).unwrap()).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(c.source, source, "{file}");
        let t = image::render(&c, 256).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(t.note.starts_with(layout), "{file}: {}", t.note);
        assert_eq!(t.width.max(t.height), 256, "{file}");
    }
}

#[test]
fn encrypted_has_no_cover() {
    let Some(dir) = fixtures() else { return };
    assert!(cover::find(File::open(dir.join("zip_encrypted.zip")).unwrap()).is_err());
}

#[test]
fn transparent_areas_become_white() {
    let Some(dir) = fixtures() else { return };
    let c = cover::find(File::open(dir.join("zip_alpha.zip")).unwrap()).unwrap();
    let t = image::render(&c, 256).unwrap();
    assert_eq!(&t.bgra[..4], &[255, 255, 255, 255]);
}

#[test]
fn provider_returns_a_bitmap_through_com() {
    let Some(dir) = fixtures() else { return };
    unsafe {
        let (init, provider) = as_thumbnail_provider(&create_provider()).unwrap();
        let path = HSTRING::from(dir.join("zip_jacket.zip").to_string_lossy().as_ref());
        let stream = SHCreateStreamOnFileEx(&path, (STGM_READ | STGM_SHARE_DENY_NONE).0, 0, false, None).unwrap();
        init.Initialize(&stream, 0).unwrap();
        let mut bitmap = HBITMAP::default();
        let mut alpha = WTS_ALPHATYPE::default();
        provider.GetThumbnail(256, &mut bitmap, &mut alpha).unwrap();
        assert!(!bitmap.is_invalid());
        let _ = DeleteObject(bitmap.into());
        // A second Initialize on the same object is refused, as the interface requires.
        assert!(init.Initialize(&stream, 0).is_err());
    }
}
