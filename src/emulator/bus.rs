use std::fs;
use std::path::Path;
use std::sync::OnceLock;

pub const BIOS_START: u32 = 0x0000_0000;
pub const BIOS_SIZE: usize = 16 * 1024;

pub const EWRAM_START: u32 = 0x0200_0000;
pub const EWRAM_SIZE: usize = 256 * 1024;
const EWRAM_REGION_SIZE: u32 = 0x0100_0000;

pub const IWRAM_START: u32 = 0x0300_0000;
pub const IWRAM_SIZE: usize = 32 * 1024;
const IWRAM_REGION_SIZE: u32 = 0x0100_0000;

pub const IO_START: u32 = 0x0400_0000;
pub const IO_SIZE: usize = 1024;

pub const PALETTE_RAM_START: u32 = 0x0500_0000;
pub const PALETTE_RAM_SIZE: usize = 1024;

pub const VRAM_START: u32 = 0x0600_0000;
pub const VRAM_SIZE: usize = 96 * 1024;

pub const OAM_START: u32 = 0x0700_0000;
pub const OAM_SIZE: usize = 1024;

pub const GAMEPAK_ROM_START: u32 = 0x0800_0000;
const GAMEPAK_ROM_END: u32 = 0x0E00_0000;
const GAMEPAK_SRAM_START: u32 = 0x0E00_0000;
const GAMEPAK_SRAM_END: u32 = 0x0E01_0000;

pub const REG_DISPCNT: u32 = IO_START + 0x000;
pub const REG_DISPSTAT: u32 = IO_START + 0x004;
pub const REG_VCOUNT: u32 = IO_START + 0x006;
pub const REG_KEYINPUT: u32 = IO_START + 0x130;
pub const REG_IE: u32 = IO_START + 0x200;
pub const REG_IF: u32 = IO_START + 0x202;
pub const REG_IME: u32 = IO_START + 0x208;
const REG_HALTCNT: u32 = IO_START + 0x301;

pub const IRQ_VBLANK: u16 = 1 << 0;
#[allow(dead_code)]
pub const IRQ_HBLANK: u16 = 1 << 1;
#[allow(dead_code)]
pub const IRQ_VCOUNT: u16 = 1 << 2;
pub const IRQ_TIMER0: u16 = 1 << 3;
pub const IRQ_TIMER1: u16 = 1 << 4;
pub const IRQ_TIMER2: u16 = 1 << 5;
pub const IRQ_TIMER3: u16 = 1 << 6;

const DMA_SAD_OFFSETS: [u32; 4] = [0x0B0, 0x0BC, 0x0C8, 0x0D4];
const DMA_DAD_OFFSETS: [u32; 4] = [0x0B4, 0x0C0, 0x0CC, 0x0D8];
const DMA_CNT_L_OFFSETS: [u32; 4] = [0x0B8, 0x0C4, 0x0D0, 0x0DC];
const DMA_CNT_H_OFFSETS: [u32; 4] = [0x0BA, 0x0C6, 0x0D2, 0x0DE];
const DMA_IRQ_MASKS: [u16; 4] = [1 << 8, 1 << 9, 1 << 10, 1 << 11];

const TIMER_RELOAD_OFFSETS: [u32; 4] = [0x100, 0x104, 0x108, 0x10C];
const TIMER_CTRL_OFFSETS: [u32; 4] = [0x102, 0x106, 0x10A, 0x10E];
const TIMER_IRQ_MASKS: [u16; 4] = [IRQ_TIMER0, IRQ_TIMER1, IRQ_TIMER2, IRQ_TIMER3];
const TIMER_PRESCALERS: [u32; 4] = [1, 64, 256, 1024];

#[derive(Debug, Clone, Copy)]
struct TimerState {
    divider: u32,
    reload: u16,
}

#[derive(Debug)]
pub struct Bus {
    bios: [u8; BIOS_SIZE],
    bios_loaded: bool,
    ewram: [u8; EWRAM_SIZE],
    iwram: [u8; IWRAM_SIZE],
    io: [u8; IO_SIZE],
    palette_ram: [u8; PALETTE_RAM_SIZE],
    vram: [u8; VRAM_SIZE],
    oam: [u8; OAM_SIZE],
    rom: Vec<u8>,
    timers: [TimerState; 4],
    halt_requested: bool,
}

