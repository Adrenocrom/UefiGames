//! Kallirs — a car game that runs directly on UEFI.
//!
//! The player steers left or right; the car stays near the bottom of the
//! screen while the world scrolls downwards. Rendering goes through a
//! software back buffer that is blitted to the GOP framebuffer once per
//! frame; a periodic timer event paces the main loop (busy-waiting with
//! `stall` would burn CPU on top of the frame work).

#![no_std]
#![no_main]

extern crate alloc;

mod assets;
mod draw;
mod game;
mod math;
mod world;

use core::time::Duration;
use uefi::boot::{self, EventType, Tpl, TimerTrigger};
use uefi::prelude::*;
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use uefi::proto::console::text::{Key, ScanCode};

use crate::game::Game;

/// Frame period (~60 FPS).
const FRAME_MS: u64 = 16;

/// Smallest display we can play on. The road plus meadow margins must fit
/// horizontally; the height must leave room for the car zone and the HUD.
const MIN_WIDTH: usize = 640;
const MIN_HEIGHT: usize = 400;

#[entry]
fn main() -> Status {
    let _ = uefi::helpers::init();

    // --- Graphics output ---
    let gop_handle = match boot::get_handle_for_protocol::<GraphicsOutput>() {
        Ok(h) => h,
        Err(e) => {
            uefi::println!("Kallirs: no graphics output ({:?})", e.status());
            return e.status();
        }
    };
    let mut gop = match boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle) {
        Ok(g) => g,
        Err(e) => {
            uefi::println!("Kallirs: cannot open graphics output ({:?})", e.status());
            return e.status();
        }
    };

    // Use the current mode; only switch up if it is unusably small.
    let (mut width, mut height) = gop.current_mode_info().resolution();
    //if width < MIN_WIDTH {
        if let Some(mode) = gop.modes().find(|m| m.info().resolution() == (640, 480)) {
            if gop.set_mode(&mode).is_ok() {
                let (w, h) = gop.current_mode_info().resolution();
                width = w;
                height = h;
            }
        }
    //}
    if width < MIN_WIDTH || height < MIN_HEIGHT {
        uefi::println!("Kallirs: display {}x{} is too small", width, height);
        return Status::UNSUPPORTED;
    }

    // Hide the blinking text cursor while the game owns the screen.
    system::with_stdout(|out| {
        let _ = out.enable_cursor(false);
    });

    // --- Input ---
    let key_event = match system::with_stdin(|input| input.wait_for_key_event()) {
        Ok(e) => e,
        Err(e) => {
            uefi::println!("Kallirs: no keyboard ({:?})", e.status());
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
                        Key::Special(sc) if sc.0 == ScanCode::LEFT.0 => game.steer_left(),
                        Key::Special(sc) if sc.0 == ScanCode::RIGHT.0 => game.steer_right(),
                        Key::Printable(c) if c == ' ' => game.primary_action(),
                        _ => {}
                    }
                }
            });
        }

        if idx == 1 {
            // Frame boundary: advance physics, draw.
            game.update();
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
    uefi::println!("Kallirs: thanks for playing!");

    Status::SUCCESS
}
