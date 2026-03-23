use std::process::ExitCode;
use std::time::Instant;

use gba_emu::emulator::{
    BUTTON_A, BUTTON_B, BUTTON_DOWN, BUTTON_L, BUTTON_LEFT, BUTTON_R, BUTTON_RIGHT,
    BUTTON_SELECT, BUTTON_START, BUTTON_UP, Gba, SCREEN_HEIGHT, SCREEN_WIDTH,
};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

use super::cli::CliArgs;
use super::debug::{log_snapshot, update_progress, DebugOptions, ProgressState};
use super::ui::{LauncherAudioOutput, LauncherSelection, run_launcher};

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

fn normalize_speed(multiplier: u32) -> u32 {
    if (1..=3).contains(&multiplier) {
        multiplier
    } else {
        1
    }
}

fn run_windowed(
    gba: &mut Gba,
    debug: DebugOptions,
    scale: Scale,
    speed_multiplier: u32,
) -> Result<(), String> {
    let mut window = Window::new(
        "GBA Emulator (Rust)",
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        WindowOptions {
            scale,
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
    let mut speed = normalize_speed(speed_multiplier);
    println!("[speed] window speed x{speed} (Tab cycles x1/x2/x3)");

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if window.is_key_pressed(Key::Tab, KeyRepeat::No) {
            speed = match speed {
                1 => 2,
                2 => 3,
                _ => 1,
            };
            println!("[speed] switched to x{speed}");
        }

        for _ in 0..speed {
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
        }

        if gba.take_frame_ready() {
            if let Err(err) =
                window.update_with_buffer(gba.framebuffer(), SCREEN_WIDTH, SCREEN_HEIGHT)
            {
                return Err(format!("Failed to draw frame: {err}"));
            }
        } else {
            window.update();
        }
    }

    Ok(())
}

fn run_headless(gba: &mut Gba, frame_count: u32, debug: DebugOptions) {
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
}

fn boot_system(gba: &mut Gba, rom_path: &str, bios_path: Option<&str>) -> Result<(), ExitCode> {
    let bios_provided = bios_path.is_some();

    if let Some(bios) = bios_path {
        println!("[boot] loading BIOS: {}", bios);
        if let Err(err) = gba.load_bios(bios) {
            eprintln!("Failed to load BIOS: {err}");
            return Err(ExitCode::from(1));
        }
        println!("[boot] BIOS loaded");
    }

    println!("[boot] loading ROM: {}", rom_path);
    if let Err(err) = gba.load_rom(rom_path) {
        eprintln!("Failed to load ROM: {err}");
        return Err(ExitCode::from(1));
    }
    println!("[boot] ROM loaded");

    gba.reset();
    if bios_provided {
        println!("[boot] entering ARM Supervisor mode via BIOS reset vector");
    } else {
        println!("[boot] entering ARM System mode via direct ROM boot");
    }
    Ok(())
}

fn selection_from_args(args: &CliArgs, audio_backend_info: &str) -> Result<LauncherSelection, ExitCode> {
    if args.launcher || args.rom_path.is_none() {
        let selection = match run_launcher(
            args.roms_dir.as_deref(),
            args.bios_path.as_deref(),
            args.window_scale.unwrap_or(4),
            audio_backend_info,
        ) {
            Ok(Some(selection)) => selection,
            Ok(None) => return Err(ExitCode::SUCCESS),
            Err(err) => {
                eprintln!("{err}");
                return Err(ExitCode::from(1));
            }
        };
        return Ok(selection);
    }

    let Some(rom_path) = args.rom_path.as_ref() else {
        eprintln!("No ROM selected");
        return Err(ExitCode::from(2));
    };

    Ok(LauncherSelection {
        rom_path: rom_path.clone(),
        bios_path: args.bios_path.clone(),
        scale: map_scale(args.window_scale.unwrap_or(4)),
        audio_output: LauncherAudioOutput::Default,
        master_volume: 0.8,
    })
}

fn map_scale(scale: u32) -> Scale {
    match scale {
        1 => Scale::X1,
        2 => Scale::X2,
        3 => Scale::X4,
        4 => Scale::X8,
        5 => Scale::X16,
        6 => Scale::X32,
        _ => Scale::X4,
    }
}

pub fn run(args: CliArgs) -> ExitCode {
    let mut gba = Gba::new();
    let backend_info = gba.audio_backend_info();

    let selection = match selection_from_args(&args, &backend_info) {
        Ok(selection) => selection,
        Err(code) => return code,
    };

    #[cfg(debug_assertions)]
    println!("[perf] debug build detected; use `cargo run --release -- ...` for near real-time emulation");

    if let Err(code) = boot_system(&mut gba, &selection.rom_path, selection.bios_path.as_deref()) {
        return code;
    }

    gba.set_trace_branches(args.trace_branches);

    #[cfg(not(feature = "audio"))]
    println!("[audio] audio backend disabled at compile-time (build with --features audio)");

    let muted = matches!(selection.audio_output, LauncherAudioOutput::Muted);
    gba.set_audio_muted(muted);
    gba.set_audio_master_volume(selection.master_volume);
    if muted {
        println!("[audio] launcher setting: muted");
    }

    let debug = DebugOptions {
        interval_frames: args.debug_interval,
        stuck_threshold: args.stuck_threshold,
        bios_watchdog_frames: args.bios_watchdog,
    };

    let debug = if args.frames.is_none()
        && selection.bios_path.is_some()
        && debug.bios_watchdog_frames.is_none()
    {
        DebugOptions {
            bios_watchdog_frames: Some(240),
            ..debug
        }
    } else {
        debug
    };

    if let Some(frame_count) = args.frames {
        run_headless(&mut gba, frame_count, debug);
        return ExitCode::SUCCESS;
    }

    if let Err(err) = run_windowed(
        &mut gba,
        debug,
        selection.scale,
        args.speed_multiplier.unwrap_or(1),
    ) {
        eprintln!("{err}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
