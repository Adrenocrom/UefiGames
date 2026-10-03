//! Software back buffer drawing primitives and an 8x8 bitmap font.
//!
//! Everything is drawn into a buffer of `BltPixel`s that is blitted to the
//! GOP framebuffer once per frame. This avoids tearing and works with every
//! GOP implementation (including `BltOnly` modes).

use crate::math;
use uefi::proto::console::gop::BltPixel;

/// 8x8 monochrome bitmap font for ASCII 0x20..=0x7E.
///
/// Public domain. Source: dhepper/font8x8 (`font8x8_basic.h`), itself based on
/// IBM public-domain VGA fonts. Each byte is one row; bit 0 is the leftmost
/// pixel (matches the upstream rendering example).
const FONT: [[u8; 8]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // U+0020 (space)
    [0x18, 0x3C, 0x3C, 0x18, 0x18, 0x00, 0x18, 0x00], // U+0021 (!)
    [0x36, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // U+0022 (")
    [0x36, 0x36, 0x7F, 0x36, 0x7F, 0x36, 0x36, 0x00], // U+0023 (#)
    [0x0C, 0x3E, 0x03, 0x1E, 0x30, 0x1F, 0x0C, 0x00], // U+0024 ($)
    [0x00, 0x63, 0x33, 0x18, 0x0C, 0x66, 0x63, 0x00], // U+0025 (%)
    [0x1C, 0x36, 0x1C, 0x6E, 0x3B, 0x33, 0x6E, 0x00], // U+0026 (&)
    [0x06, 0x06, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00], // U+0027 (')
    [0x18, 0x0C, 0x06, 0x06, 0x06, 0x0C, 0x18, 0x00], // U+0028 (()
    [0x06, 0x0C, 0x18, 0x18, 0x18, 0x0C, 0x06, 0x00], // U+0029 ())
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00], // U+002A (*)
    [0x00, 0x0C, 0x0C, 0x3F, 0x0C, 0x0C, 0x00, 0x00], // U+002B (+)
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x06], // U+002C (,)
    [0x00, 0x00, 0x00, 0x3F, 0x00, 0x00, 0x00, 0x00], // U+002D (-)
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x00], // U+002E (.)
    [0x60, 0x30, 0x18, 0x0C, 0x06, 0x03, 0x01, 0x00], // U+002F (/)
    [0x3E, 0x63, 0x73, 0x7B, 0x6F, 0x67, 0x3E, 0x00], // U+0030 (0)
    [0x0C, 0x0E, 0x0C, 0x0C, 0x0C, 0x0C, 0x3F, 0x00], // U+0031 (1)
    [0x1E, 0x33, 0x30, 0x1C, 0x06, 0x33, 0x3F, 0x00], // U+0032 (2)
    [0x1E, 0x33, 0x30, 0x1C, 0x30, 0x33, 0x1E, 0x00], // U+0033 (3)
    [0x38, 0x3C, 0x36, 0x33, 0x7F, 0x30, 0x78, 0x00], // U+0034 (4)
    [0x3F, 0x03, 0x1F, 0x30, 0x30, 0x33, 0x1E, 0x00], // U+0035 (5)
    [0x1C, 0x06, 0x03, 0x1F, 0x33, 0x33, 0x1E, 0x00], // U+0036 (6)
    [0x3F, 0x33, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x00], // U+0037 (7)
    [0x1E, 0x33, 0x33, 0x1E, 0x33, 0x33, 0x1E, 0x00], // U+0038 (8)
    [0x1E, 0x33, 0x33, 0x3E, 0x30, 0x18, 0x0E, 0x00], // U+0039 (9)
    [0x00, 0x0C, 0x0C, 0x00, 0x00, 0x0C, 0x0C, 0x00], // U+003A (:)
    [0x00, 0x0C, 0x0C, 0x00, 0x00, 0x0C, 0x0C, 0x06], // U+003B (;)
    [0x18, 0x0C, 0x06, 0x03, 0x06, 0x0C, 0x18, 0x00], // U+003C (<)
    [0x00, 0x00, 0x3F, 0x00, 0x00, 0x3F, 0x00, 0x00], // U+003D (=)
    [0x06, 0x0C, 0x18, 0x30, 0x18, 0x0C, 0x06, 0x00], // U+003E (>)
    [0x1E, 0x33, 0x30, 0x18, 0x0C, 0x00, 0x0C, 0x00], // U+003F (?)
    [0x3E, 0x63, 0x7B, 0x7B, 0x7B, 0x03, 0x1E, 0x00], // U+0040 (@)
    [0x0C, 0x1E, 0x33, 0x33, 0x3F, 0x33, 0x33, 0x00], // U+0041 (A)
    [0x3F, 0x66, 0x66, 0x3E, 0x66, 0x66, 0x3F, 0x00], // U+0042 (B)
    [0x3C, 0x66, 0x03, 0x03, 0x03, 0x66, 0x3C, 0x00], // U+0043 (C)
    [0x1F, 0x36, 0x66, 0x66, 0x66, 0x36, 0x1F, 0x00], // U+0044 (D)
    [0x7F, 0x46, 0x16, 0x1E, 0x16, 0x46, 0x7F, 0x00], // U+0045 (E)
    [0x7F, 0x46, 0x16, 0x1E, 0x16, 0x06, 0x0F, 0x00], // U+0046 (F)
    [0x3C, 0x66, 0x03, 0x03, 0x73, 0x66, 0x7C, 0x00], // U+0047 (G)
    [0x33, 0x33, 0x33, 0x3F, 0x33, 0x33, 0x33, 0x00], // U+0048 (H)
    [0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // U+0049 (I)
    [0x78, 0x30, 0x30, 0x30, 0x33, 0x33, 0x1E, 0x00], // U+004A (J)
    [0x67, 0x66, 0x36, 0x1E, 0x36, 0x66, 0x67, 0x00], // U+004B (K)
    [0x0F, 0x06, 0x06, 0x06, 0x46, 0x66, 0x7F, 0x00], // U+004C (L)
    [0x63, 0x77, 0x7F, 0x7F, 0x6B, 0x63, 0x63, 0x00], // U+004D (M)
    [0x63, 0x67, 0x6F, 0x7B, 0x73, 0x63, 0x63, 0x00], // U+004E (N)
    [0x1C, 0x36, 0x63, 0x63, 0x63, 0x36, 0x1C, 0x00], // U+004F (O)
    [0x3F, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x0F, 0x00], // U+0050 (P)
    [0x1E, 0x33, 0x33, 0x3B, 0x1E, 0x38, 0x00, 0x00], // U+0051 (Q)
    [0x3F, 0x66, 0x66, 0x3E, 0x36, 0x66, 0x67, 0x00], // U+0052 (R)
    [0x1E, 0x33, 0x07, 0x0E, 0x38, 0x33, 0x1E, 0x00], // U+0053 (S)
    [0x3F, 0x2D, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // U+0054 (T)
    [0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x3F, 0x00], // U+0055 (U)
    [0x33, 0x33, 0x33, 0x33, 0x33, 0x1E, 0x0C, 0x00], // U+0056 (V)
    [0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00], // U+0057 (W)
    [0x63, 0x63, 0x36, 0x1C, 0x1C, 0x36, 0x63, 0x00], // U+0058 (X)
    [0x33, 0x33, 0x33, 0x1E, 0x0C, 0x0C, 0x1E, 0x00], // U+0059 (Y)
    [0x7F, 0x63, 0x31, 0x18, 0x4C, 0x66, 0x7F, 0x00], // U+005A (Z)
    [0x1E, 0x06, 0x06, 0x06, 0x06, 0x06, 0x1E, 0x00], // U+005B ([)
    [0x03, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x40, 0x00], // U+005C (\)
    [0x1E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1E, 0x00], // U+005D (])
    [0x08, 0x1C, 0x36, 0x63, 0x00, 0x00, 0x00, 0x00], // U+005E (^)
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF], // U+005F (_)
    [0x0C, 0x0C, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00], // U+0060 (`)
    [0x00, 0x00, 0x1E, 0x30, 0x3E, 0x33, 0x6E, 0x00], // U+0061 (a)
    [0x07, 0x06, 0x06, 0x3E, 0x66, 0x66, 0x3B, 0x00], // U+0062 (b)
    [0x00, 0x00, 0x1E, 0x33, 0x03, 0x33, 0x1E, 0x00], // U+0063 (c)
    [0x38, 0x30, 0x30, 0x3E, 0x33, 0x33, 0x6E, 0x00], // U+0064 (d)
    [0x00, 0x00, 0x1E, 0x33, 0x3F, 0x03, 0x1E, 0x00], // U+0065 (e)
    [0x1C, 0x36, 0x06, 0x0F, 0x06, 0x06, 0x0F, 0x00], // U+0066 (f)
    [0x00, 0x00, 0x6E, 0x33, 0x33, 0x3E, 0x30, 0x1F], // U+0067 (g)
    [0x07, 0x06, 0x36, 0x6E, 0x66, 0x66, 0x67, 0x00], // U+0068 (h)
    [0x0C, 0x00, 0x0E, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // U+0069 (i)
    [0x30, 0x00, 0x30, 0x30, 0x30, 0x33, 0x33, 0x1E], // U+006A (j)
    [0x07, 0x06, 0x66, 0x36, 0x1E, 0x36, 0x67, 0x00], // U+006B (k)
    [0x0E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // U+006C (l)
    [0x00, 0x00, 0x33, 0x7F, 0x7F, 0x6B, 0x63, 0x00], // U+006D (m)
    [0x00, 0x00, 0x1F, 0x33, 0x33, 0x33, 0x33, 0x00], // U+006E (n)
    [0x00, 0x00, 0x1E, 0x33, 0x33, 0x33, 0x1E, 0x00], // U+006F (o)
    [0x00, 0x00, 0x3B, 0x66, 0x66, 0x3E, 0x06, 0x0F], // U+0070 (p)
    [0x00, 0x00, 0x6E, 0x33, 0x33, 0x3E, 0x30, 0x78], // U+0071 (q)
    [0x00, 0x00, 0x3B, 0x6E, 0x66, 0x06, 0x0F, 0x00], // U+0072 (r)
    [0x00, 0x00, 0x3E, 0x03, 0x1E, 0x30, 0x1F, 0x00], // U+0073 (s)
    [0x08, 0x0C, 0x3E, 0x0C, 0x0C, 0x2C, 0x18, 0x00], // U+0074 (t)
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x33, 0x6E, 0x00], // U+0075 (u)
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x1E, 0x0C, 0x00], // U+0076 (v)
    [0x00, 0x00, 0x63, 0x6B, 0x7F, 0x7F, 0x36, 0x00], // U+0077 (w)
    [0x00, 0x00, 0x63, 0x36, 0x1C, 0x36, 0x63, 0x00], // U+0078 (x)
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x3E, 0x30, 0x1F], // U+0079 (y)
    [0x00, 0x00, 0x3F, 0x19, 0x0C, 0x26, 0x3F, 0x00], // U+007A (z)
    [0x38, 0x0C, 0x0C, 0x07, 0x0C, 0x0C, 0x38, 0x00], // U+007B ({)
    [0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x18, 0x00], // U+007C (|)
    [0x07, 0x0C, 0x0C, 0x38, 0x0C, 0x0C, 0x07, 0x00], // U+007D (})
    [0x6E, 0x3B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // U+007E (~)
];

