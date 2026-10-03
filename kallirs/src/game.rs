//! Game state and physics for Kallirs.
//!
//! The car stays near the bottom of the screen while the world scrolls
//! downwards. The track lives in `world.rs`, the sprites in `assets.rs`;
//! this module owns the phase machine, the car, obstacles, shamrocks and
//! collisions.

use crate::assets;
use crate::draw::{self, Screen};
use crate::math;
use crate::world::{self, Track};
use core::f32::consts::PI;

// --- Layout tunables (pixels) ---
/// Car sprite size (width x height).
const CAR_W: f32 = 32.0;
const CAR_H: f32 = 52.0;
/// On-screen scale of the car sprite (16x26 art drawn at 32x52).
const CAR_SCALE: f32 = 3.0;
/// Gap between the car's bottom edge and the screen bottom.
const CAR_BOTTOM_MARGIN: f32 = 40.0;
/// Radius of the rolling boulders.
const BALL_R: f32 = 14.0;
/// Size of a construction-site barrier block.
const SITE_W: f32 = 70.0;
const SITE_H: f32 = 30.0;
/// Shamrock sprite size (13x13 art drawn at 26x26).
const SHAM_W: f32 = 26.0;
const SHAM_H: f32 = 26.0;
/// On-screen scale of the shamrock sprite.
const SHAM_SCALE: u32 = 2;
/// Dirt patch sprite size (12x8 art drawn at 24x16).
const DIRT_W: f32 = 48.0;
const DIRT_H: f32 = 32.0;
/// On-screen scale of the dirt patch sprite.
const DIRT_SCALE: u32 = 2;
/// Height of the shadow a bridge casts on the road below its deck, px.
const BRIDGE_SHADOW_H: f32 = 22.0;

// --- Driving tunables ---
/// Sideways car speed at full heading (+/-45 degrees), px/frame.
const CAR_SPEED: f32 = 6.0;
/// Maximum steering angle: 45 degrees.
const MAX_ANGLE: f32 = PI / 4.0;
/// Steering stages per side. Each arrow press steps the heading by one
/// stage; the target angle is `steer / STEER_STAGES * MAX_ANGLE`, i.e.
/// 15, 30 or 45 degrees per side.
const STEER_STAGES: i32 = 3;
/// Fraction of the remaining angle error corrected per frame (0..1).
/// The lateral speed follows the eased angle, so this also shapes how
/// quickly the car picks up (or loses) sideways speed.
const ANGLE_EASE: f32 = 0.35;
/// World scroll speed at the start of a run, px/frame.
const START_SPEED: f32 = 3.0;
/// World scroll speed ramp, px/frame per frame.
const SPEED_RAMP: f32 = 0.0015;
/// Maximum world scroll speed, px/frame.
const MAX_SPEED: f32 = 9.0;

// --- Spawning tunables ---
/// Number of obstacle/shamrock slots. Fixed-size arrays keep this `no_std`
/// and allocation-free.
const SLOTS: usize = 24;
/// Distance ahead of the car at which new things spawn, px.
const SPAWN_AHEAD: f32 = 900.0;
/// Minimum distance between two spawns, px.
const SPAWN_GAP: f32 = 130.0;
/// Probability weights per spawn roll (out of 100).
const P_BALL: u32 = 22;
const P_SITE: u32 = 22;
const P_SHAMROCK: u32 = 34;
const P_DIRT: u32 = 20;
/// Track distance of the first bridge of a run, px.
const FIRST_BRIDGE_D: f32 = 1400.0;
/// Bridges recur this many px apart (randomized within the range).
const BRIDGE_GAP_MIN: f32 = 1600.0;
const BRIDGE_GAP_MAX: f32 = 2600.0;

const COL_TEXT: draw::Color = draw::Color::new(255, 255, 255);
const COL_DIM: draw::Color = draw::Color::new(170, 170, 170);
const COL_SHADOW: draw::Color = draw::Color::new(0, 0, 0);
const COL_BRIDGE_SHADOW: draw::Color = draw::Color::new(44, 46, 50);

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Title,
    Playing,
    GameOver,
}

