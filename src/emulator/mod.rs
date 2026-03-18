mod apu;
mod bus;
mod cpu;
mod input;
mod ppu;
mod timers;

use std::path::Path;

pub use bus::Bus;
pub use cpu::Cpu;
pub use input::{BUTTON_A, BUTTON_B, BUTTON_DOWN, BUTTON_L, BUTTON_LEFT, BUTTON_R, BUTTON_RIGHT, BUTTON_SELECT, BUTTON_START, BUTTON_UP};
pub use ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

const CYCLES_PER_FRAME: u32 = 280_896;

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
}

#[derive(Debug)]
pub struct Gba {
    pub bus: Bus,
    pub cpu: Cpu,
    ppu: ppu::Ppu,
    apu: apu::Apu,
    input: input::Input,
    timers: timers::Timers,
    last_frame_bios_steps: u32,
    last_frame_rom_steps: u32,
}

impl Gba {
    pub fn new() -> Self {
        Self {
            bus: Bus::new(),
            cpu: Cpu::new(),
            ppu: ppu::Ppu::new(),
            apu: apu::Apu::new(),
            input: input::Input::new(),
            timers: timers::Timers::new(),
            last_frame_bios_steps: 0,
            last_frame_rom_steps: 0,
        }
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        if self.bus.has_bios() {
            self.cpu.force_boot_to_bios();
        } else {
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

                let iflags = self.bus.read_io16(bus::REG_IF);
                if iflags != 0 {
                    let irq_check = self.bus.read16(0x0300_22DC) | iflags;
                    self.bus.write16(0x0300_22DC, irq_check);

                    // BIOS-less IRQ compatibility: acknowledge handled IRQ bits.
                    self.bus.write_io16(bus::REG_IF, iflags);
                }
            }

            self.apu.tick(&self.bus);
            cycles += spent;
        }

        self.last_frame_bios_steps = frame_bios_steps;
        self.last_frame_rom_steps = frame_rom_steps;
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
            dispcnt: self.bus.read_io16(bus::REG_DISPCNT),
            vcount: self.bus.read_io16(bus::REG_VCOUNT),
            ime: self.bus.read_io16(bus::REG_IME),
            ie: self.bus.read_io16(bus::REG_IE),
            iflags: self.bus.read_io16(bus::REG_IF),
            handoff_7ff0: self.bus.read8(0x0300_7FF0),
            bios_irq_flags: self.bus.read16(0x03FF_FFF8),
            irq_vec: self.bus.read32(0x03FF_FFFC),
            irq_check: self.bus.read16(0x0300_22DC),
            frame_bios_steps: self.last_frame_bios_steps,
            frame_rom_steps: self.last_frame_rom_steps,
        }
    }

    pub fn set_trace_branches(&mut self, enabled: bool) {
        cpu::Cpu::set_trace_branches(enabled);
    }

    pub fn force_boot_to_rom_without_bios(&mut self) {
        self.bus.disable_bios();
        self.cpu.force_boot_to_rom();
    }
}

impl Default for Gba {
    fn default() -> Self {
        Self::new()
    }
}
