//! Decoding the cover with WIC and making the thumbnail.
//!
//! JPEGs are decoded straight at 1/2, 1/4 or 1/8 size when that is still big enough, which
//! is most of the work saved. Every format WIC has a codec for works (WebP, AVIF and HEIC
//! come with Windows extensions).
//!
//! Wide covers: a scan of the whole jacket (2:1 or wider, with flaps) or of a front+back
//! spread (1.2:1 or wider, followed by portrait pages). Which side is the front differs
//! between books, so both candidate areas next to the spine are compared: the front cover
//! is the more colorful and detailed one. If neither stands out, the whole image is kept.

use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows_core::{GUID, Interface};

use crate::cover::Cover;
use crate::{Error, Result};

/// A side must score this much higher than the other to be taken as the front cover.
const SIDE_RATIO: f64 = 1.2;

pub struct Thumb {
    pub width: u32,
    pub height: u32,
    /// Top-down BGRA rows, opaque.
    pub bgra: Vec<u8>,
    /// How the image was laid out (for diagnostics), e.g. "plain" or "jacket:left (52/31)".
    pub note: String,
}

#[derive(PartialEq, Clone, Copy)]
enum Layout {
    Plain,
    Spread,
    Jacket,
}

pub fn render(cover: &Cover, cx: u32) -> Result<Thumb> {
    let cx = cx.clamp(16, 4096);
    unsafe {
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = f.CreateStream()?;
        stream.InitializeFromMemory(&cover.bytes)?;
        let decoder = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h)?;
        if w == 0 || h == 0 {
            return Err(Error::Format("image: empty"));
        }

        let (w64, h64) = (w as u64, h as u64);
        let layout = if !cover.crop_ok {
            Layout::Plain
        } else if w64 * 10 >= h64 * 18 {
            Layout::Jacket
        } else if w64 * 10 >= h64 * 12 && cover.next_portrait == Some(true) {
            Layout::Spread
        } else {
            Layout::Plain
        };

        if layout == Layout::Plain {
            let (tw, th) = fit(w, h, cx);
            let source = scaled_source(&f, &frame, w, h, tw, th)?;
            let px = resample(&f, &source, None, tw, th)?;
            return Ok(finish(tw, th, px, "plain".into()));
        }

        // Decode once at a moderate size, pick the side, then cut and scale from that.
        let ih = h.min(cx.max(256));
        let iw = ((w64 * ih as u64 + h64 / 2) / h64).max(2) as u32;
        let source = scaled_source(&f, &frame, w, h, iw, ih)?;
        let inter = resample(&f, &source, None, iw, ih)?;
        let (left, right) = candidates(iw, ih, layout == Layout::Jacket);
        let (sl, sr) = (score(&inter, iw, &left), score(&inter, iw, &right));
        let pick = if sl > sr * SIDE_RATIO {
            Some((left, "left"))
        } else if sr > sl * SIDE_RATIO {
            Some((right, "right"))
        } else {
            None
        };

        let bitmap: IWICBitmapSource = f.CreateBitmapFromMemory(iw, ih, &GUID_WICPixelFormat32bppPBGRA, iw * 4, &inter)?.cast()?;
        let kind = if layout == Layout::Jacket { "jacket" } else { "spread" };
        let (rect, side) = match pick {
            Some((r, side)) => (Some(r), side),
            None => (None, "whole"),
        };
        let (cw, ch) = rect.map_or((iw, ih), |r| (r.Width as u32, r.Height as u32));
        let (tw, th) = fit(cw, ch, cx);
        let px = resample(&f, &bitmap, rect.as_ref(), tw, th)?;
        Ok(finish(tw, th, px, format!("{kind}:{side} ({sl:.0}/{sr:.0})")))
    }
}

