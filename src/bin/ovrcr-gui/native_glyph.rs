//! CoreText fallback for glyphs absent from egui's configured outline fonts.
//! In particular, Apple's emoji artwork is bitmap-only; loading its font into
//! egui cannot render it. Keep the normal terminal font and geometry unchanged.
use eframe::egui::{self, Color32};
use std::{collections::HashMap, ffi::c_void, ptr};

type Ref = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFStringCreateWithBytes(
        allocator: Ref,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: bool,
    ) -> Ref;
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        len: isize,
        key_callbacks: Ref,
        value_callbacks: Ref,
    ) -> Ref;
    fn CFAttributedStringCreate(allocator: Ref, text: Ref, attributes: Ref) -> Ref;
    static kCFBooleanTrue: Ref;
}
#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    fn CTFontCreateWithName(name: Ref, size: f64, matrix: Ref) -> Ref;
    fn CTLineCreateWithAttributedString(text: Ref) -> Ref;
    fn CTLineGetTypographicBounds(
        line: Ref,
        ascent: *mut f64,
        descent: *mut f64,
        leading: *mut f64,
    ) -> f64;
    fn CTLineDraw(line: Ref, context: Ref);
    static kCTFontAttributeName: Ref;
    static kCTForegroundColorFromContextAttributeName: Ref;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> Ref;
    fn CGColorSpaceRelease(space: Ref);
    fn CGBitmapContextCreate(
        data: *mut u8,
        width: usize,
        height: usize,
        bits: usize,
        stride: usize,
        space: Ref,
        info: u32,
    ) -> Ref;
    fn CGContextRelease(context: Ref);
    fn CGContextScaleCTM(context: Ref, x: f64, y: f64);
    fn CGContextSetTextPosition(context: Ref, x: f64, y: f64);
    fn CGContextSetAlpha(context: Ref, alpha: f64);
    fn CGContextSetRGBFillColor(context: Ref, r: f64, g: f64, b: f64, a: f64);
}

