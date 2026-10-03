#![no_main]
#![no_std]

extern crate alloc;

use alloc::vec;
use core::time::Duration;

use uefi::data_types::Event;
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use uefi::proto::console::text::{Input, Key, ScanCode};
use uefi::{Result, ResultExt, Status, boot, entry, system};

const PADDLE_W: u32 = 16;
const PADDLE_H: u32 = 100;
const BALL_SIZE: u32 = 12;
const PADDLE_SPEED: i32 = 8; // pixels per frame while a key is held
const BALL_SPEED: f64 = 4.0; // pixels per timer tick
const FRAME_PERIOD: Duration = Duration::from_millis(16); // ~60 FPS

/// Frames a key stays "held" after its last press (~500 ms at 60 FPS).
///
/// UEFI never sends key-up events and offers no "is this key down?"
/// query, and the keyboard's auto-repeat only re-fires for the *last*
/// key pressed — so when the other player presses a key, our repeats
/// stop even though the key is still physically down. Refreshing a
/// countdown on every press bridges that gap, while the expiry keeps
/// released keys from sticking forever. ~500 ms also matches the
/// keyboard's initial auto-repeat delay, so a plain hold moves without
/// stuttering before repeats kick in.
const HOLD_FRAMES: u32 = 32;

const BACKGROUND: BltPixel = BltPixel::new(0, 0, 0);
const CENTER_LINE: BltPixel = BltPixel::new(60, 60, 60);
const LEFT_COLOR: BltPixel = BltPixel::new(0, 255, 255);
const RIGHT_COLOR: BltPixel = BltPixel::new(255, 0, 255);
const BALL_COLOR: BltPixel = BltPixel::new(255, 255, 255);

/// 3x5 bitmap font for the digits 0-9: one u16 per glyph, three bits
/// per row, five rows, top-left pixel in the highest bit.
///
/// UEFI's text output cannot be mixed with GOP double buffering (the
/// text console draws straight to the framebuffer, so our blits would
/// overwrite it every frame), so scores are rendered with our own tiny
/// font instead.
const DIGITS: [u16; 10] = [
    0b111_101_101_101_111, // 0
    0b010_110_010_010_111, // 1
    0b111_001_111_100_111, // 2
    0b111_001_111_001_111, // 3
    0b101_101_111_001_001, // 4
    0b111_100_111_001_111, // 5
    0b111_100_111_101_111, // 6
    0b111_001_001_001_001, // 7
    0b111_101_111_101_111, // 8
    0b111_101_111_001_111, // 9
];

/// Each font pixel is drawn as a SCALE x SCALE block; a raw 3x5 glyph
/// would be near-invisible at typical GOP resolutions.
const SCORE_SCALE: i32 = 8;

/// Left-edge distance between adjacent digits (glyph plus one font
/// pixel of spacing), in screen pixels.
const DIGIT_ADVANCE: i32 = 4 * SCORE_SCALE;

/// Ink width of one digit in screen pixels.
const DIGIT_W: i32 = 3 * SCORE_SCALE;

/// Top edge of the score display.
const SCORE_Y: i32 = 24;

/// Index of the frame-timer event in the array passed to `wait_for_event`
/// (the key event sits at index 0).
const TIMER_EVENT: usize = 1;

struct Paddle {
    x: i32,
    y: i32,
}

struct Ball {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
}

impl Ball {
    /// Reset to the center, reversing direction so the ball is served
    /// towards the player who just scored.
    fn reset_to_center(&mut self, w: i32, h: i32) {
        self.x = w as f64 / 2.0;
        self.y = h as f64 / 2.0;
        self.dx = -self.dx;
        self.dy = BALL_SPEED;
    }
}

/// A key we track as held. UEFI's console reports key *presses* only —
/// there is no key-up event — so held state has to be emulated.
enum HeldKey {
    Up,
    Down,
    W,
    S,
}

/// The set of all currently held keys, as one fixed-size array of
/// per-key countdowns.
///
/// Every press (auto-repeat included) refreshes the key's countdown to
/// `HOLD_FRAMES`, and every timer tick ages all countdowns by one; a
/// key counts as held while its countdown is non-zero. This is what
/// lets both paddles move at the same time: the keyboard only
/// auto-repeats the last key pressed, so without the countdowns player
/// 2's keypress would freeze player 1's paddle mid-hold.
struct HeldKeys {
    /// Frames left before the key counts as released; 0 = not held.
    countdown: [u32; 4], // indexed by `HeldKey as usize`
}

impl HeldKeys {
    fn new() -> Self {
        HeldKeys { countdown: [0; 4] }
    }

    /// Record a press: keep the key held for another `HOLD_FRAMES`.
    fn press(&mut self, key: HeldKey) {
        self.countdown[key as usize] = HOLD_FRAMES;
    }

