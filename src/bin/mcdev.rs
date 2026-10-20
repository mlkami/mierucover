//! Development tool for mierucover (not installed).
//!
//!   mcdev scan <path>... [--render] [--size N] [--out FILE]
//!       Finds the cover of every supported file under the paths and writes one line per file
//!       (TSV: status, times, chosen entry, size, layout). Images are decoded only with
//!       --render, and never written anywhere.
//!   mcdev thumb <file> <out.png> [--size N]
//!       Makes the thumbnail and saves it as PNG (for looking at test fixtures).
//!   mcdev com <path>... [--size N]
//!       Times the provider through its COM interfaces, as Explorer calls it.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use mierucover::com::{EXTENSIONS, as_thumbnail_provider, create_provider};
use mierucover::{Error, cover, image, imghdr};
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Gdi::{DeleteObject, HBITMAP};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, STGM_READ, STGM_SHARE_DENY_NONE};
use windows::Win32::UI::Shell::{SHCreateStreamOnFileEx, WTS_ALPHATYPE};
use windows_core::{HSTRING, Interface};

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut size = 256;
    let mut out = None;
    let mut render = false;
    let mut paths = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--size" => size = it.next().and_then(|s| s.parse().ok()).unwrap_or(256),
            "--out" => out = it.next().cloned(),
            "--render" => render = true,
            _ => paths.push(a.clone()),
        }
    }
    match args.first().map(String::as_str) {
        Some("scan") => scan(&paths, render, size, out),
        Some("thumb") if paths.len() == 2 => thumb(Path::new(&paths[0]), Path::new(&paths[1]), size),
        Some("com") => com(&paths, size),
        Some("experiment") => {
            for p in &paths {
                if let Err(e) = unsafe { experiment(Path::new(p), size) } {
                    eprintln!("error: {e}");
                }
            }
        }
        Some("io") => {
            for p in &paths {
                io_count(Path::new(p));
            }
        }
        Some("list") => {
            for p in &paths {
                list(Path::new(p));
            }
        }
        Some("probe") => {
            for p in &paths {
                if let Err(e) = unsafe { probe(Path::new(p)) } {
                    eprintln!("error: {e}");
                }
            }
        }
        _ => eprintln!("usage: mcdev scan <path>... [--render] [--size N] [--out FILE] | thumb <file> <out.png> [--size N] | com <path>... [--size N]"),
    }
}

fn collect(paths: &[String]) -> Vec<PathBuf> {
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    walk(&e.path(), out);
                }
            }
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if EXTENSIONS.contains(&format!(".{}", ext.to_ascii_lowercase()).as_str()) {
                out.push(p.to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    for p in paths {
        walk(Path::new(p), &mut out);
    }
    out.sort();
    out
}

fn scan(paths: &[String], render: bool, size: u32, out: Option<String>) {
    let files = collect(paths);
    let mut w: Box<dyn Write> = match &out {
        Some(p) => Box::new(File::create(p).expect("create output")),
        None => Box::new(std::io::stdout()),
    };
    writeln!(w, "status\tfind_ms\trender_ms\tsource\tdims\tnote\tpath").unwrap();
    let mut times = Vec::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for f in &files {
        let t0 = Instant::now();
        let found = File::open(f).map_err(Error::from).and_then(cover::find);
        let find_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let path = f.display();
        match found {
            Ok(c) => {
                let dims = imghdr::dims(&c.bytes).map_or("?".into(), |(w, h)| format!("{w}x{h}"));
                let (render_ms, note, status) = if render {
                    let t1 = Instant::now();
                    match image::render(&c, size) {
                        Ok(t) => (t1.elapsed().as_secs_f64() * 1000.0, t.note, "ok"),
                        Err(e) => (t1.elapsed().as_secs_f64() * 1000.0, format!("render error: {e}"), "RENDER_ERR"),
                    }
                } else {
                    (0.0, format!("next_portrait={:?}", c.next_portrait), "ok")
                };
                let kind = note.split(" (").next().unwrap_or("").to_owned();
                *kinds.entry(if status == "ok" { kind } else { note.clone() }).or_default() += 1;
                times.push(find_ms + render_ms);
                writeln!(w, "{status}\t{find_ms:.1}\t{render_ms:.1}\t{}\t{dims}\t{note}\t{path}", c.source).unwrap();
            }
            Err(e) => {
                *kinds.entry(format!("ERR {e}")).or_default() += 1;
                writeln!(w, "ERR\t{find_ms:.1}\t\t\t\t{e}\t{path}").unwrap();
            }
        }
    }
    times.sort_by(f64::total_cmp);
    let pct = |p: f64| times.get(((times.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0);
    eprintln!("files={} ok={} p50={:.1}ms p90={:.1}ms max={:.1}ms", files.len(), times.len(), pct(0.5), pct(0.9), pct(1.0));
    for (k, n) in kinds {
        eprintln!("  {n:>6}  {k}");
    }
}

fn thumb(file: &Path, png: &Path, size: u32) {
    let result = File::open(file).map_err(Error::from).and_then(cover::find).and_then(|c| {
        let t = image::render(&c, size)?;
        println!("source={} note={} {}x{}", c.source, t.note, t.width, t.height);
        unsafe { save_png(png, &t) }.map_err(Error::from)
    });
    if let Err(e) = result {
        eprintln!("error: {e}");
    }
}

unsafe fn save_png(path: &Path, t: &image::Thumb) -> windows_core::Result<()> {
    unsafe {
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = f.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(path.to_string_lossy().as_ref()), GENERIC_WRITE.0)?;
        let encoder = f.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let (mut frame, mut props) = (None, None);
        encoder.CreateNewFrame(&mut frame, &mut props)?;
        let frame = frame.ok_or(windows_core::Error::empty())?;
        frame.Initialize(props.as_ref())?;
        frame.SetSize(t.width, t.height)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WritePixels(t.height, t.width * 4, &t.bgra)?;
        frame.Commit()?;
        encoder.Commit()
    }
}

/// Counts the reads, seeks and bytes it takes to find the cover.
fn io_count(path: &Path) {
    use std::io::{Read, Seek, SeekFrom};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    struct Counting(File, Arc<[AtomicU64; 3]>);
    impl Read for Counting {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.0.read(buf)?;
            self.1[0].fetch_add(1, Relaxed);
            self.1[1].fetch_add(n as u64, Relaxed);
            Ok(n)
        }
    }
    impl Seek for Counting {
        fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
            self.1[2].fetch_add(1, Relaxed);
            self.0.seek(to)
        }
    }
    let Ok(file) = File::open(path) else { return };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let stats = Arc::new([AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)]);
    let t0 = Instant::now();
    let result = cover::find(Counting(file, stats.clone()));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let (reads, bytes, seeks) = (stats[0].load(Relaxed), stats[1].load(Relaxed), stats[2].load(Relaxed));
    println!(
        "{ms:7.1}ms  file={:.1}MB read={:.1}MB reads={reads} seeks={seeks}  {}",
        len as f64 / 1e6,
        bytes as f64 / 1e6,
        result.map_or_else(|e| e.to_string(), |c| c.source)
    );
}