/// A source at least `need_w` x `need_h`, using the decoder's own downscaling when it has one.
unsafe fn scaled_source(f: &IWICImagingFactory, frame: &IWICBitmapFrameDecode, w: u32, h: u32, need_w: u32, need_h: u32) -> Result<IWICBitmapSource> {
    unsafe {
        if let Ok(t) = frame.cast::<IWICBitmapSourceTransform>() {
            for d in [8, 4, 2] {
                let (mut sw, mut sh) = (w.div_ceil(d), h.div_ceil(d));
                if sw < need_w || sh < need_h || t.GetClosestSize(&mut sw, &mut sh).is_err() || sw < need_w || sh < need_h || sw >= w {
                    continue;
                }
                let mut format = GUID_WICPixelFormat32bppBGR;
                if t.GetClosestPixelFormat(&mut format).is_err() {
                    break;
                }
                // At 1/4 and 1/8 size a progressive JPEG needs only its first scans (DC and low AC);
                // the rest changes the thumbnail by well under one level.
                if d >= 4 {
                    if let Ok(p) = frame.cast::<IWICProgressiveLevelControl>() {
                        if let Ok(n @ 2..) = p.GetLevelCount() {
                            let _ = p.SetCurrentLevel((n - 1).min(3));
                        }
                    }
                }
                let Some(bytes) = bytes_per_pixel(&format) else { break };
                let stride = (sw * bytes + 3) & !3;
                let mut buf = vec![0u8; stride as usize * sh as usize];
                if t.CopyPixels(std::ptr::null(), sw, sh, &format, WICBitmapTransformRotate0, stride, &mut buf).is_err() {
                    break;
                }
                return Ok(f.CreateBitmapFromMemory(sw, sh, &format, stride, &buf)?.cast()?);
            }
        }
        Ok(frame.cast()?)
    }
}

fn bytes_per_pixel(format: &GUID) -> Option<u32> {
    match *format {
        f if f == GUID_WICPixelFormat24bppBGR => Some(3),
        f if f == GUID_WICPixelFormat8bppGray => Some(1),
        f if f == GUID_WICPixelFormat32bppBGR
            || f == GUID_WICPixelFormat32bppBGRA
            || f == GUID_WICPixelFormat32bppPBGRA
            || f == GUID_WICPixelFormat32bppCMYK =>
        {
            Some(4)
        }
        _ => None,
    }
}