    fn is_held(&self, key: HeldKey) -> bool {
        self.countdown[key as usize] > 0
    }

    /// Age every held key by one frame; called once per timer tick.
    fn tick(&mut self) {
        for frames in &mut self.countdown {
            *frames = frames.saturating_sub(1);
        }
    }
}

/// Fill a rectangle in the back buffer, clipping it to the screen so we
/// never index out of bounds.
fn fill_rect(
    buf: &mut [BltPixel],
    screen: (i32, i32),
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: BltPixel,
) {
    let (sw, sh) = screen;
    let x0 = x.clamp(0, sw) as usize;
    let y0 = y.clamp(0, sh) as usize;
    let x1 = (x + w as i32).clamp(0, sw) as usize;
    let y1 = (y + h as i32).clamp(0, sh) as usize;
    for row in y0..y1 {
        let start = row * sw as usize + x0;
        buf[start..start + (x1 - x0)].fill(color);
    }
}

/// Draw one bitmap-font digit (0-9) with its top-left corner at (x, y).
fn draw_digit(
    buf: &mut [BltPixel],
    screen: (i32, i32),
    x: i32,
    y: i32,
    digit: u32,
    color: BltPixel,
) {
    let glyph = DIGITS[digit as usize];
    for row in 0..5 {
        for col in 0..3 {
            // Bit 14 is the top-left pixel; bits fill rows left to right.
            if glyph & (1 << (14 - row * 3 - col)) != 0 {
                fill_rect(
                    buf,
                    screen,
                    x + col * SCORE_SCALE,
                    y + row * SCORE_SCALE,
                    SCORE_SCALE as u32,
                    SCORE_SCALE as u32,
                    color,
                );
            }
        }
    }
}

/// Draw `n` horizontally centered on `center_x`, top edge at `y`.
fn draw_score(
    buf: &mut [BltPixel],
    screen: (i32, i32),
    center_x: i32,
    y: i32,
    n: u32,
    color: BltPixel,
) {
    // Digit count (at least one, so 0 renders as "0").
    let mut digits = 1;
    let mut rest = n / 10;
    while rest > 0 {
        digits += 1;
        rest /= 10;
    }
    // Total ink width: all glyphs plus the gaps between them.
    let width = digits * DIGIT_ADVANCE - SCORE_SCALE;
    // Start at the last digit's left edge and walk leftwards, peeling
    // the least significant digit first — no buffer needed.
    let mut x = center_x + width / 2 - DIGIT_W;
    let mut n = n;
    loop {
        draw_digit(buf, screen, x, y, n % 10, color);
        n /= 10;
        if n == 0 {
            break;
        }
        x -= DIGIT_ADVANCE;
    }
}

