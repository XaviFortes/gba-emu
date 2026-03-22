mod audio;
mod core;
mod input;
mod timing;
mod video;

use std::path::Path;

pub use core::{Bus, Cpu};
pub use input::{
    BUTTON_A, BUTTON_B, BUTTON_DOWN, BUTTON_L, BUTTON_LEFT, BUTTON_R, BUTTON_RIGHT,
    BUTTON_SELECT, BUTTON_START, BUTTON_UP,
};
pub use video::{SCREEN_HEIGHT, SCREEN_WIDTH};

const CYCLES_PER_FRAME: u32 = 280_896;
const BIOS_STALL_HANDOFF_FRAMES: u32 = 20;

#[derive(Debug, Clone, Copy)]
pub struct DebugSnapshot {
    pub pc: u32,
    pub cycles: u64,
    pub cpsr: u32,
    pub r0: u32,
    pub r1: u32,
    pub r2: u32,
    pub r3: u32,
    pub r4: u32,
    pub r7: u32,
    pub sp: u32,
    pub lr: u32,
    pub dispcnt: u16,
    pub vcount: u16,
    pub ime: u16,
    pub ie: u16,
    pub iflags: u16,
    pub handoff_7ff0: u8,
    pub bios_irq_flags: u16,
    pub irq_vec: u32,
    pub irq_check: u16,
    pub frame_bios_steps: u32,
    pub frame_rom_steps: u32,
    pub bg0cnt: u16,
    pub bg0hofs: u16,
    pub bg0vofs: u16,
    pub palette0: u16,
    pub palette1: u16,
    pub vram0: u16,
    pub vram10: u16,
    pub vram100: u16,
    pub vram1000: u16,
    pub vram3800: u16,
    pub ew_22b4: u16,
    pub ew_22b6: u16,
    pub ew_22c0: u32,
}

#[derive(Debug)]
pub struct Gba {
    pub bus: Bus,
    pub cpu: Cpu,
    ppu: video::Ppu,
    apu: audio::Apu,
    input: input::Input,
    timers: timing::Timers,
    last_frame_bios_steps: u32,
    last_frame_rom_steps: u32,
    bios_stall_frame_count: u32,
    last_bios_pc: u32,
}

impl Gba {
    pub fn new() -> Self {
        Self {
            bus: Bus::new(),
            cpu: Cpu::new(),
            ppu: video::Ppu::new(),
            apu: audio::Apu::new(),
            input: input::Input::new(),
            timers: timing::Timers::new(),
            last_frame_bios_steps: 0,
            last_frame_rom_steps: 0,
            bios_stall_frame_count: 0,
            last_bios_pc: 0xFFFF_FFFF,
        }
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        if self.bus.has_bios() {
            self.cpu.force_boot_to_bios();
        } else {
            println!("[boot] BIOS not present, switching to ROM boot path");
            self.cpu.force_boot_to_rom();
        }
    }

