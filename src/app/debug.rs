use gba_emu::emulator::DebugSnapshot;

#[derive(Debug, Clone, Copy)]
pub struct DebugOptions {
    pub interval_frames: Option<u32>,
    pub stuck_threshold: Option<u32>,
    pub bios_watchdog_frames: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProgressState {
    pub last_pc: u32,
    pub same_pc_frames: u32,
}

fn is_probably_executable(pc: u32) -> bool {
    (0x0000_0000..0x0000_4000).contains(&pc)
        || (0x0200_0000..0x0300_0000).contains(&pc)
        || (0x0300_0000..0x0400_0000).contains(&pc)
        || (0x0800_0000..0x0E00_0000).contains(&pc)
}

pub fn log_snapshot(prefix: &str, frame: u32, snap: DebugSnapshot) {
    println!(
        "[{prefix}] frame={frame} pc=0x{:08X} cpsr=0x{:08X} r0=0x{:08X} r1=0x{:08X} r2=0x{:08X} r3=0x{:08X} r4=0x{:08X} r7=0x{:08X} sp=0x{:08X} lr=0x{:08X} cycles={} dispcnt=0x{:04X} vcount={} ime=0x{:04X} ie=0x{:04X} if=0x{:04X} handoff=0x{:02X} bios_irq_flags=0x{:04X} irq_vec=0x{:08X} irq_check=0x{:04X} bios_steps={} rom_steps={} bg0cnt=0x{:04X} hofs=0x{:04X} vofs=0x{:04X} pal0=0x{:04X} pal1=0x{:04X} vram0=0x{:04X} vram10=0x{:04X} vram100=0x{:04X} vram1000=0x{:04X} vram3800=0x{:04X} ew22b4=0x{:04X} ew22b6=0x{:04X} ew22c0=0x{:08X}",
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
        snap.vram10,
        snap.vram100,
        snap.vram1000,
        snap.vram3800,
        snap.ew_22b4,
        snap.ew_22b6,
        snap.ew_22c0
    );
}

pub fn update_progress(
    prefix: &str,
    frame: u32,
    snap: DebugSnapshot,
    debug: DebugOptions,
    state: &mut ProgressState,
) {
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
            snap.pc,
            state.last_pc,
            snap.pc.wrapping_sub(state.last_pc)
        );
    }

    if abs_delta > 0x0100_0000 {
        println!(
            "[{prefix}] anomaly frame={frame} large-pc-jump prev=0x{:08X} pc=0x{:08X} delta=0x{:08X}",
            state.last_pc,
            snap.pc,
            snap.pc.wrapping_sub(state.last_pc)
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
                snap.pc,
                state.same_pc_frames
            );
            log_snapshot(prefix, frame, snap);
        }
    }
}
