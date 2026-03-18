use std::env;
use std::process::ExitCode;
use std::time::Instant;

use gba_emu::emulator::{
    BUTTON_A, BUTTON_B, BUTTON_DOWN, BUTTON_L, BUTTON_LEFT, BUTTON_R, BUTTON_RIGHT,
    BUTTON_SELECT, BUTTON_START, BUTTON_UP, DebugSnapshot, Gba, SCREEN_HEIGHT, SCREEN_WIDTH,
};
use minifb::{Key, Scale, Window, WindowOptions};

fn print_usage(bin: &str) {
    eprintln!(
        "Usage: {bin} --rom <path> [--bios <path>] [--frames <n>] [--debug-interval <frames>] [--stuck-threshold <frames>] [--bios-watchdog <frames>] [--trace-branches]"
    );
}

#[derive(Debug, Clone, Copy)]
struct DebugOptions {
    interval_frames: Option<u32>,
    stuck_threshold: Option<u32>,
    bios_watchdog_frames: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct ProgressState {
    last_pc: u32,
    same_pc_frames: u32,
}

fn is_probably_executable(pc: u32) -> bool {
    (0x0000_0000..0x0000_4000).contains(&pc)
        || (0x0200_0000..0x0300_0000).contains(&pc)
        || (0x0300_0000..0x0400_0000).contains(&pc)
        || (0x0800_0000..0x0E00_0000).contains(&pc)
}

fn log_snapshot(prefix: &str, frame: u32, snap: DebugSnapshot) {
    println!(
        "[{prefix}] frame={frame} pc=0x{:08X} cpsr=0x{:08X} r0=0x{:08X} r1=0x{:08X} r2=0x{:08X} r3=0x{:08X} r4=0x{:08X} r7=0x{:08X} sp=0x{:08X} lr=0x{:08X} cycles={} dispcnt=0x{:04X} vcount={} ime=0x{:04X} ie=0x{:04X} if=0x{:04X} handoff=0x{:02X} bios_irq_flags=0x{:04X} irq_vec=0x{:08X} irq_check=0x{:04X} bios_steps={} rom_steps={} bg0cnt=0x{:04X} hofs=0x{:04X} vofs=0x{:04X} pal0=0x{:04X} pal1=0x{:04X} vram0=0x{:04X} vram3800=0x{:04X}",
        snap.pc,
        snap.cpsr,
        snap.r0,
        snap.r1,
        snap.r2,
        snap.r3,
        snap.r4,
        snap.r7,
        snap.sp,
        snap.lr,
        snap.cycles,
        snap.dispcnt,
        snap.vcount,
        snap.ime,
        snap.ie,
        snap.iflags,
        snap.handoff_7ff0,
        snap.bios_irq_flags,
        snap.irq_vec,
        snap.irq_check,
        snap.frame_bios_steps,
        snap.frame_rom_steps,
        snap.bg0cnt,
        snap.bg0hofs,
        snap.bg0vofs,
        snap.palette0,
        snap.palette1,
        snap.vram0,
        snap.vram3800
    );
}

fn update_progress(prefix: &str, frame: u32, snap: DebugSnapshot, debug: DebugOptions, state: &mut ProgressState) {
    let signed_delta = (snap.pc as i64) - (state.last_pc as i64);
    let abs_delta = signed_delta.unsigned_abs();

    if snap.pc == state.last_pc {
        state.same_pc_frames = state.same_pc_frames.saturating_add(1);
    } else {
        state.same_pc_frames = 0;
    }

    if !is_probably_executable(snap.pc) {
        println!(
            "[{prefix}] anomaly frame={frame} pc=0x{:08X} (non-exec region) prev=0x{:08X} delta=0x{:08X}",
            snap.pc, state.last_pc, snap.pc.wrapping_sub(state.last_pc)
        );
    }

    if abs_delta > 0x0100_0000 {
        println!(
            "[{prefix}] anomaly frame={frame} large-pc-jump prev=0x{:08X} pc=0x{:08X} delta=0x{:08X}",
            state.last_pc, snap.pc, snap.pc.wrapping_sub(state.last_pc)
        );
    }

    state.last_pc = snap.pc;

    if let Some(interval) = debug.interval_frames {
        if interval != 0 && frame % interval == 0 {
            log_snapshot(prefix, frame, snap);
        }
    }

    if let Some(threshold) = debug.stuck_threshold {
        if threshold != 0 && state.same_pc_frames == threshold {
            println!(
                "[{prefix}] potential-stuck frame={frame} pc=0x{:08X} same_pc_frames={} (continuing)",
                snap.pc, state.same_pc_frames
            );
            log_snapshot(prefix, frame, snap);
        }
    }
}

fn key_mask_from_window(window: &Window) -> u16 {
    let mut mask = 0u16;

    if window.is_key_down(Key::Z) {
        mask |= BUTTON_A;
    }
    if window.is_key_down(Key::X) {
        mask |= BUTTON_B;
    }
    if window.is_key_down(Key::Backspace) {
        mask |= BUTTON_SELECT;
    }
    if window.is_key_down(Key::Enter) {
        mask |= BUTTON_START;
    }
    if window.is_key_down(Key::Right) {
        mask |= BUTTON_RIGHT;
    }
    if window.is_key_down(Key::Left) {
        mask |= BUTTON_LEFT;
    }
    if window.is_key_down(Key::Up) {
        mask |= BUTTON_UP;
    }
    if window.is_key_down(Key::Down) {
        mask |= BUTTON_DOWN;
    }
    if window.is_key_down(Key::S) {
        mask |= BUTTON_R;
    }
    if window.is_key_down(Key::A) {
        mask |= BUTTON_L;
    }

    mask
}

fn run_windowed(gba: &mut Gba, debug: DebugOptions) -> Result<(), String> {
    let mut window = Window::new(
        "GBA Emulator (Rust)",
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        WindowOptions {
            scale: Scale::X4,
            ..WindowOptions::default()
        },
    )
    .map_err(|err| format!("Failed to create window: {err}"))?;

    window.set_target_fps(60);

    let mut frame = 0u32;
    let mut progress = ProgressState {
        last_pc: gba.cpu.pc(),
        same_pc_frames: 0,
    };
    let mut bios_frames = 0u32;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        frame = frame.wrapping_add(1);
        let held = key_mask_from_window(&window);
        gba.set_input_held_mask(held);
        gba.run_frame();

        let snap = gba.debug_snapshot();
        update_progress("window", frame, snap, debug, &mut progress);

        if snap.pc < 0x0000_4000 {
            bios_frames = bios_frames.saturating_add(1);
        } else {
            bios_frames = 0;
        }

        if let Some(limit) = debug.bios_watchdog_frames {
            if limit != 0 && bios_frames == limit {
                println!(
                    "[window] bios-watchdog triggered at frame={frame}; switching to no-BIOS ROM boot"
                );
                gba.force_boot_to_rom_without_bios();
            }
        }

        if gba.take_frame_ready() {
            if let Err(err) = window.update_with_buffer(gba.framebuffer(), SCREEN_WIDTH, SCREEN_HEIGHT) {
                return Err(format!("Failed to draw frame: {err}"));
            }
        } else {
            window.update();
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    let mut args = env::args();
    let bin = args.next().unwrap_or_else(|| "gba-emu".to_string());

    let mut rom_path: Option<String> = None;
    let mut bios_path: Option<String> = None;
    let mut frames: Option<u32> = None;
    let mut debug_interval: Option<u32> = None;
    let mut stuck_threshold: Option<u32> = None;
    let mut bios_watchdog: Option<u32> = None;
    let mut trace_branches = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rom" => rom_path = args.next(),
            "--bios" => bios_path = args.next(),
            "--frames" => {
                if let Some(value) = args.next() {
                    match value.parse::<u32>() {
                        Ok(parsed) => frames = Some(parsed),
                        Err(_) => {
                            eprintln!("Invalid --frames value: {value}");
                            print_usage(&bin);
                            return ExitCode::from(2);
                        }
                    }
                }
            }
            "--debug-interval" => {
                if let Some(value) = args.next() {
                    match value.parse::<u32>() {
                        Ok(parsed) => debug_interval = Some(parsed),
                        Err(_) => {
                            eprintln!("Invalid --debug-interval value: {value}");
                            print_usage(&bin);
                            return ExitCode::from(2);
                        }
                    }
                }
            }
            "--stuck-threshold" => {
                if let Some(value) = args.next() {
                    match value.parse::<u32>() {
                        Ok(parsed) => stuck_threshold = Some(parsed),
                        Err(_) => {
                            eprintln!("Invalid --stuck-threshold value: {value}");
                            print_usage(&bin);
                            return ExitCode::from(2);
                        }
                    }
                }
            }
            "--bios-watchdog" => {
                if let Some(value) = args.next() {
                    match value.parse::<u32>() {
                        Ok(parsed) => bios_watchdog = Some(parsed),
                        Err(_) => {
                            eprintln!("Invalid --bios-watchdog value: {value}");
                            print_usage(&bin);
                            return ExitCode::from(2);
                        }
                    }
                }
            }
            "--trace-branches" => {
                trace_branches = true;
            }
            "-h" | "--help" => {
                print_usage(&bin);
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("Unknown argument: {arg}");
                print_usage(&bin);
                return ExitCode::from(2);
            }
        }
    }