/// Converts to premultiplied BGRA, optionally cuts `crop`, and scales to `w` x `h`.
unsafe fn resample(f: &IWICImagingFactory, source: &IWICBitmapSource, crop: Option<&WICRect>, w: u32, h: u32) -> Result<Vec<u8>> {
    unsafe {
        let converter = f.CreateFormatConverter()?;
        converter.Initialize(source, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
        let mut current: IWICBitmapSource = converter.cast()?;
        if let Some(rect) = crop {
            let clipper = f.CreateBitmapClipper()?;
            clipper.Initialize(&current, rect)?;
            current = clipper.cast()?;
        }
        let (mut cw, mut ch) = (0, 0);
        current.GetSize(&mut cw, &mut ch)?;
        if (cw, ch) != (w, h) {
            // A big reduction (large PNGs, which cannot be decoded small) goes through a
            // cheap Fant pass to twice the size first; the cubic pass then sets the quality.
            if cw > w * 4 && ch > h * 4 {
                let scaler = f.CreateBitmapScaler()?;
                scaler.Initialize(&current, w * 2, h * 2, WICBitmapInterpolationModeFant)?;
                current = scaler.cast()?;
            }
            let scaler = f.CreateBitmapScaler()?;
            scaler.Initialize(&current, w, h, WICBitmapInterpolationModeHighQualityCubic)?;
            current = scaler.cast()?;
        }
        let mut px = vec![0u8; w as usize * h as usize * 4];
        current.CopyPixels(std::ptr::null(), w * 4, &mut px)?;
        Ok(px)
    }
}

/// Fits `w` x `h` into a `cx` square, never enlarging.
fn fit(w: u32, h: u32, cx: u32) -> (u32, u32) {
    if w <= cx && h <= cx {
        return (w, h);
    }
    let scale = |a: u32, b: u32| ((a as u64 * cx as u64 + b as u64 / 2) / b as u64).max(1) as u32;
    if w >= h { (cx, scale(h, w)) } else { (scale(w, h), cx) }
}

/// The two places the front cover can be: the halves of a spread, or the cover-sized areas
/// on each side of the spine of a jacket (flaps on the outside are about the same width).
fn candidates(w: u32, h: u32, jacket: bool) -> (WICRect, WICRect) {
    let (w, h) = (w as i32, h as i32);
    let mid = w / 2;
    if !jacket {
        return (WICRect { X: 0, Y: 0, Width: mid, Height: h }, WICRect { X: mid, Y: 0, Width: w - mid, Height: h });
    }
    let cover = ((h as f64 * 0.70) as i32).clamp(1, mid);
    let spine = (h as f64 * 0.02) as i32;
    let lx = (mid - spine - cover).max(0);
    let rx = (mid + spine).min(w - 1);
    (
        WICRect { X: lx, Y: 0, Width: (mid - spine - lx).max(1), Height: h },
        WICRect { X: rx, Y: 0, Width: cover.min(w - rx).max(1), Height: h },
    )
}

/// How much an area looks like a front cover: colorfulness (spread of the opponent color
/// channels) plus a little for detail. Back covers are plainer, or text on a flat color.
fn score(px: &[u8], stride: u32, r: &WICRect) -> f64 {
    let (mut n, mut rg, mut rg2, mut yb, mut yb2, mut detail) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for y in r.Y..r.Y + r.Height {
        let row = y as usize * stride as usize;
        let mut prev = None;
        for x in r.X..r.X + r.Width {
            let i = (row + x as usize) * 4;
            let (b, g, red) = (px[i] as f64, px[i + 1] as f64, px[i + 2] as f64);
            let (a, c) = (red - g, 0.5 * (red + g) - b);
            rg += a;
            rg2 += a * a;
            yb += c;
            yb2 += c * c;
            n += 1.0;
            let l = 0.299 * red + 0.587 * g + 0.114 * b;
            if let Some(p) = prev {
                detail += f64::abs(l - p);
            }
            prev = Some(l);
        }
    }
    if n == 0.0 {
        return 0.0;
    }
    let var = |s: f64, s2: f64| (s2 / n - (s / n) * (s / n)).max(0.0);
    (var(rg, rg2) + var(yb, yb2)).sqrt() + 0.5 * detail / n
}

/// Puts transparent areas on white and makes the pixels opaque.
fn finish(width: u32, height: u32, mut bgra: Vec<u8>, note: String) -> Thumb {
    for p in bgra.chunks_exact_mut(4) {
        let a = p[3];
        if a != 255 {
            let k = 255 - a;
            p[0] = p[0].saturating_add(k);
            p[1] = p[1].saturating_add(k);
            p[2] = p[2].saturating_add(k);
            p[3] = 255;
        }
    }
    Thumb { width, height, bgra, note }
}

pub fn to_hbitmap(t: &Thumb) -> Result<HBITMAP> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: t.width as i32,
            biHeight: -(t.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    unsafe {
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        if bits.is_null() {
            let _ = DeleteObject(bitmap.into());
            return Err(Error::Format("bitmap: no memory"));
        }
        std::ptr::copy_nonoverlapping(t.bgra.as_ptr(), bits.cast::<u8>(), t.bgra.len());
        Ok(bitmap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_aspect() {
        assert_eq!(fit(1000, 1500, 256), (171, 256));
        assert_eq!(fit(1500, 1000, 256), (256, 171));
        assert_eq!(fit(100, 150, 256), (100, 150));
    }

    #[test]
    fn jacket_candidates_touch_the_spine() {
        let (l, r) = candidates(2200, 1000, true);
        assert_eq!((l.X + l.Width, r.X), (1080, 1120));
        assert_eq!((l.Width, r.Width), (700, 700));
    }

    #[test]
    fn colorful_side_wins() {
        let (w, h) = (40u32, 10u32);
        let mut px = vec![200u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w / 2 {
                let i = ((y * w + x) * 4) as usize;
                px[i] = (x * 37 % 255) as u8;
                px[i + 1] = (y * 91 % 255) as u8;
                px[i + 2] = ((x + y) * 53 % 255) as u8;
            }
        }
        let (l, r) = candidates(w, h, false);
        assert!(score(&px, w, &l) > score(&px, w, &r) * SIDE_RATIO);
    }
}
