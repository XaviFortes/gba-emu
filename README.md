# GBA Emulator (Rust)

A Game Boy Advance emulator prototype written in Rust.

## Requirements

- Rust toolchain (stable)
- Linux/macOS/Windows
- Optional audio backend dependencies only if you enable `audio` feature

## Build

```bash
cargo build
```

Release build:

```bash
cargo build --release
```

## Run Commands

Windowed mode (default):

```bash
cargo run -- --rom <path-to-rom.gba>
```

Windowed mode with BIOS:

```bash
cargo run -- --rom <path-to-rom.gba> --bios <path-to-bios.bin>
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

## CLI Parameters

- `--rom <path>`: Required. ROM file to load.
- `--bios <path>`: Optional BIOS file.
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