    let Some(rom) = rom_path else {
        print_usage(&bin);
        return ExitCode::from(2);
    };

    let mut gba = Gba::new();
    let bios_provided = bios_path.is_some();

    if let Some(bios) = bios_path {
        println!("[boot] loading BIOS: {}", bios);
        if let Err(err) = gba.load_bios(bios) {
            eprintln!("Failed to load BIOS: {err}");
            return ExitCode::from(1);
        }
        println!("[boot] BIOS loaded");
    }

    println!("[boot] loading ROM: {}", rom);
    if let Err(err) = gba.load_rom(rom) {
        eprintln!("Failed to load ROM: {err}");
        return ExitCode::from(1);
    }
    println!("[boot] ROM loaded");

    gba.reset();
    if bios_provided {
        println!("[boot] entering ARM Supervisor mode via BIOS reset vector");
    } else {
        println!("[boot] entering ARM System mode via direct ROM boot");
    }
    gba.set_trace_branches(trace_branches);

    #[cfg(not(feature = "audio"))]
    println!("[audio] audio backend disabled at compile-time (build with --features audio)");

    // HACK VISUAL: Encendemos la pantalla y ponemos el fondo en un color chillón (magenta)
    // para que sepas que la ventana está refrescando y la memoria gráfica existe.
    // gba.bus.write16(0x0400_0000, 0x0403); // DISPCNT = Modo 3 | Activar BG2
    // for i in 0..(240 * 160) {
        // gba.bus.write16(0x0600_0000 + (i * 2), 0x7C1F); // Rellenar VRAM con Magenta
    // }

