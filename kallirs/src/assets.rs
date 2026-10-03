//! Sprites: pixel art as string maps plus shape-based drawing helpers.
//!
//! Each sprite is an array of equal-length strings; one character is one
//! sprite pixel, mapped through `palette`. '.' is transparent. This keeps
//! the art readable and editable directly in the source.

use crate::draw::{Bitmap, Color, Screen};
use crate::math;
use core::f32::consts::PI;

const TAU: f32 = 2.0 * PI;

/// A string-map sprite.
pub struct Sprite {
    rows: &'static [&'static str],
}

impl Sprite {
    pub const fn new(rows: &'static [&'static str]) -> Sprite {
        Sprite { rows }
    }
}

impl Bitmap for Sprite {
    fn dims(&self) -> (u32, u32) {
        (self.rows[0].len() as u32, self.rows.len() as u32)
    }

    fn pixel(&self, u: u32, v: u32) -> Option<Color> {
        palette(self.rows[v as usize].as_bytes()[u as usize] as char)
    }
}

/// Character -> color map shared by all sprites.
fn palette(ch: char) -> Option<Color> {
    Some(match ch {
        'R' => Color::new(214, 36, 36),  // car body
        'W' => Color::new(255, 255, 255),  // car body
        'B' => Color::new(255, 96, 84),  // brake lights
        'Y' => Color::new(255, 224, 96), // headlights
        'G' => Color::new(52, 66, 82),  // glass
        'T' => Color::new(28, 28, 30),  // tires
        'L' => Color::new(46, 168, 58), // shamrock leaf
        'l' => Color::new(30, 120, 42), // shamrock stem
        'h' => Color::new(92, 148, 64), // grass blade, lit
        's' => Color::new(24, 64, 30),  // grass blade, shaded
        'd' => Color::new(96, 78, 58),  // dirt patch
        'k' => Color::new(70, 56, 40),  // dirt crumb, dark
        _ => return None,               // '.' and anything else: transparent
    })
}

/// Top-down car, nose up. 16x26.
pub const CAR: Sprite = Sprite::new(&[
    "..RRRRRWWRRRRR..",
    ".RRYYRRWWRRYYRR.",
    "TRRRRRRWWRRRRRRT",
    "TRRRRRRWWRRRRRRT",
    "TRRRRRRWWRRRRRRT",
    "TRRRRRRWWRRRRRRT",
    "TRRRRRRWWRRRRRRT",
    ".RGGGGGGGGGGGGR.",
    ".RGGGGGGGGGGGGR.",
    ".RGGGGGGGGGGGGR.",
    ".RGGGRRWWRRGGGR.",
    ".RRRRRRWWRRRRRR.",
    ".RRRRRRWWRRRRRR.",
    ".RRRRRRWWRRRRRR.",
    ".RRRRRRWWRRRRRR.",
    ".RGGGGGGGGGGGGR.",
    ".RGGGGGGGGGGGGR.",
    ".RRGGGGGGGGGGRR.",
    "TRRRRRRWWRRRRRRT",
    "TRRRRRRWWRRRRRRT",
    "TRRRRRRWWRRRRRRT",
    "TRRRRRRWWRRRRRRT",
    "TRRRRRRWWRRRRRRT",
    ".RRRRRRWWRRRRRR.",
    ".RRBBRRWWRRBBRR.",
    "..RRRRRWWRRRRR..",
]);

/// Shamrock. 13x13.
pub const SHAMROCK: Sprite = Sprite::new(&[
    ".............",
    "..LLL...LLL..",
    ".LLLLL.LLLLL.",
    ".LLLLL.LLLLL.",
    ".LLLLL.LLLLL.",
    ".LLLLL.LLLLL.",
    "...L.....L...",
    "....LLLLL....",
    "....LLLLL....",
    "....LLLLL....",
    "....LLLLL....",
    "......l......",
    "......l......",
]);