impl Bus {
    pub fn new() -> Self {
        let mut bus = Self {
            bios: [0; BIOS_SIZE],
            bios_loaded: false,
            ewram: [0; EWRAM_SIZE],
            iwram: [0; IWRAM_SIZE],
            io: [0; IO_SIZE],
            palette_ram: [0; PALETTE_RAM_SIZE],
            vram: [0; VRAM_SIZE],
            oam: [0; OAM_SIZE],
            rom: Vec::new(),
            timers: [
                TimerState {
                    divider: 0,
                    reload: 0,
                };
                4
            ],
            halt_requested: false,
        };
        bus.write_io16_raw(REG_KEYINPUT, 0x03FF);
        bus
    }

    pub fn load_bios<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        let data = fs::read(path)?;
        let len = data.len().min(BIOS_SIZE);
        self.bios[..len].copy_from_slice(&data[..len]);
        self.bios_loaded = len > 0;
        Ok(())
    }

    pub fn has_bios(&self) -> bool {
        self.bios_loaded
    }

    pub fn disable_bios(&mut self) {
        self.bios_loaded = false;
    }

    pub fn load_rom<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        self.rom = fs::read(path)?;
        Ok(())
    }

    pub fn vram(&self) -> &[u8] {
        &self.vram
    }

    pub fn read8(&self, addr: u32) -> u8 {
        if (BIOS_START..BIOS_START + BIOS_SIZE as u32).contains(&addr) {
            return self.bios[(addr - BIOS_START) as usize];
        }

        if (EWRAM_START..EWRAM_START + EWRAM_REGION_SIZE).contains(&addr) {
            let off = ((addr - EWRAM_START) as usize) % EWRAM_SIZE;
            return self.ewram[off];
        }

        if (IWRAM_START..IWRAM_START + IWRAM_REGION_SIZE).contains(&addr) {
            let off = ((addr - IWRAM_START) as usize) % IWRAM_SIZE;
            return self.iwram[off];
        }

        if (IO_START..IO_START + IO_SIZE as u32).contains(&addr) {
            return self.io[(addr - IO_START) as usize];
        }

        if (PALETTE_RAM_START..PALETTE_RAM_START + PALETTE_RAM_SIZE as u32).contains(&addr) {
            return self.palette_ram[(addr - PALETTE_RAM_START) as usize];
        }

        if (VRAM_START..VRAM_START + VRAM_SIZE as u32).contains(&addr) {
            return self.vram[(addr - VRAM_START) as usize];
        }

        if (OAM_START..OAM_START + OAM_SIZE as u32).contains(&addr) {
            return self.oam[(addr - OAM_START) as usize];
        }

        if (GAMEPAK_ROM_START..GAMEPAK_ROM_END).contains(&addr) {
            if self.rom.is_empty() {
                return 0xFF;
            }
            let offset = ((addr - GAMEPAK_ROM_START) as usize) % self.rom.len();
            return self.rom[offset];
        }

        if (GAMEPAK_SRAM_START..GAMEPAK_SRAM_END).contains(&addr) {
            // Backup memory is unimplemented for now; open-bus style fallback.
            return 0xFF;
        }

        0
    }

    pub fn read16(&self, addr: u32) -> u16 {
        let lo = self.read8(addr) as u16;
        let hi = self.read8(addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    pub fn read32(&self, addr: u32) -> u32 {
        let b0 = self.read8(addr) as u32;
        let b1 = self.read8(addr.wrapping_add(1)) as u32;
        let b2 = self.read8(addr.wrapping_add(2)) as u32;
        let b3 = self.read8(addr.wrapping_add(3)) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    pub fn write8(&mut self, addr: u32, value: u8) {
        if trace_bios_bus_enabled()
            && (addr == REG_IME
                || addr == REG_IE
                || addr == REG_IE + 1
                || addr == REG_IF
                || addr == REG_IF + 1
                || addr == REG_HALTCNT
                || addr == 0x03FF_FFF8
                || addr == 0x03FF_FFF9)
        {
            println!("[bios-bus] write8 addr=0x{:08X} value=0x{:02X}", addr, value);
        }

        if (EWRAM_START..EWRAM_START + EWRAM_REGION_SIZE).contains(&addr) {
            let off = ((addr - EWRAM_START) as usize) % EWRAM_SIZE;
            self.ewram[off] = value;
            return;
        }

        if (IWRAM_START..IWRAM_START + IWRAM_REGION_SIZE).contains(&addr) {
            let off = ((addr - IWRAM_START) as usize) % IWRAM_SIZE;
            self.iwram[off] = value;
            return;
        }

        if (IO_START..IO_START + IO_SIZE as u32).contains(&addr) {
            if addr == REG_HALTCNT {
                // HALTCNT bit7=0 enters HALT (STOP is bit7=1 and remains unimplemented).
                if (value & 0x80) == 0 {
                    self.halt_requested = true;
                }
                return;
            }

            if addr == REG_IF || addr == REG_IF + 1 {
                let current = self.read_io16_raw(REG_IF);
                let clear_mask = if addr == REG_IF {
                    value as u16
                } else {
                    (value as u16) << 8
                };
                self.write_io16_raw(REG_IF, current & !clear_mask);
                return;
            }

            if addr == REG_VCOUNT
                || addr == REG_VCOUNT + 1
                || addr == REG_KEYINPUT
                || addr == REG_KEYINPUT + 1
            {
                return;
            }
            let index = (addr - IO_START) as usize;
            self.io[index] = value;
            return;
        }

        if (PALETTE_RAM_START..PALETTE_RAM_START + PALETTE_RAM_SIZE as u32).contains(&addr) {
            let off = ((addr - PALETTE_RAM_START) as usize) & !1;
            self.palette_ram[off] = value;
            self.palette_ram[off + 1] = value; // Escribimos en los dos bytes
            return;
        }

        if (VRAM_START..VRAM_START + VRAM_SIZE as u32).contains(&addr) {
            let off = ((addr - VRAM_START) as usize) & !1;
            self.vram[off] = value;
            self.vram[off + 1] = value; // Escribimos en los dos bytes
            return;
        }

        if (OAM_START..OAM_START + OAM_SIZE as u32).contains(&addr) {
            self.oam[(addr - OAM_START) as usize] = value;
        }
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        if trace_bios_bus_enabled()
            && (addr == REG_IME
                || addr == REG_IE
                || addr == REG_IF
                || addr == 0x03FF_FFF8)
        {
            println!("[bios-bus] write16 addr=0x{:08X} value=0x{:04X}", addr, value);
        }

        if addr == REG_VCOUNT || addr == REG_KEYINPUT {
            return;
        }

        if addr == REG_IF {
            // IF is write-1-to-clear.
            let current = self.read_io16_raw(REG_IF);
            self.write_io16_raw(REG_IF, current & !value);
            return;
        }

        if (IO_START..IO_START + IO_SIZE as u32).contains(&addr) {
            let old_value = self.read_io16_raw(addr);
            self.write_io16_raw(addr, value);
            self.handle_side_effects_16(addr, old_value, value);
            return;
        }

        // ESCRITURA DIRECTA A VRAM (Alineada)
        if (VRAM_START..VRAM_START + VRAM_SIZE as u32).contains(&addr) {
            let off = ((addr - VRAM_START) as usize) & !1;
            self.vram[off] = (value & 0xFF) as u8;
            self.vram[off + 1] = (value >> 8) as u8;
            return;
        }
        
        // ESCRITURA DIRECTA A PALETA
        if (PALETTE_RAM_START..PALETTE_RAM_START + PALETTE_RAM_SIZE as u32).contains(&addr) {
            let off = ((addr - PALETTE_RAM_START) as usize) & !1;
            self.palette_ram[off] = (value & 0xFF) as u8;
            self.palette_ram[off + 1] = (value >> 8) as u8;
            return;
        }

        self.write8(addr, (value & 0x00FF) as u8);
        self.write8(addr.wrapping_add(1), (value >> 8) as u8);
    }

    pub fn write32(&mut self, addr: u32, value: u32) {
        if (IO_START..IO_START + IO_SIZE as u32).contains(&addr) {
            self.write16(addr, (value & 0xFFFF) as u16);
            self.write16(addr.wrapping_add(2), (value >> 16) as u16);
            return;
        }

        // ESCRITURA DIRECTA A PALETA (Alineada 32 bits)
        if (PALETTE_RAM_START..PALETTE_RAM_START + PALETTE_RAM_SIZE as u32).contains(&addr) {
            let off = ((addr - PALETTE_RAM_START) as usize) & !3;
            self.palette_ram[off] = (value & 0xFF) as u8;
            self.palette_ram[off + 1] = ((value >> 8) & 0xFF) as u8;
            self.palette_ram[off + 2] = ((value >> 16) & 0xFF) as u8;
            self.palette_ram[off + 3] = ((value >> 24) & 0xFF) as u8;
            return;
        }

        // ESCRITURA DIRECTA A VRAM (Alineada 32 bits)
        if (VRAM_START..VRAM_START + VRAM_SIZE as u32).contains(&addr) {
            let off = ((addr - VRAM_START) as usize) & !3;
            self.vram[off] = (value & 0xFF) as u8;
            self.vram[off + 1] = ((value >> 8) & 0xFF) as u8;
            self.vram[off + 2] = ((value >> 16) & 0xFF) as u8;
            self.vram[off + 3] = ((value >> 24) & 0xFF) as u8;
            return;
        }

        self.write8(addr, (value & 0x0000_00FF) as u8);
        self.write8(addr.wrapping_add(1), ((value >> 8) & 0xFF) as u8);
        self.write8(addr.wrapping_add(2), ((value >> 16) & 0xFF) as u8);
        self.write8(addr.wrapping_add(3), ((value >> 24) & 0xFF) as u8);
    }

    pub fn read_io16(&self, addr: u32) -> u16 {
        self.read16(addr)
    }

    pub fn write_io16(&mut self, addr: u32, value: u16) {
        self.write16(addr, value);
    }

    pub fn set_keyinput_from_held_mask(&mut self, held_mask: u16) {
        let keyinput = (!held_mask) & 0x03FF;
        self.write_io16_raw(REG_KEYINPUT, keyinput);
    }

    pub fn set_vcount(&mut self, value: u16) {
        self.write_io16_raw(REG_VCOUNT, value);
    }

    pub fn request_interrupt(&mut self, irq_mask: u16) {
        if trace_bios_bus_enabled() {
            println!(
                "[bios-bus] request_interrupt mask=0x{:04X} ie=0x{:04X} if_before=0x{:04X}",
                irq_mask,
                self.read_io16_raw(REG_IE),
                self.read_io16_raw(REG_IF)
            );
        }

        let pending = self.read_io16_raw(REG_IF) | irq_mask;
        self.write_io16_raw(REG_IF, pending);

        if trace_bios_bus_enabled() {
            println!(
                "[bios-bus] request_interrupt if_after=0x{:04X}",
                self.read_io16_raw(REG_IF)
            );
        }

        if !self.bios_loaded {
            // No-BIOS compatibility: many commercial games rely on a RAM IRQ-check
            // halfword normally maintained by their installed IRQ callback.
            // Mirror pending IRQ bits into that location so wait loops can progress.
            let mut irq_check = self.read16(0x0300_22DC);
            irq_check |= irq_mask;
            self.write16(0x0300_22DC, irq_check);
        }
    }

    pub fn has_pending_interrupts(&self) -> bool {
        let ime = self.read_io16_raw(REG_IME);
        let ie = self.read_io16_raw(REG_IE);
        let iflags = self.read_io16_raw(REG_IF);
        (ime & 1) != 0 && (ie & iflags) != 0
    }

    pub fn claim_pending_interrupt(&mut self) -> Option<u16> {
        let ie = self.read_io16_raw(REG_IE);
        let iflags = self.read_io16_raw(REG_IF);
        let pending = ie & iflags;
        if pending == 0 {
            return None;
        }

        let mask = 1u16 << pending.trailing_zeros();
        Some(mask)
    }

    pub fn take_halt_request(&mut self) -> bool {
        let requested = self.halt_requested;
        self.halt_requested = false;
        requested
    }

    pub fn tick_timers(&mut self, cycles: u32) {
        for i in 0..4 {
            let counter_addr = IO_START + TIMER_RELOAD_OFFSETS[i];
            let ctrl_addr = IO_START + TIMER_CTRL_OFFSETS[i];

            let ctrl = self.read_io16_raw(ctrl_addr);
            let enabled = (ctrl & (1 << 7)) != 0;
            if !enabled {
                continue;
            }

            let cascade = (ctrl & (1 << 2)) != 0;
            if cascade {
                continue;
            }

            let prescaler = TIMER_PRESCALERS[(ctrl & 0b11) as usize];
            self.timers[i].divider = self.timers[i].divider.wrapping_add(cycles);

            while self.timers[i].divider >= prescaler {
                self.timers[i].divider -= prescaler;
                let mut counter = self.read_io16_raw(counter_addr);
                counter = counter.wrapping_add(1);
                if counter == 0 {
                    counter = self.timers[i].reload;
                    if (ctrl & (1 << 6)) != 0 {
                        self.request_interrupt(TIMER_IRQ_MASKS[i]);
                    }
                }
                self.write_io16_raw(counter_addr, counter);
            }
        }
    }

    fn handle_side_effects_16(&mut self, addr: u32, old_value: u16, value: u16) {
        if let Some(idx) = timer_index_from_reload_addr(addr) {
            self.timers[idx].reload = value;
            return;
        }

        if let Some(channel) = dma_channel_from_cnt_h_addr(addr) {
            let newly_enabled = (value & (1 << 15)) != 0 && (old_value & (1 << 15)) == 0;
            let start_timing = (value >> 12) & 0b11;
            if newly_enabled && start_timing == 0 {
                self.run_dma(channel);
            }
            return;
        }

        if let Some(idx) = timer_index_from_ctrl_addr(addr) {
            let enabled = (value & (1 << 7)) != 0;
            let was_enabled = (old_value & (1 << 7)) != 0;
            if enabled && !was_enabled {
                self.timers[idx].divider = 0;
                let counter_addr = IO_START + TIMER_RELOAD_OFFSETS[idx];
                self.write_io16_raw(counter_addr, self.timers[idx].reload);
            }
        }
    }

    fn run_dma(&mut self, channel: usize) {
        let sad = IO_START + DMA_SAD_OFFSETS[channel];
        let dad = IO_START + DMA_DAD_OFFSETS[channel];
        let cnt_l_addr = IO_START + DMA_CNT_L_OFFSETS[channel];
        let cnt_h_addr = IO_START + DMA_CNT_H_OFFSETS[channel];

        let mut src = self.read32(sad);
        let mut dst = self.read32(dad);
        let cnt_l = self.read_io16_raw(cnt_l_addr);
        let mut cnt_h = self.read_io16_raw(cnt_h_addr);

        let transfer32 = (cnt_h & (1 << 10)) != 0;
        let src_ctrl = (cnt_h >> 7) & 0b11;
        let dst_ctrl = (cnt_h >> 5) & 0b11;

        let unit_size = if transfer32 { 4 } else { 2 };
        let mut words = cnt_l as u32;
        if words == 0 {
            words = if channel == 3 { 0x1_0000 } else { 0x4000 };
        }

        for _ in 0..words {
            if transfer32 {
                let value = self.read32(src & !3);
                self.write32(dst & !3, value);
            } else {
                let value = self.read16(src & !1);
                self.write16(dst & !1, value);
            }

            src = update_dma_addr(src, src_ctrl, unit_size);
            dst = update_dma_addr(dst, dst_ctrl, unit_size);
        }

        if (cnt_h & (1 << 9)) == 0 {
            cnt_h &= !(1 << 15);
            self.write_io16_raw(cnt_h_addr, cnt_h);
        }

        if (cnt_h & (1 << 14)) != 0 {
            self.request_interrupt(DMA_IRQ_MASKS[channel]);
        }
    }

    fn write_io16_raw(&mut self, addr: u32, value: u16) {
        let index = (addr - IO_START) as usize;
        self.io[index] = (value & 0x00FF) as u8;
        self.io[index + 1] = (value >> 8) as u8;
    }

    fn read_io16_raw(&self, addr: u32) -> u16 {
        let index = (addr - IO_START) as usize;
        self.io[index] as u16 | ((self.io[index + 1] as u16) << 8)
    }
}

fn timer_index_from_ctrl_addr(addr: u32) -> Option<usize> {
    TIMER_CTRL_OFFSETS
        .iter()
        .position(|off| IO_START + *off == addr)
}

fn timer_index_from_reload_addr(addr: u32) -> Option<usize> {
    TIMER_RELOAD_OFFSETS
        .iter()
        .position(|off| IO_START + *off == addr)
}

fn dma_channel_from_cnt_h_addr(addr: u32) -> Option<usize> {
    DMA_CNT_H_OFFSETS
        .iter()
        .position(|off| IO_START + *off == addr)
}

fn update_dma_addr(addr: u32, control: u16, amount: u32) -> u32 {
    match control {
        0 => addr.wrapping_add(amount),
        1 => addr.wrapping_sub(amount),
        2 => addr,
        3 => addr.wrapping_add(amount),
        _ => addr,
    }
}

fn trace_bios_bus_enabled() -> bool {
    static TRACE: OnceLock<bool> = OnceLock::new();
    *TRACE.get_or_init(|| {
        std::env::var("GBA_TRACE_BIOS_BUS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ewram_roundtrip_32bit() {
        let mut bus = Bus::new();
        bus.write32(EWRAM_START, 0x1122_3344);
        assert_eq!(bus.read32(EWRAM_START), 0x1122_3344);
    }

    #[test]
    fn reads_out_of_bounds_rom_as_ff() {
        let bus = Bus::new();
        assert_eq!(bus.read8(GAMEPAK_ROM_START), 0xFF);
    }

    #[test]
    fn keyinput_is_active_low() {
        let mut bus = Bus::new();
        bus.set_keyinput_from_held_mask(0b11);
        assert_eq!(bus.read_io16(REG_KEYINPUT) & 0b11, 0);
    }
}