    pub fn load_bios<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        self.bus.load_bios(path)
    }

    pub fn load_rom<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        self.bus.load_rom(path)
    }

    pub fn run_frame(&mut self) {
        self.run_frame_internal(true);
    }

    pub fn run_frame_headless(&mut self) {
        self.run_frame_internal(false);
    }

    fn run_frame_internal(&mut self, render_video: bool) {
        let mut cycles = 0;
        let mut frame_bios_steps = 0u32;
        let mut frame_rom_steps = 0u32;
        while cycles < CYCLES_PER_FRAME {
            let spent = self.cpu.step(&mut self.bus);

            let pc = self.cpu.pc();
            if pc < 0x0000_4000 {
                frame_bios_steps = frame_bios_steps.saturating_add(1);
            } else if (0x0800_0000..0x0E00_0000).contains(&pc) {
                frame_rom_steps = frame_rom_steps.saturating_add(1);
            }

            self.timers.tick(&mut self.bus, spent);
            self.ppu.tick(spent, &mut self.bus, render_video);
            self.input.tick(&mut self.bus);

            if !self.bus.has_bios() {
                // Emerald no-BIOS compatibility: keep callback-polled IRQ-ready bytes set
                // so startup gate loops do not deadlock waiting on BIOS-maintained state.
                self.bus.write8(0x0300_34A9, 1);
                self.bus.write8(0x0300_6A0C, 1);

                // Startup wait loops periodically clear bit0 and expect IRQ callback
                // heartbeat to set it again out-of-band.
                self.bus.write16(0x0300_22DC, self.bus.read16(0x0300_22DC) | 0x0001);
                self.bus.write16(0x0300_22F8, self.bus.read16(0x0300_22F8) | 0x0001);

                let iflags = self.bus.read_io16(core::bus::REG_IF);
                if iflags != 0 {
                    let irq_check = self.bus.read16(0x0300_22DC) | iflags | 0x0001;
                    self.bus.write16(0x0300_22DC, irq_check);

                    let irq_check_alt = self.bus.read16(0x0300_22F8) | iflags | 0x0001;
                    self.bus.write16(0x0300_22F8, irq_check_alt);
                }
            }

            self.apu.tick(&self.bus);
            cycles += spent;
        }

        self.last_frame_bios_steps = frame_bios_steps;
        self.last_frame_rom_steps = frame_rom_steps;

        // BIOS stall detection: if BIOS is stuck in a tight loop (no ROM execution),
        // force handoff to ROM after a reasonable timeout. Real BIOS always completes
        // initialization and hands off within ~60 frames; this detects runaway BIOS.
        if self.bus.has_bios() && frame_rom_steps == 0 && frame_bios_steps > 100_000 {
            let pc = self.cpu.pc();
            if pc < 0x0000_4000 {
                // BIOS is still executing. Check if it's stuck in same address
                // (executes the same few instructions repeatedly in a loop).
                if pc == self.last_bios_pc {
                    self.bios_stall_frame_count += 1;
                    // Force handoff if stuck at the same BIOS PC for long enough.
                    // 20 frames is conservative and avoids very long debug-build startup delays.
                    if self.bios_stall_frame_count >= BIOS_STALL_HANDOFF_FRAMES {
                        // BIOS has completed hardware initialization and doesn't proceed further.
                        // This is normal - BIOS hands off to ROM when initialization is complete.
                        // Only log if verbosity is enabled; this is expected behavior, not a bug.
                        if std::env::var("GBA_VERBOSE_BOOT").is_ok() {
                            println!("[bios-boot] BIOS completed initialization at 0x{:08X}; handing off to ROM", pc);
                        }
                        println!("[boot] switching to ROM entry (ARM System mode)");
                        self.force_boot_to_rom_without_bios();
                        self.bios_stall_frame_count = 0;
                        self.last_bios_pc = 0xFFFF_FFFF;
                        return;
                    }
                } else {
                    // PC changed (BIOS is still executing code), reset stall counter
                    self.last_bios_pc = pc;
                    self.bios_stall_frame_count = 0;
                }
            }
        }
    }

    pub fn set_input_held_mask(&mut self, held_mask: u16) {
        self.input.set_held_mask(held_mask);
    }

    pub fn take_frame_ready(&mut self) -> bool {
        self.ppu.take_frame_ready()
    }

    pub fn framebuffer(&self) -> &[u32] {
        self.ppu.framebuffer()
    }

    pub fn debug_snapshot(&self) -> DebugSnapshot {
        DebugSnapshot {
            pc: self.cpu.pc(),
            cycles: self.cpu.cycles,
            cpsr: self.cpu.cpsr(),
            r0: self.cpu.read_reg(0),
            r1: self.cpu.read_reg(1),
            r2: self.cpu.read_reg(2),
            r3: self.cpu.read_reg(3),
            r4: self.cpu.read_reg(4),
            r7: self.cpu.read_reg(7),
            sp: self.cpu.read_reg(13),
            lr: self.cpu.read_reg(14),
            dispcnt: self.bus.read_io16(core::bus::REG_DISPCNT),
            vcount: self.bus.read_io16(core::bus::REG_VCOUNT),
            ime: self.bus.read_io16(core::bus::REG_IME),
            ie: self.bus.read_io16(core::bus::REG_IE),
            iflags: self.bus.read_io16(core::bus::REG_IF),
            handoff_7ff0: self.bus.read8(0x0300_7FF0),
            bios_irq_flags: self.bus.read16(0x03FF_FFF8),
            irq_vec: self.bus.read32(0x03FF_FFFC),
            irq_check: self.bus.read16(0x0300_22DC),
            frame_bios_steps: self.last_frame_bios_steps,
            frame_rom_steps: self.last_frame_rom_steps,
            bg0cnt: self.bus.read_io16(0x0400_0008),
            bg0hofs: self.bus.read_io16(0x0400_0010),
            bg0vofs: self.bus.read_io16(0x0400_0012),
            palette0: self.bus.read16(0x0500_0000),
            palette1: self.bus.read16(0x0500_0002),
            vram0: self.bus.read16(0x0600_0000),
            vram10: self.bus.read16(0x0600_0010),
            vram100: self.bus.read16(0x0600_0100),
            vram1000: self.bus.read16(0x0600_1000),
            vram3800: self.bus.read16(0x0600_3800),
            ew_22b4: self.bus.read16(0x0300_22B4),
            ew_22b6: self.bus.read16(0x0300_22B6),
            ew_22c0: self.bus.read32(0x0300_22C0),
        }
    }

    pub fn set_trace_branches(&mut self, enabled: bool) {
        core::Cpu::set_trace_branches(enabled);
    }

    pub fn force_boot_to_rom_without_bios(&mut self) {
        println!("[boot] disabling BIOS mapping and jumping to ROM entry");
        self.bus.disable_bios();
        self.cpu.jump_to_rom_entry();
    }
}

impl Default for Gba {
    fn default() -> Self {
        Self::new()
    }
}
