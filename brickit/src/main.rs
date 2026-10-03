//! BrickShot — a mouse-controlled breakout game that runs directly on UEFI.
//!
//! The paddle follows the mouse. UEFI has two pointer protocols and firmware
//! exposes either one depending on the device: `AbsolutePointer` (USB tablets,
//! touchscreens — reports absolute coordinates) and `Pointer` (PS/2 mice —
//! reports relative movement only). Both are supported; for relative mice a
//! virtual cursor position is accumulated from the deltas. Arrow keys work as
//! a fallback if no pointer device is present.
//!
//! Rendering goes through a software back buffer that is blitted to the GOP
//! framebuffer once per frame; a periodic timer event paces the main loop
//! (busy-waiting with `stall` would burn CPU on top of the frame work).

#![no_std]
#![no_main]

extern crate alloc;

mod draw;
mod game;
mod math;

use core::time::Duration;
use uefi::boot::{self, EventType, ScopedProtocol, Tpl, TimerTrigger};
use uefi::prelude::*;
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use uefi::proto::console::pointer::{AbsolutePointer, Pointer};
use uefi::proto::console::text::{Key, ScanCode};

use crate::game::Game;

/// Frame period (~60 FPS).
const FRAME_MS: u64 = 16;

/// Smallest display we can play on. The brick grid is 10 * (64 + 6) - 6 =
/// 694 px wide; on anything narrower the outer brick columns would be
/// clipped off-screen and the level could never be cleared. The height
/// leaves room for the grid (which ends at y = 194), the paddle and the
/// menu footer.
const MIN_WIDTH: usize = 700;
const MIN_HEIGHT: usize = 320;

/// How many frames a key press counts as "held". UEFI reports key presses
/// only (there are no release events), so this bridges the keyboard's
/// auto-repeat delay (typically 250–500 ms) and lets arrow keys move the
/// paddle continuously.
const HOLD_FRAMES: u32 = 8;

struct HeldKeys {
    left: u32,
    right: u32,
}

impl HeldKeys {
    fn new() -> Self {
        HeldKeys { left: 0, right: 0 }
    }

    fn tick(&mut self) {
        self.left = self.left.saturating_sub(1);
        self.right = self.right.saturating_sub(1);
    }
}

/// A polled pointer device, abstracting over both UEFI pointer protocols.
struct Mouse {
    inner: MouseInner,
    prev_left: bool,
}

enum MouseInner {
    /// PS/2-style mouse: relative deltas, scaled to pixels.
    Relative {
        proto: ScopedProtocol<Pointer>,
        px_per_count: f32,
    },
    /// USB tablet / touchscreen: absolute coordinates mapped to the screen.
    Absolute {
        proto: ScopedProtocol<AbsolutePointer>,
        min_x: f32,
        min_y: f32,
        scale_x: f32,
        scale_y: f32,
    },
}

impl Mouse {
    /// Drain all pending state updates. Returns the latest absolute position
    /// (if any), the accumulated relative movement in pixels, and whether the
    /// left button was pressed since the last poll.
    fn poll(&mut self) -> (Option<(f32, f32)>, f32, f32, bool) {
        let mut abs = None;
        let mut rel_dx = 0.0;
        let mut rel_dy = 0.0;
        let mut clicked = false;
        loop {
            let update = match &mut self.inner {
                MouseInner::Relative { proto, px_per_count } => match proto.read_state() {
                    Ok(Some(st)) => Some((
                        None,
                        st.relative_movement_x as f32 * *px_per_count,
                        st.relative_movement_y as f32 * *px_per_count,
                        st.left_button.is_true(),
                    )),
                    _ => None,
                },
                MouseInner::Absolute { proto, min_x, min_y, scale_x, scale_y } => match proto.read_state() {
                    // Bit 0 of active_buttons is the primary button.
                    Ok(Some(st)) => Some((
                        Some((
                            (st.current_x as f32 - *min_x) * *scale_x,
                            (st.current_y as f32 - *min_y) * *scale_y,
                        )),
                        0.0,
                        0.0,
                        st.active_buttons & 0x1 != 0,
                    )),
                    _ => None,
                },
            };
            let Some((a, rx, ry, left)) = update else {
                break;
            };
            if a.is_some() {
                abs = a;
            }
            rel_dx += rx;
            rel_dy += ry;
            if left && !self.prev_left {
                clicked = true;
            }
            self.prev_left = left;
        }
        (abs, rel_dx, rel_dy, clicked)
    }
}