    let debug = DebugOptions {
        interval_frames: debug_interval,
        stuck_threshold,
        bios_watchdog_frames: bios_watchdog,
    };

    let debug = if frames.is_none() && bios_provided && debug.bios_watchdog_frames.is_none() {
        DebugOptions {
            bios_watchdog_frames: Some(240),
            ..debug
        }
    } else {
        debug
    };

    if let Some(frame_count) = frames {
        let started = Instant::now();
        let mut progress = ProgressState {
            last_pc: gba.cpu.pc(),
            same_pc_frames: 0,
        };
        let mut bios_frames = 0u32;

        for frame in 1..=frame_count {
            gba.run_frame_headless();
            let snap = gba.debug_snapshot();
            update_progress("headless", frame, snap, debug, &mut progress);

            if snap.pc < 0x0000_4000 {
                bios_frames = bios_frames.saturating_add(1);
            } else {
                bios_frames = 0;
            }

            if let Some(limit) = debug.bios_watchdog_frames {
                if limit != 0 && bios_frames == limit {
                    println!(
                        "[headless] bios-watchdog triggered at frame={frame}; switching to no-BIOS ROM boot"
                    );
                    gba.force_boot_to_rom_without_bios();
                }
            }
        }
        let elapsed = started.elapsed();
        let final_snap = gba.debug_snapshot();
        log_snapshot("headless-final", frame_count, final_snap);
        println!(
            "Executed {frame_count} frame(s), CPU cycles: {}, PC: 0x{:08X}, elapsed: {:.2?}",
            gba.cpu.cycles,
            gba.cpu.pc(),
            elapsed
        );
        return ExitCode::SUCCESS;
    }

    if let Err(err) = run_windowed(&mut gba, debug) {
        eprintln!("{err}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
