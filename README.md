# UefiGames

Three small arcade games that run **directly on UEFI firmware** — no
operating system, no bootloader payload, no `std`. Your machine's firmware
loads them the same way it loads a bootloader, and they take over the
display, keyboard and mouse to run at ~60 FPS. A fun demonstration that
UEFI is a perfectly usable (if quirky) platform.

```
┌──────────────────────────────────────────────────────────────────────┐
│                                                                      │
│   PONG                    BRICKSHOT                    KALLIRS       │
│   ┌──┐                   ▓▓▓▓▓▓▓  ▓▓▓▓▓▓▓              ______        │
│   │  │   ●               ▒▒▒▒▒▒▒  ▒▒▒▒▒▒▒             /|_||_\`.__    │
│   │  │                   ░░░░░░░  ░░░░░░░            (   _    _ _\   │
│   └──┐                   ●                          =`-(_)--(_)-'    │
│      │                   ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄                             │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

## The games

| Game | Genre | Players | Input |
|---|---|---|---|
| [**Pong**](pong/) | Classic two-player tennis | 2 | Keyboard |
| [**BrickShot**](brickit/) | Breakout with power-ups | 1 | Mouse (or keyboard) |
| [**Kallirs**](kallirs/) | Endless-road car dodger | 1 | Keyboard |

### Pong

Classic two-player tennis. First to miss, loses the point. The ball is
served towards the player who just scored.

| Key | Action |
|---|---|
| <kbd>W</kbd> / <kbd>S</kbd> | Left paddle up / down |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Right paddle up / down |
| <kbd>Esc</kbd> | Quit |

### BrickShot

A mouse-controlled breakout game. Destroy the wall, catch power-ups,
don't drop the ball. Three difficulty presets, multiball up to 8 balls,
and a bounce angle that depends on *where* the ball hits the paddle —
edge hits send it flying sideways at up to 60°.

→ Full manual: [`brickit/README.md`](brickit/README.md)

### Kallirs

A tiny arcade car game. Steer a car down an endless country road,
collect shamrocks, dodge rolling boulders and construction sites — and
don't drift off the asphalt, you only have one life. Each press of
<kbd>←</kbd>/<kbd>→</kbd> steps the heading one stage (up to ±45°), and
the car keeps its heading between presses.

→ Full manual: [`kallirs/README.md`](kallirs/README.md)

## Quick start

Each game is a standalone crate in this workspace with its own `run.sh`
that builds it, packs it into a small FAT disk image at the standard UEFI
boot path (`efi/boot/bootx64.efi`), and boots it in QEMU with OVMF
firmware:

```sh
cd pong    # or brickit, or kallirs
./run.sh
```

### Prerequisites

| Tool | Notes |
|---|---|
| Rust 1.85+ | edition 2024; `rustup target add x86_64-unknown-uefi` |
| `qemu-system-x86_64` | emulator |
| OVMF (edk2) | UEFI firmware for QEMU; `run.sh` defaults to `/usr/share/edk2/x64/OVMF.4m.fd` (Arch path) — override with `OVMF=/usr/share/OVMF/OVMF_CODE.fd ./run.sh` on Debian/Ubuntu |
| mtools (`mformat`, `mmd`, `mcopy`) | to build the small FAT disk image |

### Real hardware

Copy the built `.efi` binary to your EFI system partition as
`efi/boot/bootx64.efi` (or add it to the boot menu under any name), then
boot it like any UEFI application. The games only read input and write
to the framebuffer — they never touch your disks. They need a
GOP-capable firmware (which every x86_64 machine from the last decade
has) and a display of at least 640×400 (BrickShot: 700×320).

## How it works

All three games are `#![no_std]` / `#![no_main]` UEFI applications built
on [uefi-rs](https://github.com/rust-osdev/uefi-rs) (`uefi` crate 0.41,
which also provides the global allocator and panic handler). They share
the same architecture:

- **Event loop** — a periodic ~16 ms UEFI timer event paces the game at
  ~60 FPS; `wait_for_event` blocks on {keyboard, timer} instead of
  busy-waiting with `stall()`.
- **Double buffering** — every frame is composed in a software back
  buffer and published with a single blit to the GOP framebuffer: no
  tearing, and it works on every GOP implementation.
- **Held-key emulation** — UEFI only reports key *presses* (no key-up
  events, no "is this key down?" query), so a press counts as held for
  ~30 frames (~500 ms) to bridge the keyboard's auto-repeat delay. This
  is also what lets both Pong paddles move at the same time.
- **No `libm`** — `sin`/`cos`/`floor`/`ceil` are hand-rolled (Taylor
  series with quadrant reduction, max error ~3e-5), since `no_std`
  `core` has none of these.
- **No entropy source** — the xorshift32 RNG is seeded from the screen
  resolution, so runs differ between machines and video modes.
- **Clean exit** — the timer event is closed and the text console is
  restored before returning to the firmware.

## License

[MIT](LICENSE) © 2026 Adrenocrom