/// What occupies a spawn slot.
#[derive(Clone, Copy)]
enum Thing {
    /// A boulder rolling sideways; `x` is its center, `vx` its speed.
    Ball { x: f32, vx: f32 },
    /// A static construction-site barrier; `x` is its center.
    Site { x: f32 },
    /// A collectible shamrock; `x` is its center.
    Shamrock { x: f32 },
    /// A dirt patch on the asphalt. Purely cosmetic.
    Dirt { x: f32 },
    /// An overhead bridge crossing the whole road. No collision; it is
    /// drawn over the car and hides whatever passes underneath.
    Bridge,
}

/// A thing plus the track distance at which it sits.
#[derive(Clone, Copy)]
struct Spawn {
    d: f32,
    thing: Thing,
}

pub struct Game {
    width: f32,
    height: f32,
    phase: Phase,
    /// Car center x. The car's y is fixed near the bottom; the world moves.
    car_x: f32,
    /// Heading notch: -STEER_STAGES = full left, 0 = straight,
    /// +STEER_STAGES = full right. Persists between key presses; each
    /// arrow press steps it by one stage.
    steer: i32,
    /// Current heading in radians, eased towards `steer * MAX_ANGLE`.
    car_angle: f32,
    /// Driven distance, px. Also the world scroll offset.
    scroll: f32,
    /// World scroll speed, px/frame.
    speed: f32,
    /// Shamrocks collected.
    score: u32,
    /// Set when the player asked to leave (ESC on the title screen).
    quit: bool,
    /// Xorshift32 state, seeded from the resolution (UEFI offers no entropy
    /// source). Feeds the track and the spawn rolls.
    rng: world::Rng,
    track: Track,
    /// Spawn slots; `None` = free. A slot is freed when its thing scrolls
    /// past the car.
    spawns: [Option<Spawn>; SLOTS],
    /// Track distance of the next spawn roll.
    next_spawn_d: f32,
    /// Track distance of the next bridge.
    next_bridge_d: f32,
}

impl Game {
    pub fn new(width: f32, height: f32) -> Game {
        let seed = 0x9E37_79B9 ^ (width as u32) ^ ((height as u32) << 16);
        let mut rng = world::Rng::new(seed);
        let track = Track::new(width, rng.next());
        Game {
            width,
            height,
            phase: Phase::Title,
            car_x: width / 2.0,
            steer: 0,
            car_angle: 0.0,
            scroll: 0.0,
            speed: START_SPEED,
            score: 0,
            quit: false,
            rng,
            track,
            spawns: [None; SLOTS],
            next_spawn_d: 0.0,
            next_bridge_d: FIRST_BRIDGE_D,
        }
    }

    /// Space bar: start a run, or restart after a crash.
    pub fn primary_action(&mut self) {
        if self.phase != Phase::Playing {
            self.score = 0;
            self.car_x = self.width / 2.0;
            self.steer = 0;
            self.car_angle = 0.0;
            self.scroll = 0.0;
            self.speed = START_SPEED;
            self.spawns = [None; SLOTS];
            self.next_spawn_d = SPAWN_AHEAD;
            self.next_bridge_d = FIRST_BRIDGE_D;
            self.track = Track::new(self.width, self.rng.next());
            self.phase = Phase::Playing;
        }
    }

    /// ESC: leave the current run for the title screen; ESC there quits.
    pub fn escape(&mut self) {
        if self.phase == Phase::Title {
            self.quit = true;
        } else {
            self.phase = Phase::Title;
        }
    }

    /// Whether the player asked to leave the game.
    pub fn wants_quit(&self) -> bool {
        self.quit
    }

    /// Y coordinate of the car center (fixed; the world scrolls past it).
    fn car_y(&self) -> f32 {
        self.height - CAR_BOTTOM_MARGIN - CAR_H / 2.0
    }

    /// Track distance currently under the car.
    fn car_d(&self) -> f32 {
        self.scroll + CAR_BOTTOM_MARGIN + CAR_H / 2.0
    }

    /// LEFT pressed: step the heading one stage left (saturating at full
    /// left). The car keeps the resulting heading until it is stepped back.
    pub fn steer_left(&mut self) {
        if self.phase == Phase::Playing {
            self.steer = (self.steer - 1).max(-STEER_STAGES);
        }
    }