struct Owned(Ref, unsafe extern "C" fn(Ref));
impl Owned {
    fn new(value: Ref, release: unsafe extern "C" fn(Ref)) -> Option<Self> {
        if value.is_null() {
            None
        } else {
            Some(Self(value, release))
        }
    }
    fn string(text: &str) -> Option<Self> {
        // CoreFoundation copies these UTF-8 bytes during the call.
        Self::new(
            unsafe {
                CFStringCreateWithBytes(
                    ptr::null(),
                    text.as_ptr(),
                    text.len() as isize,
                    0x0800_0100,
                    false,
                )
            },
            CFRelease,
        )
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // Each value comes from a matching Create function and is released once.
        unsafe { (self.1)(self.0) }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    text: String,
    size: u32,
    scale: u32,
    color: Color32,
    bold: bool,
    italic: bool,
}

#[derive(Clone)]
struct Glyph {
    texture: egui::TextureHandle,
    size: egui::Vec2,
}

/// A cached rasterization result plus the frame it was last painted in, so
/// stale entries (and their textures) can be evicted without a hard cap on
/// distinct glyphs. `glyph` is `None` when rasterization legitimately
/// produced nothing (see `rasterize`'s size guards), which we still cache to
/// avoid retrying CoreText every frame for the same key.
#[derive(Clone)]
struct CacheEntry {
    glyph: Option<Glyph>,
    last_used: u64,
}

// Above this many entries, evict everything not painted in the previous
// frame. A full 40x120 screen of distinct CJK/emoji glyphs is on the order
// of a few thousand cells, so this only trims genuinely stale entries.
const MAX_CACHE_ENTRIES: usize = 4096;

#[derive(Clone)]
pub(super) struct NativeGlyphs {
    cache: HashMap<Key, CacheEntry>,
    frame: u64,
    max_entries: usize,
    #[cfg(test)]
    pub(super) rasterize_calls: usize,
}
impl Default for NativeGlyphs {
    fn default() -> Self {
        Self {
            cache: HashMap::new(),
            frame: 0,
            max_entries: MAX_CACHE_ENTRIES,
            #[cfg(test)]
            rasterize_calls: 0,
        }
    }
}
impl NativeGlyphs {
    /// Test-only constructor overriding the eviction threshold, so tests can
    /// exercise the sweep without painting thousands of real glyphs.
    #[cfg(test)]
    pub(super) fn with_max_entries(max_entries: usize) -> Self {
        Self {
            max_entries,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.cache.len()
    }

    /// Marks the start of a new whole-terminal paint. Must be called exactly
    /// once per frame (not once per cell) so `last_used` stamps distinguish
    /// "painted this frame" from "painted a previous frame".
    pub(super) fn begin_frame(&mut self) {
        if self.cache.len() > self.max_entries {
            let previous_frame = self.frame;
            self.cache
                .retain(|_, entry| entry.last_used >= previous_frame);
        }
        self.frame += 1;
    }

    pub(super) fn paint(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        font: &egui::FontId,
        cell: &vt100::Cell,
        color: Color32,
    ) -> bool {
        let scale = ui.ctx().pixels_per_point();
        let key = Key {
            text: cell.contents().to_owned(),
            size: font.size.to_bits(),
            scale: scale.to_bits(),
            color,
            bold: cell.bold(),
            italic: cell.italic(),
        };
        let frame = self.frame;
        #[cfg(test)]
        if !self.cache.contains_key(&key) {
            self.rasterize_calls += 1;
        }
        let entry = self.cache.entry(key.clone()).or_insert_with(|| {
            let glyph = rasterize(&key).map(|image| {
                let size = egui::vec2(image.size[0] as f32, image.size[1] as f32) / scale;
                Glyph {
                    texture: ui.ctx().load_texture(
                        "native terminal glyph",
                        image,
                        egui::TextureOptions::LINEAR,
                    ),
                    size,
                }
            });
            CacheEntry {
                glyph,
                last_used: frame,
            }
        });
        // Stamp on every use, not only on insertion, so a live entry survives
        // `begin_frame`'s sweep for as long as it keeps getting painted.
        entry.last_used = frame;
        let Some(glyph) = &entry.glyph else {
            return false;
        };
        // Preserve native glyph dimensions. Clip instead of stretching to the cell.
        let origin = rect.min + egui::vec2(0.0, (rect.height() - glyph.size.y) * 0.5);
        ui.painter().with_clip_rect(rect).image(
            glyph.texture.id(),
            egui::Rect::from_min_size(origin, glyph.size),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        true
    }
}

#[test]
fn native_raster_retains_style_scale_and_opacity() {
    for text in ["界", "🙂"] {
        let key = Key {
            text: text.into(),
            size: 11.0f32.to_bits(),
            scale: 1.0f32.to_bits(),
            color: Color32::RED,
            bold: false,
            italic: false,
        };
        let regular = rasterize(&key).unwrap();
        let retina = rasterize(&Key {
            scale: 2.0f32.to_bits(),
            ..key.clone()
        })
        .unwrap();
        for (normal, doubled) in regular.size.into_iter().zip(retina.size) {
            assert!(
                doubled.abs_diff(normal * 2) <= 1,
                "DPI must change raster resolution, not point size"
            );
        }
        if text == "界" {
            let styled = rasterize(&Key {
                bold: true,
                italic: true,
                ..key.clone()
            })
            .unwrap();
            assert_ne!(
                regular.pixels, styled.pixels,
                "native fallback must carry font traits"
            );
        }
        let dim = rasterize(&Key {
            color: Color32::RED.gamma_multiply(0.5),
            ..key
        })
        .unwrap();
        let alpha = dim.pixels.iter().map(|pixel| pixel.a()).max().unwrap();
        assert!(
            alpha > 0 && alpha <= 128,
            "dim {text} lost its opacity: {alpha}"
        );
    }
    assert!(Owned::new(ptr::null(), CFRelease).is_none());
}

fn rasterize(key: &Key) -> Option<egui::ColorImage> {
    let text = Owned::string(&key.text)?;
    let name = Owned::string(match (key.bold, key.italic) {
        (false, false) => "Menlo-Regular",
        (true, false) => "Menlo-Bold",
        (false, true) => "Menlo-Italic",
        (true, true) => "Menlo-BoldItalic",
    })?;
    let scale = f32::from_bits(key.scale) as f64;
    // All borrowed CF dictionary values remain alive until after drawing. CoreText
    // supplies the platform fallback font and bitmap/color emoji rendering.
    unsafe {
        let font = Owned::new(
            CTFontCreateWithName(name.0, f32::from_bits(key.size) as f64, ptr::null()),
            CFRelease,
        )?;
        let keys = [
            kCTFontAttributeName,
            kCTForegroundColorFromContextAttributeName,
        ];
        let values = [font.0, kCFBooleanTrue];
        let attrs = Owned::new(
            CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                2,
                ptr::null(),
                ptr::null(),
            ),
            CFRelease,
        )?;
        let attributed = Owned::new(
            CFAttributedStringCreate(ptr::null(), text.0, attrs.0),
            CFRelease,
        )?;
        let line = Owned::new(CTLineCreateWithAttributedString(attributed.0), CFRelease)?;
        let (mut ascent, mut descent) = (0.0, 0.0);
        let advance =
            CTLineGetTypographicBounds(line.0, &mut ascent, &mut descent, ptr::null_mut());
        let width = (advance * scale).ceil() as usize;
        let height = ((ascent + descent) * scale).ceil() as usize;
        if width == 0 || height == 0 || width > 1024 || height > 1024 {
            return None;
        }
        let mut pixels = vec![0u8; width * height * 4];
        let space = Owned::new(CGColorSpaceCreateDeviceRGB(), CGColorSpaceRelease)?;
        // Big-endian RGBA with premultiplied alpha; the buffer outlives the context.
        let context = Owned::new(
            CGBitmapContextCreate(
                pixels.as_mut_ptr(),
                width,
                height,
                8,
                width * 4,
                space.0,
                0x4001,
            ),
            CGContextRelease,
        )?;
        CGContextScaleCTM(context.0, scale, scale);
        CGContextSetTextPosition(context.0, 0.0, descent);
        let [r, g, b, a] = key.color.to_srgba_unmultiplied();
        // Color emoji ignores the foreground fill, but obeys context opacity.
        CGContextSetAlpha(context.0, a as f64 / 255.0);
        CGContextSetRGBFillColor(
            context.0,
            r as f64 / 255.0,
            g as f64 / 255.0,
            b as f64 / 255.0,
            1.0,
        );
        CTLineDraw(line.0, context.0);
        drop(context);
        Some(egui::ColorImage::from_rgba_premultiplied(
            [width, height],
            &pixels,
        ))
    }
}