/// Lists the entries of a zip-family file with the first bytes of each (names and magic only).
fn list(path: &Path) {
    use mierucover::archive::{Archive, zip::Zip};
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => return eprintln!("error: {e}"),
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("7z")) {
        match mierucover::archive::sevenz::SevenZ::open(&mut file) {
            Ok(z) => {
                for (k, e) in z.entries().iter().enumerate().take(40) {
                    println!("{k:>4} {:>10}  {}", e.size, e.name);
                }
            }
            Err(e) => println!("{e}"),
        }
        return;
    }
    let mut zip = match Zip::open(&mut file) {
        Ok(z) => z,
        Err(e) => return println!("{e} (file size {len})"),
    };
    for i in 0..zip.entries().len().min(60) {
        let head = zip.read_prefix(i, 8).map(|b| b.iter().map(|x| format!("{x:02X}")).collect::<String>()).unwrap_or_else(|e| e.to_string());
        let e = &zip.entries()[i];
        println!("{:>10}  {head:<16}  {}", e.size, e.name);
    }
}

/// Compares decoding strategies on the cover of each file: time, and mean absolute pixel
/// difference against a full-quality decode (the pixels themselves are not kept).
unsafe fn experiment(path: &Path, cx: u32) -> mierucover::Result<()> {
    let c = cover::find(File::open(path)?)?;
    let base = unsafe { decode_variant(&c.bytes, None, WICBitmapInterpolationModeHighQualityCubic, cx)? };
    print!("{:>9}", format!("{:.0}ms", base.1));
    let levels = unsafe { progressive_levels(&c.bytes) };
    for level in 0..levels.min(4) {
        let v = unsafe { decode_variant(&c.bytes, Some(level), WICBitmapInterpolationModeHighQualityCubic, cx)? };
        print!("  L{level}:{:.0}ms/d{:.2}", v.1, diff(&base.0, &v.0));
    }
    let v = unsafe { decode_variant(&c.bytes, None, WICBitmapInterpolationModeFant, cx)? };
    print!("  fant:{:.0}ms/d{:.2}", v.1, diff(&base.0, &v.0));
    let t0 = Instant::now();
    let t = image::render(&c, cx)?;
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("  render:{ms:.0}ms/d{:.2}  levels={levels} {}", diff(&base.0, &t.bgra), c.source);
    Ok(())
}

fn diff(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() {
        return f64::NAN;
    }
    a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64).sum::<f64>() / a.len() as f64
}

unsafe fn progressive_levels(bytes: &[u8]) -> u32 {
    unsafe {
        let run = || -> windows_core::Result<u32> {
            let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = f.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let frame = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?.GetFrame(0)?;
            frame.cast::<IWICProgressiveLevelControl>()?.GetLevelCount()
        };
        run().unwrap_or(0)
    }
}