    /// RIGHT pressed: step the heading one stage right (saturating at
    /// full right).
    pub fn steer_right(&mut self) {
        if self.phase == Phase::Playing {
            self.steer = (self.steer + 1).min(STEER_STAGES);
        }
    }

    /// Advance one frame. The car keeps its heading between key presses:
    /// it goes on drifting sideways until an arrow key steps the heading.
    pub fn update(&mut self) {
        if self.phase != Phase::Playing {
            return;
        }

        // The heading eases towards the stage set by the last key presses,
        // and the lateral speed follows the eased angle: full CAR_SPEED at
        // +/-45 degrees, none when straight. Straightening therefore bleeds
        // the drift off instead of stopping it dead.
        let target = self.steer as f32 / STEER_STAGES as f32 * MAX_ANGLE;
        self.car_angle += (target - self.car_angle) * ANGLE_EASE;
        self.car_x = (self.car_x + self.car_angle / MAX_ANGLE * CAR_SPEED)
            .clamp(CAR_W / 2.0, self.width - CAR_W / 2.0);

        // The world scrolls down; the car drives up the track.
        self.scroll += self.speed;
        self.speed = (self.speed + SPEED_RAMP).min(MAX_SPEED);

        self.spawn_ahead();
        self.ensure_site_ahead();
        self.update_things();
        self.check_collisions();
    }

    /// Roll new spawns up to `SPAWN_AHEAD` ahead of the car.
    fn spawn_ahead(&mut self) {
        let horizon = self.car_d() + SPAWN_AHEAD;
        while self.next_spawn_d < horizon {
            let d = self.next_spawn_d;
            let roll = self.rng.below(100);
            let thing = if roll < P_BALL {
                let cx = self.track.center_x(d);
                let vx = self.rng.in_range(1.5, 3.5) * if self.rng.below(2) == 0 { -1.0 } else { 1.0 };
                Some(Thing::Ball { x: cx, vx })
            } else if roll < P_BALL + P_SITE {
                let cx = self.track.center_x(d);
                let x = self.rng.in_range(cx - 130.0, cx + 130.0);
                Some(Thing::Site { x })
            } else if roll < P_BALL + P_SITE + P_SHAMROCK {
                let cx = self.track.center_x(d);
                let x = self.rng.in_range(cx - 150.0, cx + 150.0);
                Some(Thing::Shamrock { x })
            } else if roll < P_BALL + P_SITE + P_SHAMROCK + P_DIRT {
                let cx = self.track.center_x(d);
                let x = self.rng.in_range(cx - 160.0, cx + 160.0);
                Some(Thing::Dirt { x })
            } else {
                None
            };
            if let Some(thing) = thing {
                if let Some(slot) = self.spawns.iter_mut().find(|s| s.is_none()) {
                    *slot = Some(Spawn { d, thing });
                }
            }
            self.next_spawn_d += SPAWN_GAP;
        }

        // Bridges cross the whole road every so often. They are drawn over
        // the car, so they hide it (and anything else) while it passes
        // underneath.
        while self.next_bridge_d < horizon {
            let d = self.next_bridge_d;
            let gap = self.rng.in_range(BRIDGE_GAP_MIN, BRIDGE_GAP_MAX);
            if let Some(i) = self.spawns.iter().position(|s| s.is_none()) {
                self.spawns[i] = Some(Spawn { d, thing: Thing::Bridge });
                self.next_bridge_d = d + gap;
            } else {
                break; // no free slot; try again next frame
            }
        }
    }

    /// There is always at least one construction site ahead of the car.
    fn ensure_site_ahead(&mut self) {
        let car_d = self.car_d();
        let has_site = self
            .spawns
            .iter()
            .flatten()
            .any(|s| matches!(s.thing, Thing::Site { .. }) && s.d > car_d + 150.0);
        if has_site {
            return;
        }
        let d = car_d + self.rng.in_range(350.0, 800.0);
        let cx = self.track.center_x(d);
        let x = self.rng.in_range(cx - 130.0, cx + 130.0);
        if let Some(i) = self.spawns.iter().position(|s| s.is_none()) {
            self.spawns[i] = Some(Spawn { d, thing: Thing::Site { x } });
        }
    }