/// Grass detail tile, 16x16. '.' is transparent: the mown band color shows
/// through. 'h' blades catch the light, 's' marks the shaded blade bases
/// and pebbles. Tiled over the meadow by `world::meadow_line`, which locks
/// the tile rows to the world distance so the grass scrolls with the road.
pub const GRASS: Sprite = Sprite::new(&[
    "...h......h.....",
    "...s.s....s..h..",
    "......h......s..",
    ".h....s.........",
    ".s.......s..h...",
    "........h...s...",
    "s...h...s.......",
    "....s.........h.",
    "h.............s.",
    "s..s.....h......",
    "..h......s...s..",
    "..s........h....",
    ".....h.....s....",
    ".....s.h........",
    "......ss.......h",
    "...............s",
]);

/// Dirt patch on the asphalt, 12x8. 'd' is the dirt, 'k' the darker
/// crumb specks; '.' lets the road color show through so the patch sits
/// flat on the surface. Drawn at 2x (24x16) by `game.rs`.
pub const DIRT: Sprite = Sprite::new(&[
    "....dddd....",
    "..dddddddd..",
    ".dddddddddd.",
    "ddddkkdddddd",
    "dddddddddddd",
    ".dddddddddd.",
    "..dddddddd..",
    "....dddd....",
]);

/// Wedge colors of the rainbow ball, starting at the ray given by `phase`.
const BALL_WEDGES: [Color; 6] = [
    Color::new(230, 57, 70),  // red
    Color::new(255, 146, 40), // orange
    Color::new(255, 216, 61), // yellow
    Color::new(76, 187, 91),  // green
    Color::new(58, 134, 255), // blue
    Color::new(170, 92, 232), // violet
];
const COL_BALL_RIM: Color = Color::new(250, 250, 250);
const COL_BALL_DOT: Color = Color::new(250, 250, 250);
const COL_STRIPE_A: Color = Color::new(240, 140, 30);
const COL_STRIPE_B: Color = Color::new(245, 245, 245);
const COL_BARRIER_EDGE: Color = Color::new(40, 40, 44);

/// A rainbow ball like the ones kids play with: six colored wedges
/// (red/orange/yellow/green/blue/violet) that rotate with the roll, a white
/// rim and a white dot orbiting the hub. `phase` is the rotation of the
/// surface pattern; the caller advances it with the rolling distance so the
/// spin is visible.
///
/// `core` has no `atan2`, so the wedge is picked per pixel with cross-product
/// tests against the six boundary rays: pixel direction `p` lies in wedge `i`
/// iff it is clockwise of ray `i` and counterclockwise of ray `i+1`. The
/// cross product `a x b = ax*by - ay*bx` is positive when `b` is clockwise
/// from `a` in screen coordinates (y down).
pub fn draw_ball(screen: &mut Screen, x: f32, y: f32, r: f32, phase: f32) {
    let r2 = r * r;
    let x0 = math::floor(x - r).max(0.0) as u32;
    let y0 = math::floor(y - r).max(0.0) as u32;
    let x1 = math::ceil(x + r).min(screen.width() as f32) as u32;
    let y1 = math::ceil(y + r).min(screen.height() as f32) as u32;

    // Unit vectors of the six wedge boundaries, rotated by `phase`.
    let mut rays = [(0.0f32, 0.0f32); 6];
    for (i, ray) in rays.iter_mut().enumerate() {
        let a = phase + (i as f32 / 6.0) * TAU;
        *ray = (math::cos(a), math::sin(a));
    }

    for yy in y0..y1 {
        for xx in x0..x1 {
            let dx = xx as f32 + 0.5 - x;
            let dy = yy as f32 + 0.5 - y;
            let d2 = dx * dx + dy * dy;
            if d2 > r2 {
                continue;
            }
            // Rim: outer 12% of the radius.
            let color = if d2 > r2 * 0.77 {
                COL_BALL_RIM
            } else {
                // Which wedge: the first boundary ray the pixel is
                // counterclockwise of. A pixel exactly on a ray counts as
                // the wedge that starts there.
                let mut wedge = 5;
                for i in 0..6 {
                    let (rx, ry) = rays[i];
                    // Counterclockwise of ray i: cross(ray, p) <= 0.
                    if rx * dy - ry * dx <= 0.0 {
                        wedge = i;
                        break;
                    }
                }
                BALL_WEDGES[wedge]
            };
            screen.px(xx, yy, color);
        }
    }

    // White hub dot, orbiting with the roll.
    let dx = math::cos(phase) * r * 0.5;
    let dy = math::sin(phase) * r * 0.5;
    screen.circle(x + dx, y + dy, r * 0.28, COL_BALL_DOT);
}

