//! The scrolling track: road geometry, rendering and the meadow around it.
//!
//! The road is a centerline `center_x(d)` over the driven distance `d`. It
//! is built from fixed-length segments whose horizontal drift ("bend") is
//! smoothed with a cosine ease, so consecutive segments join without kinks.
//! Segments live in a small ring buffer that is refilled with random bends
//! as the car advances — an endless road with O(1) lookup per scanline.

use crate::assets;
use crate::draw::{self, Bitmap, Screen};
use crate::math;
use core::f32::consts::PI;

/// Length of one track segment along the road, px.
const SEG_LEN: f32 = 240.0;
/// Ring buffer size. One slot covers `SEG_LEN` px of track; the screen plus
/// spawn lookahead needs ~3 slots, so 8 leaves a wide margin for the
/// look-backs that happen while rendering the rows top to bottom.
const RING: usize = 8;
/// Maximum horizontal drift of a single segment, px. This bounds the road
/// slope at `MAX_BEND * PI / (2 * SEG_LEN)` ≈ 0.65, which the car's lateral
/// speed must outpace at maximum driving speed.
const MAX_BEND: f32 = 100.0;
/// Road margin kept free of the screen edges, px.
const EDGE_MARGIN: f32 = 60.0;

/// Half-width of the asphalt, px.
pub const ROAD_HALF_W: f32 = 200.0;
/// Width of the white stripes flanking the asphalt, px.
pub const EDGE_W: f32 = 6.0;
/// Side length of the grass detail tile, px (see `assets::GRASS`).
const GRASS_TILE: u32 = 16;

pub const COL_MEADOW_A: draw::Color = draw::Color::new(40, 96, 44);
const COL_MEADOW_B: draw::Color = draw::Color::new(46, 106, 50);
const COL_FLOWER: draw::Color = draw::Color::new(240, 230, 130);
const COL_ROAD: draw::Color = draw::Color::new(70, 72, 76);
const COL_EDGE: draw::Color = draw::Color::new(235, 235, 235);
const COL_DASH: draw::Color = draw::Color::new(250, 250, 210);

/// Xorshift32 generator. UEFI offers no entropy source, so callers seed it
/// with whatever varies between runs (e.g. the screen resolution).
pub struct Rng(u32);

impl Rng {
    /// The state must never be zero; `| 1` guarantees that.
    pub fn new(seed: u32) -> Rng {
        Rng(seed | 1)
    }

    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform value in `0..n` (good enough for game randomness).
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    /// Uniform `f32` in `lo..=hi`.
    pub fn in_range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * (self.next() as f32 / u32::MAX as f32)
    }
}

/// One ring slot: the road center x at the segment start, plus the total
/// horizontal drift over the segment.
#[derive(Clone, Copy)]
struct Slot {
    x_start: f32,
    bend: f32,
}

pub struct Track {
    slots: [Slot; RING],
    /// Slot index holding segment `first_seg`.
    head: usize,
    first_seg: i64,
    /// The road center is steered to stay within `min_cx..=max_cx` so it
    /// never leaves the screen.
    min_cx: f32,
    max_cx: f32,
    rng: Rng,
}

impl Track {
    pub fn new(width: f32, seed: u32) -> Track {
        let mut track = Track {
            slots: [Slot { x_start: 0.0, bend: 0.0 }; RING],
            head: 0,
            first_seg: 0,
            min_cx: ROAD_HALF_W + EDGE_MARGIN,
            max_cx: (width - ROAD_HALF_W - EDGE_MARGIN).max(ROAD_HALF_W + EDGE_MARGIN),
            rng: Rng::new(seed),
        };
        // The first two segments are straight so every run starts on an
        // even road; the rest of the ring is filled with random bends.
        track.slots[0] = Slot { x_start: width / 2.0, bend: 0.0 };
        let mut end_x = width / 2.0;
        for i in 1..RING {
            let bend = if i < 2 { 0.0 } else { track.gen_bend(end_x) };
            track.slots[i] = Slot { x_start: end_x, bend };
            end_x += bend;
        }
        track
    }

    /// Road center x at driven distance `d` (px, 0 at the start).
    pub fn center_x(&mut self, d: f32) -> f32 {
        let seg = math::floor(d / SEG_LEN) as i64;
        self.ensure(seg);
        let slot = &self.slots[self.slot_of(seg)];
        let t = (d - seg as f32 * SEG_LEN) / SEG_LEN;
        slot.x_start + slot.bend * smooth(t)
    }