    /// Move rolling balls and free slots whose thing scrolled past the car.
    fn update_things(&mut self) {
        let car_d = self.car_d();
        for slot in self.spawns.iter_mut() {
            if let Some(spawn) = slot {
                if let Thing::Ball { x, vx } = &mut spawn.thing {
                    // Bounce at the road edges.
                    let cx = self.track.center_x(spawn.d);
                    let half = world::ROAD_HALF_W - BALL_R;
                    *x += *vx;
                    if *x < cx - half {
                        *x = cx - half;
                        *vx = -(*vx);
                    }
                    if *x > cx + half {
                        *x = cx + half;
                        *vx = -(*vx);
                    }
                }
                if spawn.d < car_d - 60.0 {
                    *slot = None;
                }
            }
        }
    }

    /// Off-road driving and crashes end the run; shamrocks score points.
    fn check_collisions(&mut self) {
        let car_d = self.car_d();
        let car_cx = self.track.center_x(car_d);
        let car_y = self.car_y();

        // Off the asphalt (edge stripes count as road).
        if (self.car_x - car_cx).abs() > world::ROAD_HALF_W - CAR_W / 2.0 {
            self.phase = Phase::GameOver;
            return;
        }

        for slot in self.spawns.iter_mut() {
            let Some(spawn) = slot else { continue };
            // Only things at the car's track distance can interact.
            if (spawn.d - car_d).abs() > 40.0 {
                continue;
            }
            let thing_y = self.height - (spawn.d - self.scroll);
            match spawn.thing {
                Thing::Ball { x, .. } => {
                    // Circle-vs-rect: closest point on the car body to the
                    // ball center, then a plain distance check.
                    let px = x.clamp(self.car_x - CAR_W / 2.0, self.car_x + CAR_W / 2.0);
                    let py = thing_y.clamp(car_y - CAR_H / 2.0, car_y + CAR_H / 2.0);
                    let dx = x - px;
                    let dy = thing_y - py;
                    if dx * dx + dy * dy < BALL_R * BALL_R {
                        self.phase = Phase::GameOver;
                        return;
                    }
                }
                Thing::Site { x } => {
                    let dy = car_y - thing_y;
                    if (self.car_x - x).abs() < (CAR_W + SITE_W) / 2.0
                        && dy.abs() < (CAR_H + SITE_H) / 2.0
                    {
                        self.phase = Phase::GameOver;
                        return;
                    }
                }
                Thing::Shamrock { x } => {
                    let dy = car_y - thing_y;
                    if (self.car_x - x).abs() < (CAR_W + SHAM_W) / 2.0
                        && dy.abs() < (CAR_H + SHAM_H) / 2.0
                    {
                        self.score += 1;
                        *slot = None;
                    }
                }
                // Dirt is cosmetic and bridges are overhead: no collision.
                Thing::Dirt { .. } | Thing::Bridge => {}
            }
        }
    }

    /// Screen y of a thing at track distance `d`.
    fn thing_y(&self, d: f32) -> f32 {
        self.height - (d - self.scroll)
    }

    pub fn render(&mut self, screen: &mut Screen) {
        match self.phase {
            Phase::Title => self.render_title(screen),
            Phase::Playing | Phase::GameOver => self.render_world(screen),
        }
    }

    fn render_title(&self, screen: &mut Screen) {
        // A frozen piece of meadow (mown bands, grass texture, flowers) as
        // the backdrop, matching the look of the game world.
        let height = screen.height();
        for y in 0..height {
            world::meadow_line(screen, y, (height - y) as f32);
        }
        let cy = height / 2;
        screen.text_centered(cy - 60, "KALLIRS", COL_TEXT);
        screen.text_centered(cy - 30, "STEER WITH LEFT AND RIGHT - THE CAR KEEPS ITS HEADING", COL_DIM);
        screen.text_centered(cy + 10, "PRESS SPACE TO START - ESC TO QUIT", COL_TEXT);
    }