/// A striped roadworks barrier block with a dark outline.
pub fn draw_barrier(screen: &mut Screen, x: f32, y: f32, w: f32, h: f32) {
    let (x0, y0) = (x as u32, y as u32);
    for py in 0..h as u32 {
        for px in 0..w as u32 {
            let border = py == 0 || py + 1 == h as u32 || px == 0 || px + 1 == w as u32;
            let color = if border {
                COL_BARRIER_EDGE
            } else if (px + py) % 16 < 8 {
                COL_STRIPE_A
            } else {
                COL_STRIPE_B
            };
            screen.px(x0 + px, y0 + py, color);
        }
    }
}

/// Height of the bridge deck (the part that hides the car), px.
pub const BRIDGE_DECK_H: f32 = 46.0;
/// Height of the bridge railing on top of the deck, px.
const BRIDGE_RAIL_H: u32 = 10;
/// Height of the bridge piers, px.
const BRIDGE_PIER_H: u32 = 26;
/// Width of a bridge pier, px.
const BRIDGE_PIER_W: u32 = 18;

const COL_BRIDGE_DECK: Color = Color::new(120, 116, 110);
const COL_BRIDGE_DECK_DARK: Color = Color::new(88, 84, 80);
const COL_BRIDGE_RAIL: Color = Color::new(150, 146, 140);
const COL_BRIDGE_PIER: Color = Color::new(104, 100, 96);

/// An overhead bridge crossing the whole road at screen y `y` (the deck
/// center). It spans the full screen width so the car cannot drive around
/// it, and it is drawn after the car so it hides the car and anything else
/// passing underneath.
///
/// The deck is a horizontal band with a dark underside edge; the railing on
/// top has posts every 24 px; the piers below reach down towards the road
/// surface. The shadow the bridge casts on the road is drawn separately by
/// `game.rs` (it must follow the road curve).
pub fn draw_bridge(screen: &mut Screen, y: f32) {
    let width = screen.width();
    let y0 = (y - BRIDGE_DECK_H / 2.0) as u32;
    let y1 = y0 + BRIDGE_DECK_H as u32;

    // Deck: a light band with a dark underside edge (the shadow side).
    screen.rect(0, y0, width, BRIDGE_DECK_H as u32, COL_BRIDGE_DECK);
    screen.rect(0, y1 - 6, width, 6, COL_BRIDGE_DECK_DARK);

    // Railing on top of the deck: a band with posts every 24 px.
    // `saturating_sub`: a bridge entering from the top of the screen has
    // its deck top clipped at y=0, and the railing must not underflow.
    let rail_y = y0.saturating_sub(BRIDGE_RAIL_H);
    screen.rect(0, rail_y, width, BRIDGE_RAIL_H, COL_BRIDGE_RAIL);
    let mut px = 0;
    while px < width {
        screen.rect(px, rail_y, 3, BRIDGE_RAIL_H, COL_BRIDGE_DECK_DARK);
        px += 24;
    }

    // Piers below the deck, reaching down towards the road.
    let pier_y = y1;
    let mut px = 24;
    while px < width {
        screen.rect(px, pier_y, BRIDGE_PIER_W, BRIDGE_PIER_H, COL_BRIDGE_PIER);
        screen.rect(px, pier_y, 3, BRIDGE_PIER_H, COL_BRIDGE_DECK_DARK);
        px += 160;
    }
}
