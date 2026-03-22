# GBA Emulator (Rust)

A Game Boy Advance emulator prototype written in Rust.

## Requirements

- Rust toolchain (stable)
- Linux/macOS/Windows
- Optional audio backend dependencies only if you enable `audio` feature

### Audio Prerequisites (Linux)

If you build with `--features audio`, install ALSA development headers first:

```bash
sudo apt-get update && sudo apt-get install -y libasound2-dev pkg-config
```

Fedora equivalent:

```bash
sudo dnf install -y alsa-lib-devel pkgconf-pkg-config
```

PipeWire note:

- Even on PipeWire-based desktops, this backend typically goes through ALSA compatibility.
- You still need `libasound2-dev` to compile.
- At runtime, audio is usually routed to PipeWire automatically when PipeWire ALSA/Pulse compatibility is installed.
- If needed, install PipeWire compatibility packages for your distro.

## Build

```bash
cargo build
```

Release build:

```bash
cargo build --release
```

## Run Commands

Launcher mode (default when no ROM is passed):

```bash
cargo run --release --
```

Windowed mode with direct ROM boot:

```bash
cargo run -- --rom <path-to-rom.gba>
```

For best performance (recommended for real gameplay), use release mode:

```bash
cargo run --release -- --rom <path-to-rom.gba>
```

Windowed mode with BIOS:

```bash
cargo run -- --rom <path-to-rom.gba> --bios <path-to-bios.bin>
```

Open launcher explicitly (Switch-style game browser):

```bash
cargo run -- --launcher
```

Open launcher with a custom ROM library directory:

```bash
cargo run -- --launcher --roms-dir ./roms
```

Headless mode for N frames:

```bash
cargo run -- --rom <path-to-rom.gba> --frames <N>
```

Headless with BIOS and debug logs every 60 frames:

```bash
cargo run -- --rom <path-to-rom.gba> --bios <path-to-bios.bin> --frames 600 --debug-interval 60
```

Run with branch tracing enabled:

```bash
cargo run -- --rom <path-to-rom.gba> --trace-branches
```

Enable audio feature:

```bash
cargo run --features audio -- --rom <path-to-rom.gba>
```

Enable audio feature in release mode:

```bash
cargo run --release --features audio -- --rom <path-to-rom.gba>
```

## CLI Parameters

- `--rom <path>`: Optional. ROM file to load directly (skips launcher).
- `--bios <path>`: Optional BIOS file.
- `--launcher`: Force launcher UI mode.
- `--roms-dir <dir>`: Directory scanned for `.gba` files in launcher mode.
- `--scale <1..6>`: Window scale preset (`1=X1`, `2=X2`, `3=X4`, `4=X8`, `5=X16`, `6=X32`).
- `--frames <N>`: Run headless for exactly `N` frames.
- `--debug-interval <frames>`: Print full debug snapshot every N frames.
- `--stuck-threshold <frames>`: Emit warning when PC stays unchanged for N frames.
- `--bios-watchdog <frames>`: If BIOS execution appears stuck for N frames, force ROM boot handoff.
- `--trace-branches`: Enable CPU branch tracing logs.
- `-h`, `--help`: Print usage.

## Controls (Windowed)

- `Z`: A
- `X`: B
- `Backspace`: Select
- `Enter`: Start
- Arrow keys: D-pad
- `A`: L
- `S`: R
- `Esc`: Exit

## Launcher Controls

- `Left` / `Right`: Select game card
- `Up` / `Down`: Select a setting row
- `A` / `D`: Change setting value
- `R`: Rescan ROM directory
- `Enter`: Launch selected game
- `Esc`: Exit launcher

Launcher settings currently include:

- Window scale preset
- BIOS on/off toggle (auto-detected from `--bios` or `gba_bios.bin`)
- Audio output mode (`Default` / `Muted` UI toggle)

## Project Structure

```text
src/
  app/
    cli.rs        # CLI argument parsing and usage
    debug.rs      # progress/anomaly/snapshot logging
    runner.rs     # boot + window/headless runtime loops
  emulator/
    core/
      bus.rs      # memory map, MMIO, DMA, timers, interrupts
      cpu.rs      # ARM/THUMB core and execution
    video/
      ppu.rs      # scanline rendering and vblank behavior
    audio/
      apu.rs      # optional audio backend integration
    input/
      input.rs    # key mask translation to KEYINPUT
    timing/
      timers.rs   # timer ticking adapter
    mod.rs        # Gba orchestrator and public emulator API
  lib.rs
  main.rs
```

## Useful Dev Commands

```bash
cargo check
cargo test
cargo fmt
```
