//! Rasterizes an SF Symbol into RGBA pixels for the tray icon.
//!
//! tray-icon's `Icon::from_path` is Windows-only, so on macOS the only way
//! to hand it a real icon is raw pixels. Rendering a system symbol through
//! AppKit gets a crisp, native-looking glyph that auto-adapts to light/dark
//! menu bars via template rendering, instead of hand-drawing one.

use anyhow::{Context, Result};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBitmapImageRep, NSDeviceRGBColorSpace, NSFontWeightRegular, NSGraphicsContext, NSImage,
    NSImageSymbolConfiguration, NSImageSymbolScale,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use tray_icon::Icon;

/// Renders `name` (an SF Symbols catalog name, e.g. `"lightbulb.fill"`) as a
/// template icon at `point_size`, rasterized @2x for Retina crispness.
pub fn render_sf_symbol(name: &str, point_size: f64) -> Result<Icon> {
    let ns_name = NSString::from_str(name);
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns_name, None)
        .with_context(|| format!("no such SF Symbol: {name:?}"))?;
    image.setTemplate(true);

    let weight = unsafe { NSFontWeightRegular };
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight_scale(
        point_size,
        weight,
        NSImageSymbolScale::Medium,
    );
    let image = image.imageWithSymbolConfiguration(&config).unwrap_or(image);

    let px = (point_size * 2.0).round() as isize; // @2x for Retina
    let rep = new_bitmap(px, px)?;

    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)
        .context("creating bitmap graphics context")?;
    let previous = NSGraphicsContext::currentContext();
    NSGraphicsContext::setCurrentContext(Some(&context));
    image.drawInRect(NSRect::new(
        NSPoint::ZERO,
        NSSize::new(px as f64, px as f64),
    ));
    context.flushGraphics();
    NSGraphicsContext::setCurrentContext(previous.as_deref());

    let rgba = copy_rgba(&rep, px as usize, px as usize);
    Icon::from_rgba(rgba, px as u32, px as u32).context("building tray icon from rendered pixels")
}

fn new_bitmap(width: isize, height: isize) -> Result<Retained<NSBitmapImageRep>> {
    unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            width,
            height,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }
    .context("allocating offscreen bitmap")
}

fn copy_rgba(rep: &NSBitmapImageRep, width: usize, height: usize) -> Vec<u8> {
    let stride = rep.bytesPerRow() as usize;
    let data = rep.bitmapData();
    let mut out = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let row_ptr = unsafe { data.add(row * stride) };
        let row_bytes = unsafe { std::slice::from_raw_parts(row_ptr, width * 4) };
        out.extend_from_slice(row_bytes);
    }
    out
}