/// Open the best available pointer device, preferring absolute pointers.
fn open_mouse(screen_w: f32, screen_h: f32) -> Option<Mouse> {
    if let Ok(handle) = boot::get_handle_for_protocol::<AbsolutePointer>() {
        if let Ok(mut proto) = boot::open_protocol_exclusive::<AbsolutePointer>(handle) {
            let _ = proto.reset(false);
            let mode = proto.mode();
            let min_x = mode.absolute_min_x as f32;
            let max_x = mode.absolute_max_x as f32;
            let min_y = mode.absolute_min_y as f32;
            let max_y = mode.absolute_max_y as f32;
            let scale_x = if max_x > min_x {
                screen_w / (max_x - min_x)
            } else {
                screen_w / 32767.0
            };
            let scale_y = if max_y > min_y {
                screen_h / (max_y - min_y)
            } else {
                screen_h / 32767.0
            };
            return Some(Mouse {
                inner: MouseInner::Absolute { proto, min_x, min_y, scale_x, scale_y },
                prev_left: false,
            });
        }
    }
    if let Ok(handle) = boot::get_handle_for_protocol::<Pointer>() {
        if let Ok(mut proto) = boot::open_protocol_exclusive::<Pointer>(handle) {
            let _ = proto.reset(false);
            let counts_per_mm = proto.mode().resolution_x as f32;
            // Scale so ~10 cm of desk travel sweeps the full screen width.
            let px_per_count = if counts_per_mm >= 1.0 {
                (screen_w / (counts_per_mm * 100.0)).clamp(0.05, 8.0)
            } else {
                2.0
            };
            return Some(Mouse {
                inner: MouseInner::Relative { proto, px_per_count },
                prev_left: false,
            });
        }
    }
    None
}