/// A 24-bit RGB color.
#[derive(Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b }
    }

    fn to_blt(self) -> BltPixel {
        BltPixel::new(self.r, self.g, self.b)
    }
}

/// Number of decimal digits of `n` (at least 1).
fn digit_count(n: u32) -> u32 {
    let mut n = n;
    let mut d = 1;
    while n >= 10 {
        n /= 10;
        d += 1;
    }
    d
}

/// Pixel width of `prefix` followed by the decimal representation of `n`.
pub fn num_width(prefix: &str, n: u32) -> u32 {
    (prefix.len() as u32 + digit_count(n)) * 8
}

/// A rectangular bitmap sprite that can be blitted to the screen.
/// Coordinates: `u` right, `v` down, origin at the top-left; `None` marks
/// transparent pixels.
pub trait Bitmap {
    fn dims(&self) -> (u32, u32);
    fn pixel(&self, u: u32, v: u32) -> Option<Color>;
}

pub struct Screen {
    width: u32,
    height: u32,
    pixels: alloc::vec::Vec<BltPixel>,
}

impl Screen {
    pub fn new(width: u32, height: u32) -> Screen {
        Screen {
            width,
            height,
            pixels: alloc::vec![BltPixel::new(0, 0, 0); (width * height) as usize],
        }
    }