unsafe fn decode_variant(bytes: &[u8], level: Option<u32>, mode: WICBitmapInterpolationMode, cx: u32) -> windows_core::Result<(Vec<u8>, f64)> {
    unsafe {
        let t0 = Instant::now();
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = f.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let frame = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?.GetFrame(0)?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h)?;
        let (tw, th) = if w >= h { (cx.min(w), ((h as u64 * cx.min(w) as u64 + w as u64 / 2) / w as u64) as u32) } else { (((w as u64 * cx.min(h) as u64 + h as u64 / 2) / h as u64) as u32, cx.min(h)) };
        if let Some(l) = level {
            let p = frame.cast::<IWICProgressiveLevelControl>()?;
            p.SetCurrentLevel(l.min(p.GetLevelCount()? - 1))?;
        }
        let mut source: IWICBitmapSource = frame.cast()?;
        if let Ok(t) = frame.cast::<IWICBitmapSourceTransform>() {
            for d in [8, 4, 2] {
                let (mut sw, mut sh) = (w.div_ceil(d), h.div_ceil(d));
                if sw < tw || sh < th || t.GetClosestSize(&mut sw, &mut sh).is_err() || sw < tw || sh < th {
                    continue;
                }
                let mut pf = GUID_WICPixelFormat32bppBGR;
                t.GetClosestPixelFormat(&mut pf)?;
                let bpp = if pf == GUID_WICPixelFormat24bppBGR { 3 } else if pf == GUID_WICPixelFormat8bppGray { 1 } else { 4 };
                let stride = (sw * bpp + 3) & !3;
                let mut buf = vec![0u8; (stride * sh) as usize];
                t.CopyPixels(std::ptr::null(), sw, sh, &pf, WICBitmapTransformRotate0, stride, &mut buf)?;
                source = f.CreateBitmapFromMemory(sw, sh, &pf, stride, &buf)?.cast()?;
                break;
            }
        }
        let conv = f.CreateFormatConverter()?;
        conv.Initialize(&source, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
        let scaler = f.CreateBitmapScaler()?;
        scaler.Initialize(&conv, tw, th, mode)?;
        let mut px = vec![0u8; (tw * th * 4) as usize];
        scaler.CopyPixels(std::ptr::null(), tw * 4, &mut px)?;
        Ok((px, t0.elapsed().as_secs_f64() * 1000.0))
    }
}

/// Prints how WIC sees the cover image (codec, pixel format, native downscaling), not its pixels.
unsafe fn probe(path: &Path) -> mierucover::Result<()> {
    let c = cover::find(File::open(path)?)?;
    unsafe {
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = f.CreateStream()?;
        stream.InitializeFromMemory(&c.bytes)?;
        let decoder = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h)?;
        let format = frame.GetPixelFormat()?;
        let head: Vec<String> = c.bytes.iter().take(4).map(|b| format!("{b:02X}")).collect();
        let progressive = c.bytes.windows(2).any(|m| m == [0xFF, 0xC2]);
        print!("{} {}x{} format={format:?} head={} progressive_marker={progressive}", c.source, w, h, head.join(""));
        match frame.cast::<IWICBitmapSourceTransform>() {
            Ok(t) => {
                let (mut sw, mut sh) = (w.div_ceil(8), h.div_ceil(8));
                let r = t.GetClosestSize(&mut sw, &mut sh);
                let mut pf = GUID_WICPixelFormat32bppBGR;
                let r2 = t.GetClosestPixelFormat(&mut pf);
                println!(" transform: closest(1/8)={sw}x{sh} {:?} pixfmt={pf:?} {:?}", r.is_ok(), r2.is_ok());
            }
            Err(e) => println!(" transform: none ({e})"),
        }
    }
    Ok(())
}

fn com(paths: &[String], size: u32) {
    let files = collect(paths);
    let mut times = Vec::new();
    let mut failed = 0;
    for f in &files {
        let t0 = Instant::now();
        let ok = unsafe { com_one(f, size) }.is_ok();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if ok {
            times.push(ms);
        } else {
            failed += 1;
        }
        println!("{}\t{ms:.1}\t{}", if ok { "ok" } else { "FAIL" }, f.display());
    }
    times.sort_by(f64::total_cmp);
    let pct = |p: f64| times.get(((times.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0);
    eprintln!("files={} ok={} failed={failed} p50={:.1}ms p90={:.1}ms max={:.1}ms", files.len(), times.len(), pct(0.5), pct(0.9), pct(1.0));
}

unsafe fn com_one(path: &Path, size: u32) -> windows_core::Result<()> {
    unsafe {
        let (init, provider) = as_thumbnail_provider(&create_provider())?;
        let stream = SHCreateStreamOnFileEx(&HSTRING::from(path.to_string_lossy().as_ref()), (STGM_READ | STGM_SHARE_DENY_NONE).0, 0, false, None)?;
        init.Initialize(&stream, 0)?;
        let mut bitmap = HBITMAP::default();
        let mut alpha = WTS_ALPHATYPE::default();
        provider.GetThumbnail(size, &mut bitmap, &mut alpha)?;
        let _ = DeleteObject(bitmap.into());
        Ok(())
    }
}