fn game_loop(input: &mut Input, gop: &mut GraphicsOutput, events: &[Event; 2]) -> Result {
    let (w, h) = gop.current_mode_info().resolution();
    let screen = (w as i32, h as i32);
    let (w, h) = screen;

    // Back buffer: each frame is composed here and published with a
    // single blit, so the display never shows a half-drawn frame.
    let mut back = vec![BACKGROUND; (w * h) as usize];

    let mut left = Paddle {
        x: 24,
        y: h / 2 - PADDLE_H as i32 / 2,
    };
    let mut right = Paddle {
        x: w - 24 - PADDLE_W as i32,
        y: h / 2 - PADDLE_H as i32 / 2,
    };
    let mut ball = Ball {
        x: w as f64 / 2.0,
        y: h as f64 / 2.0,
        dx: BALL_SPEED,
        dy: BALL_SPEED,
    };
    let mut held = HeldKeys::new();
    let mut left_score = 0;
    let mut right_score = 0;

    loop {
        // Sleep until a key arrives or the frame timer fires. Keys are
        // handled the moment they happen instead of at the next frame
        // boundary, and the timer paces the game at a fixed rate.
        let tick = boot::wait_for_event(events).discard_errdata()? == TIMER_EVENT;

        // Drain everything queued since the last wake. Each press (or
        // auto-repeat) refreshes the key's countdown in the held-key
        // array; whether it is *still* held is decided by the countdown
        // expiring on ticks, since UEFI never reports key releases.
        while let Some(key) = input.read_key()? {
            match key {
                Key::Special(ScanCode::ESCAPE) => return Ok(()),
                Key::Special(ScanCode::UP) => held.press(HeldKey::Up),
                Key::Special(ScanCode::DOWN) => held.press(HeldKey::Down),
                Key::Printable(c) if c == 'w' || c == 'W' => held.press(HeldKey::W),
                Key::Printable(c) if c == 's' || c == 'S' => held.press(HeldKey::S),
                _ => {}
            }
        }

        // Paddles and ball advance only on timer ticks, so their speeds
        // are tied to the frame rate, not to how often keys arrive.
        if tick {
            held.tick();

            if held.is_held(HeldKey::W) {
                left.y -= PADDLE_SPEED;
            }
            if held.is_held(HeldKey::S) {
                left.y += PADDLE_SPEED;
            }
            if held.is_held(HeldKey::Up) {
                right.y -= PADDLE_SPEED;
            }
            if held.is_held(HeldKey::Down) {
                right.y += PADDLE_SPEED;
            }
            left.y = left.y.clamp(0, h - PADDLE_H as i32);
            right.y = right.y.clamp(0, h - PADDLE_H as i32);

            ball.x += ball.dx;
            ball.y += ball.dy;

            // Bounce off top and bottom.
            if ball.y < 0.0 {
                ball.y = 0.0;
                ball.dy = -ball.dy;
            } else if ball.y + BALL_SIZE as f64 > h as f64 {
                ball.y = h as f64 - BALL_SIZE as f64;
                ball.dy = -ball.dy;
            }

            // Bounce off paddles.
            let (top, bottom) = (ball.y, ball.y + BALL_SIZE as f64);
            let overlaps = |py: i32| bottom >= py as f64 && top <= py as f64 + PADDLE_H as f64;

            if ball.dx < 0.0
                && ball.x <= left.x as f64 + PADDLE_W as f64
                && ball.x + BALL_SIZE as f64 >= left.x as f64
                && overlaps(left.y)
            {
                ball.x = left.x as f64 + PADDLE_W as f64;
                ball.dx = -ball.dx;
            }
            if ball.dx > 0.0
                && ball.x + BALL_SIZE as f64 >= right.x as f64
                && ball.x <= right.x as f64 + PADDLE_W as f64
                && overlaps(right.y)
            {
                ball.x = right.x as f64 - BALL_SIZE as f64;
                ball.dx = -ball.dx;
            }

            // Missed: the other side scores, and the ball resets to the
            // middle, served towards the player who just scored.
            if ball.x + (BALL_SIZE as f64) < 0.0 {
                right_score += 1;
                ball.reset_to_center(w, h);
            } else if ball.x > w as f64 {
                left_score += 1;
                ball.reset_to_center(w, h);
            }
        }

        // Compose the frame in the back buffer: clear, dashed center
        // line, scores, paddles, ball.
        back.fill(BACKGROUND);
        let mut y = 0;
        while y < h {
            fill_rect(&mut back, screen, w / 2 - 2, y, 4, 16, CENTER_LINE);
            y += 32;
        }
        // Scores, centered above each player's half in their color.
        draw_score(&mut back, screen, w / 4, SCORE_Y, left_score, LEFT_COLOR);
        draw_score(&mut back, screen, w * 3 / 4, SCORE_Y, right_score, RIGHT_COLOR);
        fill_rect(
            &mut back,
            screen,
            left.x,
            left.y,
            PADDLE_W,
            PADDLE_H,
            LEFT_COLOR,
        );
        fill_rect(
            &mut back,
            screen,
            right.x,
            right.y,
            PADDLE_W,
            PADDLE_H,
            RIGHT_COLOR,
        );
        fill_rect(
            &mut back,
            screen,
            ball.x as i32,
            ball.y as i32,
            BALL_SIZE,
            BALL_SIZE,
            BALL_COLOR,
        );

        // Publish the finished frame with one blit: the screen only ever
        // moves from one complete frame to the next.
        gop.blt(BltOp::BufferToVideo {
            buffer: &back[..],
            src: BltRegion::Full,
            dest: (0, 0),
            dims: (w as usize, h as usize),
        })?;
    }
}

fn app(input: &mut Input, gop: &mut GraphicsOutput) -> Result {
    let key_event = input.wait_for_key_event()?;

    // Periodic timer event: wakes the game loop once per frame.
    //
    // SAFETY: the event has no notification callback, so the
    // exit-boot-services hazards of `create_event` do not apply.
    let timer_event = unsafe {
        boot::create_event(boot::EventType::TIMER, boot::Tpl::APPLICATION, None, None)?
    };
    boot::set_timer(&timer_event, boot::TimerTrigger::Periodic(FRAME_PERIOD))?;

    let events = [key_event, timer_event];
    let result = game_loop(input, gop, &events);

    // The key event belongs to the console driver and must not be closed.
    // The timer is ours, and being periodic it would keep firing after we
    // exit if left open.
    let [_, timer] = events;
    let _ = boot::close_event(timer);
    result
}

#[entry]
fn main() -> Status {
    system::with_stdin(|input| {
        let gop_handle = boot::get_handle_for_protocol::<GraphicsOutput>()?;
        let mut gop = boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle)?;
        app(input, &mut gop)
    })
    .status()
}