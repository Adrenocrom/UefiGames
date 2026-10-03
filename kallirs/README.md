# Kallirs

```
      ______
     /|_||_\`.__
    (   _    _ _\
    =`-(_)--(_)-'
```

A tiny arcade car game that runs **directly on UEFI firmware** — no operating
system, no bootloader payload, just your machine's firmware and a single `.efi`
binary.

Steer a car down an endless country road. Collect shamrocks, dodge rolling
boulders and construction sites, and don't drift off the asphalt — you only
have one life.

## Gameplay

- The car stays near the bottom of the screen; the world scrolls past.
- Each press of **←** / **→** steps the heading one stage (up to ±45°).
  The car **keeps its heading** between presses — straightening bleeds the
  drift off gradually instead of stopping it dead.
- The road bends, the speed ramps up, and the track is endless.
- Collect **shamrocks** for points. Crash into a boulder or a barrier, or
  leave the road, and the run is over.

### Controls

| Key            | Action                                    |
| -------------- | ----------------------------------------- |
| `←` / `→`      | Steer one stage left / right (max ±45°)   |
| `Space`        | Start a run / retry after a crash          |
| `Esc`          | Back to the title screen / quit from there |

## Quick start

The easiest way to play is in QEMU:

```sh
./run.sh
```

`run.sh` builds the release binary, packs it into a small FAT disk image at
the standard UEFI boot path (`/efi/boot/bootx64.efi`), and boots it in QEMU
with OVMF firmware.

### Requirements

- Rust (stable) with the UEFI target:

  ```sh
  rustup target add x86_64-unknown-uefi
  ```

- `qemu-system-x86_64`
- OVMF firmware — the script looks for `/usr/share/edk2/x64/OVMF.4m.fd`
  (Arch); on Debian/Ubuntu use
  `OVMF=/usr/share/OVMF/OVMF_CODE.fd ./run.sh`
- `mtools` (`mformat`, `mmd`, `mcopy`) for building the FAT image

### Real hardware

Copy `target/x86_64-unknown-uefi/release/kallirs.efi` to your EFI system
partition as `efi/boot/bootx64.efi` (or add it to the boot menu under any
name), then boot it like any UEFI application. The game needs a GOP graphics
output of at least 640×400 and a keyboard.

## How it works

The whole game is a single `#![no_std]` / `#![no_main]` UEFI application:

- **`main.rs`** — UEFI entry point. Opens the graphics output protocol,
  sets up a keyboard event and a periodic ~60 FPS timer event, and runs the
  event loop: input is drained on key presses, physics and rendering happen
  on frame boundaries.
- **`game.rs`** — the phase machine (title / playing / game over), car
  physics, obstacle spawning, collisions and the HUD.
- **`world.rs`** — the endless track. The road is a centerline built from
  fixed-length segments with cosine-eased bends, kept in a small ring buffer
  that refills as the car advances — O(1) lookup per scanline. Also renders
  the meadow around the road and provides the RNG.
- **`draw.rs`** — a software back buffer of `BltPixel`s plus drawing
  primitives (rects, lines, scaled/rotated blits, text) and an 8×8 bitmap
  font. The buffer is blitted to the GOP framebuffer once per frame, which
  avoids tearing and works on every GOP implementation.
- **`assets.rs`** — sprites as readable string maps (one character = one
  pixel, `.` = transparent) plus shape-based drawing for balls and bridges.
- **`math.rs`** — hand-rolled `sin`/`cos`/rounding, since `no_std` `core`
  has no libm; a Taylor series with quadrant reduction is plenty for bounce
  angles and rasterization.

A few implementation notes worth knowing:

- **No heap in the hot path.** Obstacles live in a fixed-size slot array;
  the track in a ring buffer. `alloc` is only used by the `uefi` crate's
  helpers.
- **No entropy source on UEFI**, so the xorshift32 RNG is seeded from the
  screen resolution — runs differ between machines and modes.
- **Bridges** are drawn *over* the car, so they genuinely hide it (and
  anything else) while you pass underneath; their shadow is drawn per
  scanline so it follows the road curve.
- **Boulders** bounce off the road edges, and their spin follows their
  horizontal travel, so they visibly reverse when they bounce.

## Tuning

All gameplay numbers (car speed, steering stages, spawn rates, speed ramp,
bridge spacing, …) are documented constants at the top of `src/game.rs` and
`src/world.rs`. Tweak and rebuild.

## Credits

The 8×8 font in `draw.rs` is the public-domain
[dhepper/font8x8](https://github.com/dhepper/font8x8), based on IBM's
public-domain VGA fonts.