    fn slot_of(&self, seg: i64) -> usize {
        (self.head + (seg - self.first_seg) as usize) % RING
    }

    /// Make sure segment `seg` is in the ring, generating bends for the
    /// segments entering at the far end. `seg` must not lie before
    /// `first_seg`; callers only look at the visible window, which the ring
    /// always covers with room to spare.
    fn ensure(&mut self, seg: i64) {
        while self.first_seg + RING as i64 <= seg {
            let tail = (self.head + RING - 1) % RING;
            let end_x = self.slots[tail].x_start + self.slots[tail].bend;
            let bend = self.gen_bend(end_x);
            // The slot that held `first_seg` is recycled for `first_seg + RING`.
            self.slots[self.head] = Slot { x_start: end_x, bend };
            self.head = (self.head + 1) % RING;
            self.first_seg += 1;
        }
    }

    /// A bend that steers the road towards a random on-screen target.
    fn gen_bend(&mut self, end_x: f32) -> f32 {
        if self.rng.below(4) == 0 {
            return 0.0; // straight segment
        }
        let target = self.rng.in_range(self.min_cx, self.max_cx);
        (target - end_x).clamp(-MAX_BEND, MAX_BEND)
    }
}

/// Cosine ease-in-out: 0 at t=0, 1 at t=1, with a horizontal slope at both
/// ends, so consecutive segments join without kinks.
fn smooth(t: f32) -> f32 {
    (1.0 - math::cos(PI * t)) * 0.5
}

/// Draw one scanline of grass detail: tile row `v` of the 16x16 grass
/// texture, repeated across the whole screen width. `u_off` shifts the
/// tile origin sideways so the tiling does not line up into an obvious
/// grid.
pub fn grass_line(screen: &mut Screen, y: u32, v: u32, u_off: u32) {
    let width = screen.width();
    let v = v % GRASS_TILE;
    for u in 0..GRASS_TILE {
        if let Some(color) = assets::GRASS.pixel(u, v) {
            let mut x = (u + u_off) % GRASS_TILE;
            while x < width {
                screen.px(x, y, color);
                x += GRASS_TILE;
            }
        }
    }
}

/// Draw the meadow for the scanline showing track distance `d`: a mown
/// band, the grass texture on top, and the occasional flower.
pub fn meadow_line(screen: &mut Screen, y: u32, d: f32) {
    let band = math::floor(d / 32.0);
    let base = if (band as u32) & 1 == 0 { COL_MEADOW_A } else { COL_MEADOW_B };
    screen.hline(0.0, screen.width() as f32, y, base);

    // The tile row follows the world distance, so the grass scrolls with
    // the road instead of sitting still; each band shifts the tile
    // sideways a bit to break up the grid.
    let v = (math::floor(d) as u32) % GRASS_TILE;
    let u_off = (band as u32).wrapping_mul(5) % GRASS_TILE;
    grass_line(screen, y, v, u_off);

    // Flowers whose positions derive from the band index, so they scroll
    // with the world instead of flickering.
    if d - band * 32.0 < 3.0 {
        let h = hash(band as u32);
        let span = screen.width() - 12;
        let fx1 = 4.0 + (h % span) as f32;
        let fx2 = 4.0 + ((h >> 9) % span) as f32;
        screen.hline(fx1, fx1 + 2.0, y, COL_FLOWER);
        screen.hline(fx2, fx2 + 2.0, y, COL_FLOWER);
    }
}

/// Draw the meadow and the road visible at scroll offset `scroll`.
///
/// The road is rendered per scanline: row `y` shows track distance
/// `d = scroll + (height - y)`, so the road curves smoothly with no
/// geometry engine involved.
pub fn render(screen: &mut Screen, track: &mut Track, scroll: f32) {
    let height = screen.height() as f32;
    for y in 0..height as u32 {
        let d = scroll + (height - y as f32);
        let cx = track.center_x(d);

        meadow_line(screen, y, d);

        // Road with edge stripes and a dashed center line.
        screen.hline(cx - ROAD_HALF_W - EDGE_W, cx - ROAD_HALF_W, y, COL_EDGE);
        screen.hline(cx + ROAD_HALF_W, cx + ROAD_HALF_W + EDGE_W, y, COL_EDGE);
        screen.hline(cx - ROAD_HALF_W, cx + ROAD_HALF_W, y, COL_ROAD);
        if d % 48.0 < 24.0 {
            screen.hline(cx - 2.0, cx + 2.0, y, COL_DASH);
        }
    }
}

/// Small integer hash (splitmix-style finalizer).
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    x
}