    pub fn pixels(&self) -> &[BltPixel] {
        &self.pixels
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Set one pixel; out-of-range coordinates are ignored.
    pub fn px(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height {
            self.pixels[y as usize * self.width as usize + x as usize] = color.to_blt();
        }
    }

    /// Fill a horizontal span from `x0` (inclusive) to `x1` (exclusive) on
    /// row `y`. The span is clipped to the screen; out-of-range rows are
    /// ignored. Used for the per-scanline road rendering.
    pub fn hline(&mut self, x0: f32, x1: f32, y: u32, color: Color) {
        if y >= self.height {
            return;
        }
        let x0 = math::floor(x0).max(0.0) as u32;
        let x1 = math::ceil(x1).min(self.width as f32) as u32;
        if x1 > x0 {
            self.rect(x0, y, x1 - x0, 1, color);
        }
    }

    /// Blit a bitmap sprite with its top-left corner at `(x, y)`.
    pub fn blit(&mut self, bmp: &dyn Bitmap, x: f32, y: f32) {
        let (w, h) = bmp.dims();
        let (x0, y0) = (x as u32, y as u32);
        for v in 0..h {
            for u in 0..w {
                if let Some(color) = bmp.pixel(u, v) {
                    self.px(x0 + u, y0 + v, color);
                }
            }
        }
    }

    /// Blit a bitmap sprite scaled by an integer factor with its top-left
    /// corner at `(x, y)`. Nearest-neighbor sampling keeps the chunky
    /// pixel-art look; the sprite stays axis-aligned.
    pub fn blit_scaled(&mut self, bmp: &dyn Bitmap, x: f32, y: f32, scale: u32) {
        if scale <= 1 {
            self.blit(bmp, x, y);
            return;
        }
        let (w, h) = bmp.dims();
        let (x0, y0) = (x as u32, y as u32);
        for v in 0..h {
            for u in 0..w {
                if let Some(color) = bmp.pixel(u, v) {
                    self.rect(x0 + u * scale, y0 + v * scale, scale, scale, color);
                }
            }
        }
    }

    /// Blit a bitmap sprite rotated around its center and scaled by `scale`.
    ///
    /// `angle` is the rotation in radians (positive = clockwise on screen,
    /// matching the car steering left/right). The sprite is additionally
    /// squashed vertically by `cos(angle)`, which reads as the car tilting
    /// into the turn — the "3D rotation" look from idea.md.
    ///
    /// Implementation: for every destination pixel in the bounding box, the
    /// inverse rotation maps it back into sprite space; pixels that fall
    /// outside the sprite are skipped. This is the standard affine blit and
    /// needs no interpolation, so a scaled sprite keeps its chunky pixel-art
    /// look.
    pub fn blit_rotated(&mut self, bmp: &dyn Bitmap, cx: f32, cy: f32, angle: f32, scale: f32) {
        let (w, h) = bmp.dims();
        let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
        let (sin, cos) = (math::sin(angle), math::cos(angle));
        let squash = cos.abs().max(0.55); // keep the car visible at 45°

        // Bounding box of the scaled, rotated sprite (the squash only
        // shrinks it). `max(hw, hh) * scale` is a safe upper bound for the
        // rotated half-diagonal (`core` has no `sqrt` for f32).
        let rad = hw.max(hh) * scale;
        let x0 = math::floor(cx - rad).max(0.0) as u32;
        let y0 = math::floor(cy - rad * squash).max(0.0) as u32;
        let x1 = math::ceil(cx + rad).min(self.width as f32) as u32;
        let y1 = math::ceil(cy + rad * squash).min(self.height as f32) as u32;

        for yy in y0..y1 {
            for xx in x0..x1 {
                // Destination offset from the sprite center.
                let dx = xx as f32 + 0.5 - cx;
                let dy = (yy as f32 + 0.5 - cy) / squash;
                // Inverse rotation into sprite space, then undo the scale.
                let u = (dx * cos + dy * sin) / scale + hw;
                let v = (-dx * sin + dy * cos) / scale + hh;
                if u >= 0.0 && v >= 0.0 && u < w as f32 && v < h as f32 {
                    if let Some(color) = bmp.pixel(u as u32, v as u32) {
                        self.px(xx, yy, color);
                    }
                }
            }
        }
    }

    pub fn fill(&mut self, color: Color) {
        let px = color.to_blt();
        for p in self.pixels.iter_mut() {
            *p = px;
        }
    }

    /// Fill an axis-aligned rectangle given in integer pixel coordinates.
    /// Coordinates are clipped to the screen, so out-of-range values are safe.
    pub fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        let x0 = x.min(self.width);
        let y0 = y.min(self.height);
        let x1 = x.saturating_add(w).min(self.width);
        let y1 = y.saturating_add(h).min(self.height);
        let px = color.to_blt();
        for yy in y0..y1 {
            let row = yy as usize * self.width as usize;
            self.pixels[row + x0 as usize..row + x1 as usize].fill(px);
        }
    }

