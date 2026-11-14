// SPDX-License-Identifier: GPL-3.0-only
//! App icons for the "App through mic" picker: the icon Windows shows for an app's
//! .exe, read from the file (read-only) and turned into an image the window can draw.
//!
//! Like `win.rs`, the few Win32 calls are declared directly instead of adding a crate.
//! The pixels come out as 32-bit BGRA; they are wrapped in a small BMP header (no
//! encoder needed) that GPUI's image loader understands, alpha included.
//!
//! Python analogy: `app_icon(path)` is like `PIL.Image` from `win32gui.ExtractIconEx`,
//! saved to an in-memory BMP.

use std::ffi::c_void;
use std::sync::Arc;

use gpui::{Image, ImageFormat};

type Hicon = *mut c_void;
type Hgdi = *mut c_void;

#[repr(C)]
struct IconInfo {
    is_icon: i32,
    x_hotspot: u32,
    y_hotspot: u32,
    mask: Hgdi,
    color: Hgdi,
}

#[repr(C)]
struct Bitmap {
    kind: i32,
    width: i32,
    height: i32,
    width_bytes: i32,
    planes: u16,
    bits_pixel: u16,
    bits: *mut c_void,
}

#[repr(C)]
struct BitmapInfo {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    clr_used: u32,
    clr_important: u32,
    /// Room for a colour table, which 32-bit DIBs don't use.
    colors: [u32; 4],
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ExtractIconExW(
        file: *const u16,
        index: i32,
        large: *mut Hicon,
        small: *mut Hicon,
        n: u32,
    ) -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetIconInfo(icon: Hicon, info: *mut IconInfo) -> i32;
    fn DestroyIcon(icon: Hicon) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: *mut c_void) -> *mut c_void;
    fn DeleteDC(dc: *mut c_void) -> i32;
    fn DeleteObject(obj: Hgdi) -> i32;
    fn GetObjectW(obj: Hgdi, size: i32, out: *mut c_void) -> i32;
    fn GetDIBits(
        dc: *mut c_void,
        bitmap: Hgdi,
        start: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut BitmapInfo,
        usage: u32,
    ) -> i32;
}

/// Largest icon side we accept (Windows' "large" icon is 32–64 px depending on DPI).
const MAX_SIDE: i32 = 256;

/// The icon of the program at `path` (a full .exe path), or `None` if it has none or
/// it can't be read.
pub fn app_icon(path: &str) -> Option<Arc<Image>> {
    let (w, h, bgra) = icon_pixels(path)?;
    Some(Arc::new(Image::from_bytes(
        ImageFormat::Bmp,
        bmp(w, h, &bgra),
    )))
}

/// Top-down BGRA pixels of the exe's first large icon.
fn icon_pixels(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    // SAFETY: plain Win32 calls; every handle obtained here is released below, and the
    // buffers outlive the calls that write into them.
    unsafe {
        let mut icon: Hicon = std::ptr::null_mut();
        if ExtractIconExW(wide.as_ptr(), 0, &mut icon, std::ptr::null_mut(), 1) == 0
            || icon.is_null()
        {
            return None;
        }
        let mut info = IconInfo {
            is_icon: 0,
            x_hotspot: 0,
            y_hotspot: 0,
            mask: std::ptr::null_mut(),
            color: std::ptr::null_mut(),
        };
        let got = GetIconInfo(icon, &mut info) != 0;
        DestroyIcon(icon);
        if !got {
            return None;
        }
        let result = color_bits(info.color);
        if !info.color.is_null() {
            DeleteObject(info.color);
        }
        if !info.mask.is_null() {
            DeleteObject(info.mask);
        }
        result
    }
}

/// # Safety
/// `color` must be a valid bitmap handle (or null).
unsafe fn color_bits(color: Hgdi) -> Option<(u32, u32, Vec<u8>)> {
    if color.is_null() {
        return None; // monochrome icon: skip it, the letter badge stands in
    }
    // SAFETY: see the caller; the DC is deleted on every path.
    unsafe {
        let mut bm: Bitmap = std::mem::zeroed();
        if GetObjectW(color, size_of::<Bitmap>() as i32, (&raw mut bm).cast()) == 0 {
            return None;
        }
        let (w, h) = (bm.width, bm.height.abs());
        if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
            return None;
        }
        let mut bi = BitmapInfo {
            size: 40,
            width: w,
            height: -h, // top-down rows
            planes: 1,
            bit_count: 32,
            compression: 0, // BI_RGB
            size_image: 0,
            x_ppm: 0,
            y_ppm: 0,
            clr_used: 0,
            clr_important: 0,
            colors: [0; 4],
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return None;
        }
        let lines = GetDIBits(dc, color, 0, h as u32, px.as_mut_ptr().cast(), &mut bi, 0);
        DeleteDC(dc);
        if lines != h {
            return None;
        }
        // Old icons carry no alpha at all: show them opaque rather than invisible.
        if px.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
            px.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|p| p[3] = 255);
        }
        Some((w as u32, h as u32, px))
    }
}

/// A 32-bit BMP with an alpha channel (BITMAPV4HEADER + BI_BITFIELDS) from top-down
/// BGRA rows.
fn bmp(w: u32, h: u32, bgra: &[u8]) -> Vec<u8> {
    const HEADERS: u32 = 14 + 108;
    let size = HEADERS + bgra.len() as u32;
    let mut b = Vec::with_capacity(size as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&size.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&HEADERS.to_le_bytes());
    // BITMAPV4HEADER
    b.extend_from_slice(&108u32.to_le_bytes());
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes()); // bottom-up rows
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(&3u32.to_le_bytes()); // BI_BITFIELDS
    b.extend_from_slice(&(bgra.len() as u32).to_le_bytes());
    b.extend_from_slice(&2835i32.to_le_bytes());
    b.extend_from_slice(&2835i32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    for mask in [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000] {
        b.extend_from_slice(&mask.to_le_bytes());
    }
    b.extend_from_slice(&0x7352_4742u32.to_le_bytes()); // 'sRGB'
    b.extend_from_slice(&[0u8; 36 + 12]); // endpoints + gamma (unused for sRGB)
    for row in bgra.chunks_exact(w as usize * 4).rev() {
        b.extend_from_slice(row);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_header_and_rows_are_laid_out_bottom_up() {
        // 1×2: top pixel red, bottom pixel blue (BGRA).
        let px = [0, 0, 255, 255, 255, 0, 0, 128];
        let b = bmp(1, 2, &px);
        assert_eq!(&b[..2], b"BM");
        assert_eq!(b.len(), 14 + 108 + 8);
        assert_eq!(u32::from_le_bytes(b[10..14].try_into().unwrap()), 122);
        // The first stored row is the bottom one.
        assert_eq!(&b[122..126], &[255, 0, 0, 128]);
        assert_eq!(&b[126..130], &[0, 0, 255, 255]);
    }
}