#[entry]
fn main() -> Status {
    let _ = uefi::helpers::init();

    // --- Graphics output ---
    let gop_handle = match boot::get_handle_for_protocol::<GraphicsOutput>() {
        Ok(h) => h,
        Err(e) => {
            uefi::println!("BrickShot: no graphics output ({:?})", e.status());
            return e.status();
        }
    };
    let mut gop = match boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle) {
        Ok(g) => g,
        Err(e) => {
            uefi::println!("BrickShot: cannot open graphics output ({:?})", e.status());
            return e.status();
        }
    };

    // Use the current mode; only switch up if it is unusably small.
    let (mut width, mut height) = gop.current_mode_info().resolution();
        if let Some(mode) = gop.modes().find(|m| m.info().resolution() == (1024, 768)) {
            if gop.set_mode(&mode).is_ok() {
                let (w, h) = gop.current_mode_info().resolution();
                width = w;
                height = h;
            }
        }
    if width < MIN_WIDTH || height < MIN_HEIGHT {
        uefi::println!("BrickShot: display {}x{} is too small", width, height);
        return Status::UNSUPPORTED;
    }

    // Hide the blinking text cursor while the game owns the screen.
    system::with_stdout(|out| {
        let _ = out.enable_cursor(false);
    });

    // --- Input devices ---
    let mut mouse = open_mouse(width as f32, height as f32);
    let mut held = HeldKeys::new();

    let key_event = match system::with_stdin(|input| input.wait_for_key_event()) {
        Ok(e) => e,
        Err(e) => {
            uefi::println!("BrickShot: no keyboard ({:?})", e.status());
            return e.status();
        }
    };
    // SAFETY: plain waitable timer, no notification callback.
    let timer_event = match unsafe {
        boot::create_event(EventType::TIMER, Tpl::APPLICATION, None, None)
    } {
        Ok(e) => e,
        Err(e) => return e.status(),
    };
    if let Err(e) = boot::set_timer(&timer_event, TimerTrigger::Periodic(Duration::from_millis(FRAME_MS))) {
        let _ = boot::close_event(timer_event);
        return e.status();
    }

    let mut screen = draw::Screen::new(width as u32, height as u32);
    let mut game = Game::new(width as f32, height as f32);
    // UEFI mice can be relative-only, so we track a virtual cursor ourselves.
    let mut cursor_x = width as f32 / 2.0;
    let mut cursor_y = height as f32 / 2.0;
    let key_speed = width as f32 / 90.0; // arrow-key paddle speed, px/frame

    let events = [key_event, timer_event];
    loop {
        let idx = match boot::wait_for_event(&events).discard_errdata() {
            Ok(i) => i,
            Err(_) => break,
        };

        if idx == 0 {
            // A key arrived: drain every pending press.
            system::with_stdin(|input| {
                while let Ok(Some(key)) = input.read_key() {
                    match key {
                        Key::Special(sc) if sc.0 == ScanCode::ESCAPE.0 => game.escape(),
                        Key::Special(sc) if sc.0 == ScanCode::LEFT.0 => held.left = HOLD_FRAMES,
                        Key::Special(sc) if sc.0 == ScanCode::RIGHT.0 => held.right = HOLD_FRAMES,
                        Key::Special(sc) if sc.0 == ScanCode::UP.0 => {
                            game.menu_up();
                        }
                        Key::Special(sc) if sc.0 == ScanCode::DOWN.0 => {
                            game.menu_down();
                        }
                        Key::Printable(c) if c == ' ' => game.primary_action(),
                        _ => {}
                    }
                }
            });
        }

        if idx == 1 {
            // Frame boundary: apply input, advance physics, draw.
            let mut clicked = false;
            if let Some(m) = mouse.as_mut() {
                let (abs, rel_dx, rel_dy, click) = m.poll();
                if let Some((x, y)) = abs {
                    cursor_x = x;
                    cursor_y = y;
                }
                cursor_x += rel_dx;
                cursor_y += rel_dy;
                clicked = click;
            }
            held.tick();
            // LEFT/RIGHT steer the paddle during play. In the menu they
            // mean nothing and would only push the cursor dot around,
            // fighting the UP/DOWN selection.
            if !game.in_menu() {
                if held.left > 0 {
                    cursor_x -= key_speed;
                }
                if held.right > 0 {
                    cursor_x += key_speed;
                }
            }
            cursor_x = cursor_x.clamp(0.0, width as f32);
            cursor_y = cursor_y.clamp(0.0, height as f32);

            // Update before handling the click: in the menu this refreshes
            // the hover highlight from the current cursor position, so a
            // fast move+click in the same frame still hits the right entry.
            game.update(cursor_x, cursor_y);
            if clicked {
                game.primary_action();
            }
            game.render(&mut screen);
            let _ = gop.blt(BltOp::BufferToVideo {
                buffer: screen.pixels(),
                src: BltRegion::Full,
                dest: (0, 0),
                dims: (width, height),
            });
        }

        if game.wants_quit() {
            break;
        }
    }

    // Stop the frame timer (the key event belongs to the console driver).
    let [_, timer_event] = events;
    let _ = boot::set_timer(&timer_event, TimerTrigger::Cancel);
    let _ = boot::close_event(timer_event);

    // Leave a clean text console behind.
    let _ = gop.blt(BltOp::VideoFill {
        color: BltPixel::new(0, 0, 0),
        dest: (0, 0),
        dims: (width, height),
    });
    system::with_stdout(|out| {
        let _ = out.set_cursor_position(0, 0);
        let _ = out.enable_cursor(true);
    });
    uefi::println!("BrickShot: thanks for playing!");

    Status::SUCCESS
}
