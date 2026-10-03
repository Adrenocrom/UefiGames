# BrickShot

> A mouse-controlled breakout game that runs **directly on UEFI firmware** —
> no operating system, no bootloader, no `std`. The firmware *is* the runtime.

BrickShot is a `#![no_std]` Rust application built for the `x86_64-unknown-uefi`
target. The PC's firmware loads it the same way it loads a bootloader, and it
takes over the display, mouse and keyboard to run a complete breakout game at
~60 FPS — a fun demonstration that UEFI is a perfectly usable (if quirky)
platform.

```
┌──────────────────────────────────────────────────┐
│ LIVES 5                                 SCORE 40 │
│                                                  │
│  ▓▓▓▓▓▓▓  ▓▓▓▓▓▓▓  ▓▓▓▓▓▓▓  ▓▓▓▓▓▓▓  ▓▓▓▓▓▓▓     │
│  ▒▒▒▒▒▒▒  ▒▒▒▒▒▒▒  ▒▒▒▒▒▒▒  ▒▒▒▒▒▒▒  ▒▒▒▒▒▒▒     │
│  ░░░░░░░  ░░░░░░░  ░░░░░░░  ░░░░░░░  ░░░░░░░     │
│                                                  │
│                       ●                [M]       │
│                                                  │
│                ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄                   │
└──────────────────────────────────────────────────┘
```
*(approximate — the real thing has colors)*

## Features

- **No OS required** — boots as a UEFI application; the only dependency is firmware every x86_64 machine already has
- **Mouse-controlled paddle**, supporting both UEFI pointer protocols automatically:
  - `AbsolutePointer` (USB tablets, touchscreens) — preferred
  - `Pointer` (PS/2 mice) — relative deltas accumulated into a virtual cursor
- **Keyboard fallback** (arrow keys) when no pointer device is present
- Three difficulty presets, falling power-ups, multiball (up to 8 balls)
- ~60 FPS software rendering into a back buffer, blitted once per frame (no tearing)
- Hand-rolled `sin`/`cos`/`floor`/`ceil` — no `libm`, no `std`
- Clean exit: restores the text console and says thanks

## Quick start

### Prerequisites

| Tool | Notes |
|---|---|
| Rust 1.85+ | edition 2024; `rustup target add x86_64-unknown-uefi` |
| `qemu-system-x86_64` | emulator |
| OVMF (edk2) | UEFI firmware for QEMU; `run.sh` defaults to `/usr/share/edk2/x64/OVMF.4m.fd` (Arch path) — override with `OVMF=/usr/share/OVMF/OVMF_CODE.fd ./run.sh` on Debian/Ubuntu |
| mtools + dd | to build the small FAT disk image |

### Run it

```sh
./run.sh
```

The script builds the app, creates a 2 MB FAT image containing
`efi/boot/bootx64.efi`, and boots it in QEMU with OVMF and a USB tablet
(so the absolute-pointer code path is exercised).

Manual build, if you just want the binary:

```sh
cargo build --release --target x86_64-unknown-uefi
# → target/x86_64-unknown-uefi/release/brickit.efi
```

## How to play

| Input | Action |
|---|---|
| Move mouse | Move the paddle (it tracks the cursor's x position) |
| Left click / <kbd>Space</kbd> | Launch the ball · confirm menu entry · continue after level clear / game over |
| <kbd>←</kbd> / <kbd>→</kbd> | Move the paddle (keyboard fallback) |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Select menu entry |
| <kbd>Esc</kbd> | In game: back to the menu · in the menu: quit |

The bounce angle depends on *where* the ball hits the paddle — edge hits send
it flying sideways at up to 60°. A ball that comes in from the side bounces
off the paddle's side instead of climbing over it.

### Difficulty

| Mode | Lives | Paddle width |
|---|---|---|
| SIMPLE | 10 | 150 px |
| NORMAL | 5 | 110 px |
| HARD | 3 | 80 px |

### Power-ups

Destroyed bricks have a 25% chance to drop a collectible:

| Drop | Effect |
|---|---|
| **G** (green) | Paddle grows by 30 px (max 220 px) |
| **S** (red) | Paddle shrinks by 30 px (min 60 px) |
| **M** (blue) | Multiball: two extra balls split off at ±25° (max 8) |

Paddle-width effects reset when you lose a ball. Clear the whole wall to start
the next round — score and lives carry over. Ten points per brick.

## Running on real hardware

1. Build the binary (see above).
2. Format a USB stick with FAT32.
3. Copy `brickit.efi` to `EFI/boot/bootx64.efi` on the stick.
4. Boot the machine from USB (UEFI mode) and pick the stick in the boot menu.

The game only reads input and writes to the framebuffer — it never touches
your disks. It needs a GOP-capable firmware (which every x86_64 machine from
the last decade has) and a display of at least **700×320** — the brick grid
alone is 694 px wide, and on anything narrower the outer columns would be
unreachable. You can also launch it from a UEFI shell.

## How it works

| Module | Responsibility |
|---|---|
| `src/main.rs` | Entry point: opens the GOP, picks a usable video mode, opens pointer devices, runs the event loop (keyboard + 16 ms frame timer), blits the frame buffer |
| `src/game.rs` | Game state machine (menu → ready → playing → level clear / game over), physics, power-ups, scoring, menu hit-testing |
| `src/draw.rs` | Software back buffer of `BltPixel`s, rect/circle/text primitives, 8×8 bitmap font |
| `src/math.rs` | `sin`/`cos` via Taylor series with quadrant reduction, plus `floor`/`ceil` — `no_std` `core` has none of these |

### UEFI-specific design notes

- **Frame pacing**: a periodic UEFI timer event (~16 ms) wakes the main loop; `wait_for_event` blocks on {keyboard, timer}. Busy-waiting with `stall()` would burn CPU on top of the per-frame work.
- **Two pointer protocols**: firmware exposes either `AbsolutePointer` (tablets/touchscreens, absolute coordinates) or `Pointer` (PS/2, relative-only). Both are polled; relative deltas are accumulated into a virtual cursor, which is drawn as a small dot in the menu so a PS/2 mouse isn't guesswork.
- **No key-release events**: UEFI only reports presses. A press counts as "held" for 30 frames (~500 ms) to bridge the keyboard's auto-repeat delay, so arrow keys move the paddle smoothly.
- **Entropy**: UEFI offers none, so the xorshift32 RNG is seeded from the screen resolution. It only rolls power-up spawns, so quality is irrelevant.
- **No `libm`**: bounce angles and circle rasterization need `sin`/`cos`/`floor`/`ceil`; a Taylor series with quadrant reduction (max error ~3e-5) is plenty and avoids an extra dependency.
- **Rendering**: everything is drawn into a `Vec<BltPixel>` back buffer and blitted once per frame — no tearing, and it works even with `BltOnly` GOP modes.
- **Borrow discipline**: `step_ball` is a free function so the caller can hand it disjoint borrows (`&mut Ball` + `&mut bricks`) and keep using the rest of `self` right after; drops are `Copy` and taken by value for the same reason.
- **Safety**: a single `unsafe` block (creating the plain waitable timer event, which has no notification callback). All drawing primitives clip to the screen; float→int casts saturate.

## Credits

- Built on [uefi-rs](https://github.com/rust-osdev/uefi-rs) (`uefi` crate 0.41, which also provides the global allocator and panic handler).
- 8×8 font: [dhepper/font8x8](https://github.com/dhepper/font8x8) (public domain, based on IBM's public-domain VGA fonts).