    /// Fill an axis-aligned rectangle given in float coordinates (game space).
    pub fn rect_f(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.rect(x as u32, y as u32, w as u32, h as u32, color);
    }

    /// Fill a circle (used for the ball).
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        let r2 = r * r;
        let x0 = math::floor(cx - r).max(0.0) as u32;
        let y0 = math::floor(cy - r).max(0.0) as u32;
        let x1 = math::ceil(cx + r).min(self.width as f32) as u32;
        let y1 = math::ceil(cy + r).min(self.height as f32) as u32;
        let px = color.to_blt();
        for yy in y0..y1 {
            for xx in x0..x1 {
                let dx = xx as f32 + 0.5 - cx;
                let dy = yy as f32 + 0.5 - cy;
                if dx * dx + dy * dy <= r2 {
                    self.pixels[yy as usize * self.width as usize + xx as usize] = px;
                }
            }
        }
    }

    /// Draw a single line of 8x8 text at pixel coordinates.
    pub fn text(&mut self, x: u32, y: u32, s: &str, color: Color) {
        let mut cx = x;
        for ch in s.bytes() {
            self.glyph(cx, y, ch, color);
            cx = cx.saturating_add(8);
        }
    }

    /// Draw 8x8 text centered horizontally on the screen.
    pub fn text_centered(&mut self, y: u32, s: &str, color: Color) {
        let w = (s.len() as u32) * 8;
        self.text(self.width.saturating_sub(w) / 2, y, s, color);
    }

    /// Draw `prefix` followed by `n` in decimal (avoids `format_args!`
    /// lifetime pitfalls in no_std; numbers are formatted into a stack buffer).
    pub fn text_num(&mut self, x: u32, y: u32, prefix: &str, n: u32, color: Color) {
        let mut buf = [0u8; 10]; // u32::MAX has 10 digits
        let mut i = buf.len();
        let mut v = n;
        loop {
            i -= 1;
            buf[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.text(x, y, prefix, color);
        if let Ok(s) = core::str::from_utf8(&buf[i..]) {
            self.text(x + (prefix.len() as u32) * 8, y, s, color);
        }
    }

    fn glyph(&mut self, x: u32, y: u32, ch: u8, color: Color) {
        if !(0x20..=0x7E).contains(&ch) {
            return; // control characters and DEL render as blank
        }
        let glyph = FONT[(ch - 0x20) as usize];
        let px = color.to_blt();
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8u32 {
                if bits & (1 << col) != 0 {
                    let xx = x + col;
                    let yy = y + row as u32;
                    if xx < self.width && yy < self.height {
                        self.pixels[yy as usize * self.width as usize + xx as usize] = px;
                    }
                }
            }
        }
    }
}