    fn render_world(&mut self, screen: &mut Screen) {
        world::render(screen, &mut self.track, self.scroll);

        // Work on a copy of the spawn table: the bridge shadows need
        // `self.track` while iterating, and the copy keeps the borrows
        // disjoint.
        let spawns = self.spawns;

        // Dirt lies on the asphalt, under everything else.
        for spawn in spawns.iter().flatten() {
            if let Thing::Dirt { x } = spawn.thing {
                let y = self.thing_y(spawn.d);
                if y > -20.0 && y < self.height + 20.0 {
                    screen.blit_scaled(&assets::DIRT, x - DIRT_W / 2.0, y - DIRT_H / 2.0, DIRT_SCALE);
                }
            }
        }

        // Bridge shadows fall on the road below the deck, towards the
        // camera; the things standing on the road draw over them.
        for spawn in spawns.iter().flatten() {
            if let Thing::Bridge = spawn.thing {
                let y = self.thing_y(spawn.d);
                if y > -60.0 && y < self.height + 60.0 {
                    self.bridge_shadow(screen, y + assets::BRIDGE_DECK_H / 2.0);
                }
            }
        }

        // Things, farthest first so nearer ones draw on top.
        for spawn in spawns.iter().flatten() {
            let y = self.thing_y(spawn.d);
            if y < -40.0 || y > self.height + 40.0 {
                continue;
            }
            match spawn.thing {
                Thing::Ball { x, .. } => {
                    // The spin follows the horizontal travel, so it visibly
                    // reverses when the ball bounces off a road edge.
                    let phase = x * 0.15;
                    assets::draw_ball(screen, x, y, BALL_R, phase);
                }
                Thing::Site { x } => {
                    assets::draw_barrier(screen, x - SITE_W / 2.0, y - SITE_H / 2.0, SITE_W, SITE_H);
                }
                Thing::Shamrock { x } => {
                    screen.blit_scaled(&assets::SHAMROCK, x - SHAM_W / 2.0, y - SHAM_H / 2.0, SHAM_SCALE);
                }
                Thing::Dirt { .. } | Thing::Bridge => {}
            }
        }

        // The car, rotated by the steering angle (squashed for the tilt).
        screen.blit_rotated(&assets::CAR, self.car_x, self.car_y(), self.car_angle, CAR_SCALE);

        // Bridge decks, drawn over the car: the bridge blocks the view of
        // the car and of anything else passing underneath.
        for spawn in spawns.iter().flatten() {
            if let Thing::Bridge = spawn.thing {
                let y = self.thing_y(spawn.d);
                if y > -60.0 && y < self.height + 60.0 {
                    assets::draw_bridge(screen, y);
                }
            }
        }

        // HUD.
        //screen.rect(6, 6, draw::num_width("SHAMROCKS ", self.score) + 4, 20, COL_SHADOW);
        screen.text_num(8, 8, "SHAMROCKS ", self.score, COL_TEXT);

        if self.phase == Phase::GameOver {
            screen.rect(
                0,
                self.height as u32 / 2 - 34,
                self.width as u32,
                52,
                COL_SHADOW,
            );
            screen.text_centered(self.height as u32 / 2 - 20, "GAME OVER", COL_TEXT);
            screen.text_centered(
                self.height as u32 / 2 + 4,
                "SPACE TO RETRY - ESC FOR TITLE",
                COL_DIM,
            );
        }
    }

    /// The shadow a bridge casts on the road just below its deck. Drawn
    /// per scanline so it follows the road curve.
    fn bridge_shadow(&mut self, screen: &mut Screen, top: f32) {
        let y0 = math::floor(top).max(0.0) as u32;
        let y1 = math::ceil(top + BRIDGE_SHADOW_H).min(self.height) as u32;
        for y in y0..y1 {
            let d = self.scroll + (self.height - y as f32);
            let cx = self.track.center_x(d);
            screen.hline(
                cx - world::ROAD_HALF_W - world::EDGE_W,
                cx + world::ROAD_HALF_W + world::EDGE_W,
                y,
                COL_BRIDGE_SHADOW,
            );
        }
    }
}
