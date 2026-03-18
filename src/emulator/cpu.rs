use super::bus::{
    Bus, BIOS_SIZE, EWRAM_SIZE, EWRAM_START, GAMEPAK_ROM_START, IWRAM_SIZE, IWRAM_START,
    OAM_SIZE, OAM_START, PALETTE_RAM_SIZE, PALETTE_RAM_START, VRAM_SIZE, VRAM_START,
};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static TRACE_BRANCHES: AtomicBool = AtomicBool::new(false);

pub const REG_SP: usize = 13;
pub const REG_LR: usize = 14;
pub const REG_PC: usize = 15;

const CPSR_N: u32 = 1 << 31;
const CPSR_Z: u32 = 1 << 30;
const CPSR_C: u32 = 1 << 29;
const CPSR_V: u32 = 1 << 28;
const CPSR_THUMB: u32 = 1 << 5;
const CPSR_IRQ_DISABLE: u32 = 1 << 7;
const BIOSLESS_IRQ_RETURN_MAGIC: u32 = 0xFFFF_FFE0;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMode {
    User,
    Fiq,
    Irq,
    Supervisor,
    Abort,
    Undefined,
    System,
}

#[derive(Debug)]
pub struct Cpu {
    regs: [u32; 16],
    cpsr: u32,
    mode: CpuMode,
    spsr_irq: u32,
    spsr_svc: u32,
    spsr_fiq: u32,
    spsr_abt: u32,
    spsr_und: u32,
    banked_sp_irq: u32,
    banked_lr_irq: u32,
    banked_sp_svc: u32,
    banked_lr_svc: u32,
    banked_r8_fiq: u32,
    banked_r9_fiq: u32,
    banked_r10_fiq: u32,
    banked_r11_fiq: u32,
    banked_r12_fiq: u32,
    banked_sp_fiq: u32,
    banked_lr_fiq: u32,
    banked_sp_abt: u32,
    banked_lr_abt: u32,
    banked_sp_und: u32,
    banked_lr_und: u32,
    banked_r8_sys: u32,
    banked_r9_sys: u32,
    banked_r10_sys: u32,
    banked_r11_sys: u32,
    banked_r12_sys: u32,
    banked_sp_sys: u32,
    banked_lr_sys: u32,
    biosless_irq_active: bool,
    biosless_irq_saved: [u32; 5],
    biosless_irq_lr: u32,
    pub halted: bool,
    pub cycles: u64,
}

impl Cpu {
    pub fn new() -> Self {
        let mut cpu = Self {
            regs: [0; 16],
            cpsr: 0x0000_001F,
            mode: CpuMode::System,
            spsr_irq: 0,
            spsr_svc: 0,
            spsr_fiq: 0,
            spsr_abt: 0,
            spsr_und: 0,
            banked_sp_irq: 0x0300_7FA0,
            banked_lr_irq: 0,
            banked_sp_svc: 0x0300_7FE0,
            banked_lr_svc: 0,
            banked_r8_fiq: 0,
            banked_r9_fiq: 0,
            banked_r10_fiq: 0,
            banked_r11_fiq: 0,
            banked_r12_fiq: 0,
            banked_sp_fiq: 0,
            banked_lr_fiq: 0,
            banked_sp_abt: 0,
            banked_lr_abt: 0,
            banked_sp_und: 0,
            banked_lr_und: 0,
            banked_r8_sys: 0,
            banked_r9_sys: 0,
            banked_r10_sys: 0,
            banked_r11_sys: 0,
            banked_r12_sys: 0,
            banked_sp_sys: 0x0300_7F00,
            banked_lr_sys: 0,
            biosless_irq_active: false,
            biosless_irq_saved: [0; 5],
            biosless_irq_lr: 0,
            halted: false,
            cycles: 0,
        };
        cpu.regs[REG_PC] = GAMEPAK_ROM_START;
        cpu
    }

    pub fn reset(&mut self) {
        self.regs = [0; 16];
        self.regs[REG_SP] = 0x0300_7F00;
        self.regs[REG_PC] = GAMEPAK_ROM_START;
        self.cpsr = 0x0000_001F;
        self.mode = CpuMode::System;
        self.spsr_irq = 0;
        self.spsr_svc = 0;
        self.spsr_fiq = 0;
        self.spsr_abt = 0;
        self.spsr_und = 0;
        self.banked_sp_irq = 0x0300_7FA0;
        self.banked_lr_irq = 0;
        self.banked_sp_svc = 0x0300_7FE0;
        self.banked_lr_svc = 0;
        self.banked_r8_fiq = 0;
        self.banked_r9_fiq = 0;
        self.banked_r10_fiq = 0;
        self.banked_r11_fiq = 0;
        self.banked_r12_fiq = 0;
        self.banked_sp_fiq = 0;
        self.banked_lr_fiq = 0;
        self.banked_sp_abt = 0;
        self.banked_lr_abt = 0;
        self.banked_sp_und = 0;
        self.banked_lr_und = 0;
        self.banked_r8_sys = self.regs[8];
        self.banked_r9_sys = self.regs[9];
        self.banked_r10_sys = self.regs[10];
        self.banked_r11_sys = self.regs[11];
        self.banked_r12_sys = self.regs[12];
        self.banked_sp_sys = self.regs[REG_SP];
        self.banked_lr_sys = self.regs[REG_LR];
        self.biosless_irq_active = false;
        self.biosless_irq_saved = [0; 5];
        self.biosless_irq_lr = 0;
        self.halted = false;
        self.cycles = 0;
    }

    pub fn pc(&self) -> u32 {
        self.regs[REG_PC]
    }

    pub fn set_pc(&mut self, value: u32) {
        self.regs[REG_PC] = value;
    }

    pub fn read_reg(&self, index: usize) -> u32 {
        self.regs[index]
    }

    pub fn write_reg(&mut self, index: usize, value: u32) {
        self.regs[index] = value;
    }

    pub fn is_thumb(&self) -> bool {
        (self.cpsr & CPSR_THUMB) != 0
    }

    pub fn cpsr(&self) -> u32 {
        self.cpsr
    }

    pub fn force_boot_to_rom(&mut self) {
        self.cpsr &= !CPSR_THUMB;
        self.set_pc(GAMEPAK_ROM_START);
    }

    pub fn set_trace_branches(enabled: bool) {
        TRACE_BRANCHES.store(enabled, Ordering::Relaxed);
    }

    pub fn step(&mut self, bus: &mut Bus) -> u32 {
        if self.halted {
            if !bus.has_bios() && bus.has_pending_interrupts() {
                // No BIOS path: SWI IntrWait/Halt should still resume when IF/IE/IME match,
                // even if we are not entering the BIOS IRQ exception vector.
                self.halted = false;
                self.cycles += 1;
                return 1;
            }

            if self.handle_irq(bus) {
                self.halted = false;
                self.cycles += 7;
                return 7;
            }
            return 1;
        }

        if self.handle_irq(bus) {
            self.cycles += 7;
            return 7;
        }

        let pc = self.pc();
        let thumb = self.is_thumb();
        let expected_next = if thumb {
            pc.wrapping_add(2)
        } else {
            pc.wrapping_add(4)
        };

        let spent = if thumb {
            let instr = bus.read16(pc);
            self.set_pc(pc.wrapping_add(2));
            let spent = self.exec_thumb(bus, instr);
            if TRACE_BRANCHES.load(Ordering::Relaxed) && self.pc() != expected_next {
                println!(
                    "[trace] THUMB branch pc=0x{:08X} instr=0x{:04X} -> 0x{:08X}",
                    pc,
                    instr,
                    self.pc()
                );
            }
            spent
        } else {
            let instr = bus.read32(pc);
            self.set_pc(pc.wrapping_add(4));
            let spent = self.exec_arm(bus, instr);
            if TRACE_BRANCHES.load(Ordering::Relaxed) && self.pc() != expected_next {
                println!(
                    "[trace] ARM branch pc=0x{:08X} instr=0x{:08X} -> 0x{:08X}",
                    pc,
                    instr,
                    self.pc()
                );
            }
            spent
        };

        self.cycles += spent as u64;
        spent
    }

    fn handle_irq(&mut self, bus: &mut Bus) -> bool {
        if !bus.has_bios() {
            return false;
        }

        if (self.cpsr & CPSR_IRQ_DISABLE) != 0 {
            return false;
        }

        if !bus.has_pending_interrupts() {
            return false;
        }

        let Some(_mask) = bus.claim_pending_interrupt() else {
            return false;
        };

        self.spsr_irq = self.cpsr;
        self.switch_mode(CpuMode::Irq);

        self.regs[REG_LR] = if self.is_thumb() {
            self.pc().wrapping_add(2)
        } else {
            self.pc().wrapping_add(4)
        };

        self.cpsr &= !CPSR_THUMB;
        self.cpsr |= CPSR_IRQ_DISABLE;

        if bus.has_bios() {
            // Standard ARM IRQ vector entry via BIOS.
            self.set_pc(0x0000_0018);
        } else {
            // BIOS-less mode: emulate BIOS IRQ dispatcher callback.
            let handler = bus.read32(0x03FF_FFFC);
            if handler == 0 {
                self.set_pc(0x0000_0018);
                return true;
            }

            self.biosless_irq_active = true;
            self.biosless_irq_saved[0] = self.regs[0];
            self.biosless_irq_saved[1] = self.regs[1];
            self.biosless_irq_saved[2] = self.regs[2];
            self.biosless_irq_saved[3] = self.regs[3];
            self.biosless_irq_saved[4] = self.regs[12];
            self.biosless_irq_lr = self.regs[REG_LR];

            self.regs[REG_LR] = BIOSLESS_IRQ_RETURN_MAGIC;
            if (handler & 1) != 0 {
                self.cpsr |= CPSR_THUMB;
                self.set_pc(handler & !1);
            } else {
                self.cpsr &= !CPSR_THUMB;
                self.set_pc(handler & !3);
            }
        }

        true
    }
    fn exec_arm(&mut self, bus: &mut Bus, instr: u32) -> u32 {
        let cond = ((instr >> 28) & 0xF) as u8;
        if !self.condition_passed(cond) {
            return 1;
        }
        let trace_msr = trace_msr_enabled();

        // BX Rm
        if (instr & 0x0FFF_FFF0) == 0x012F_FF10 {
            let rm = (instr & 0xF) as usize;
            self.branch_exchange(self.regs[rm]);
            return 3;
        }

        // MRS Rd, CPSR
        if (instr & 0x0FBF_0FFF) == 0x010F_0000 {
            let rd = ((instr >> 12) & 0xF) as usize;
            self.regs[rd] = self.cpsr;
            return 1;
        }

        // MRS Rd, SPSR
        if (instr & 0x0FBF_0FFF) == 0x014F_0000 {
            let rd = ((instr >> 12) & 0xF) as usize;
            self.regs[rd] = self.current_spsr();
            return 1;
        }

        // MSR CPSR/SPSR fields, Rm
        // Explicit bitfield decode avoids false positives while accepting non-canonical Rd encodings.
        let msr_op = (instr >> 23) & 0x1F;
        let msr_bits_21_20 = (instr >> 20) & 0x3;

        if trace_msr && msr_op == 0b00010 && msr_bits_21_20 == 0b10 && (((instr >> 4) & 0xFF) != 0) {
            println!(
                "[msr-trace] drop-reg pc=0x{:08X} instr=0x{:08X} mode={:?} rd_bits=0x{:X} low11_4=0x{:02X}",
                self.pc().wrapping_sub(4),
                instr,
                self.mode,
                (instr >> 12) & 0xF,
                (instr >> 4) & 0xFF
            );
        }

        if msr_op == 0b00010 && msr_bits_21_20 == 0b10 && (((instr >> 4) & 0xFF) == 0) {
            let rm = (instr & 0xF) as usize;
            let field_mask = ((instr >> 16) & 0xF) as u8;
            let value = self.regs[rm];
            let write_spsr = (instr & (1 << 22)) != 0;
            if trace_msr {
                let cpsr_before = self.cpsr;
                let spsr_before = self.current_spsr();
                println!(
                    "[msr-trace] reg-before pc=0x{:08X} instr=0x{:08X} mode={:?} rd_bits=0x{:X} rm={} field=0x{:X} spsr={} value=0x{:08X} cpsr=0x{:08X} spsr_cur=0x{:08X}",
                    self.pc().wrapping_sub(4),
                    instr,
                    self.mode,
                    (instr >> 12) & 0xF,
                    rm,
                    field_mask,
                    write_spsr,
                    value,
                    cpsr_before,
                    spsr_before
                );
            }
            if write_spsr {
                self.write_spsr_fields(value, field_mask);
            } else {
                self.write_cpsr_fields(value, field_mask);
            }
            if trace_msr {
                println!(
                    "[msr-trace] reg-after  pc=0x{:08X} instr=0x{:08X} mode={:?} cpsr=0x{:08X} spsr_cur=0x{:08X}",
                    self.pc().wrapping_sub(4),
                    instr,
                    self.mode,
                    self.cpsr,
                    self.current_spsr()
                );
            }
            return 1;
        }

        // MSR CPSR/SPSR fields, #immediate
        if trace_msr && msr_op == 0b00110 && msr_bits_21_20 == 0b10 {
            println!(
                "[msr-trace] imm-hit    pc=0x{:08X} instr=0x{:08X} mode={:?} rd_bits=0x{:X}",
                self.pc().wrapping_sub(4),
                instr,
                self.mode,
                (instr >> 12) & 0xF
            );
        }
        if msr_op == 0b00110 && msr_bits_21_20 == 0b10 {
            let imm8 = instr & 0xFF;
            let rot = ((instr >> 8) & 0xF) * 2;
            let value = imm8.rotate_right(rot);
            let field_mask = ((instr >> 16) & 0xF) as u8;
            let write_spsr = (instr & (1 << 22)) != 0;
            if trace_msr {
                let cpsr_before = self.cpsr;
                let spsr_before = self.current_spsr();
                println!(
                    "[msr-trace] imm-before pc=0x{:08X} instr=0x{:08X} mode={:?} rd_bits=0x{:X} imm8=0x{:02X} rot={} field=0x{:X} spsr={} value=0x{:08X} cpsr=0x{:08X} spsr_cur=0x{:08X}",
                    self.pc().wrapping_sub(4),
                    instr,
                    self.mode,
                    (instr >> 12) & 0xF,
                    imm8,
                    rot,
                    field_mask,
                    write_spsr,
                    value,
                    cpsr_before,
                    spsr_before
                );
            }
            if write_spsr {
                self.write_spsr_fields(value, field_mask);
            } else {
                self.write_cpsr_fields(value, field_mask);
            }
            if trace_msr {
                println!(
                    "[msr-trace] imm-after  pc=0x{:08X} instr=0x{:08X} mode={:?} cpsr=0x{:08X} spsr_cur=0x{:08X}",
                    self.pc().wrapping_sub(4),
                    instr,
                    self.mode,
                    self.cpsr,
                    self.current_spsr()
                );
            }
            return 1;
        }

        // SWI
        if (instr & 0x0F00_0000) == 0x0F00_0000 {
            let swi = (instr & 0x00FF_FFFF) as u8;
            if swi == 0x04 || swi == 0x05 {
                return self.hle_swi(bus, swi);
            }
            if bus.has_bios() {
                self.software_interrupt(false);
                return 3;
            }
            return self.hle_swi(bus, swi);
        }

        // B / BL
        if ((instr >> 25) & 0b111) == 0b101 {
            let imm24 = instr & 0x00FF_FFFF;
            let signed = ((imm24 << 8) as i32) >> 6;
            if (instr & (1 << 24)) != 0 {
                self.regs[REG_LR] = self.pc();
            }
            let target = self.pc().wrapping_add(4).wrapping_add_signed(signed);
            self.set_pc(target);
            return 3;
        }

        // MUL/MLA
        if (instr & 0x0FC0_00F0) == 0x0000_0090 {
            return self.arm_multiply(instr);
        }

        // SWP / SWPB
        if (instr & 0x0FB0_0FF0) == 0x0100_0090 {
            return self.arm_swap(bus, instr);
        }

        // ¡AQUÍ ESTÁ LO NUEVO! Long Multiply (UMULL, UMLAL, SMULL, SMLAL)
        if (instr & 0x0F80_00F0) == 0x0080_0090 {
            return self.arm_long_multiply(instr);
        }

        // Halfword and signed transfer.
        if (instr & 0x0E00_0090) == 0x0000_0090 {
            return self.arm_halfword_transfer(bus, instr);
        }

        // Block data transfer LDM/STM
        if ((instr >> 25) & 0b111) == 0b100 {
            return self.arm_block_transfer(bus, instr);
        }

        // Single data transfer LDR/STR
        if ((instr >> 26) & 0b11) == 0b01 {
            return self.arm_single_data_transfer(bus, instr);
        }

        // Data processing
        if ((instr >> 26) & 0b11) == 0b00 {
            return self.arm_data_processing(instr);
        }

        self.unknown_arm(instr)
    }

    fn arm_multiply(&mut self, instr: u32) -> u32 {
        let accumulate = (instr & (1 << 21)) != 0;
        let set_flags = (instr & (1 << 20)) != 0;
        let rd = ((instr >> 16) & 0xF) as usize;
        let rn = ((instr >> 12) & 0xF) as usize;
        let rs = ((instr >> 8) & 0xF) as usize;
        let rm = (instr & 0xF) as usize;

        let mut result = self.regs[rm].wrapping_mul(self.regs[rs]);
        if accumulate {
            result = result.wrapping_add(self.regs[rn]);
        }
        self.regs[rd] = result;

        if set_flags {
            self.set_nz(result);
        }

        3
    }

    fn arm_long_multiply(&mut self, instr: u32) -> u32 {
        let is_signed = (instr & (1 << 22)) != 0; // Bit 22: 1 = Signed (SMLAL/SMULL), 0 = Unsigned
        let accumulate = (instr & (1 << 21)) != 0; // Bit 21: 1 = Add to result (SMLAL/UMLAL)
        let set_flags = (instr & (1 << 20)) != 0; // Bit 20: 1 = Set N and Z flags
        let rd_hi = ((instr >> 16) & 0xF) as usize;
        let rd_lo = ((instr >> 12) & 0xF) as usize;
        let rs = ((instr >> 8) & 0xF) as usize;
        let rm = (instr & 0xF) as usize;

        let rm_val = self.regs[rm];
        let rs_val = self.regs[rs];

        // Multiplicamos en 64 bits (con o sin signo)
        let mut result: u64 = if is_signed {
            // Convertimos a i32 para arrastrar el signo, y luego a i64 (64 bits con signo)
            ((rm_val as i32) as i64).wrapping_mul((rs_val as i32) as i64) as u64
        } else {
            // Multiplicación pura sin signo
            (rm_val as u64).wrapping_mul(rs_val as u64)
        };

        // Si es SMLAL o UMLAL, le sumamos lo que ya hubiera en [RdHi, RdLo]
        if accumulate {
            let hi = self.regs[rd_hi] as u64;
            let lo = self.regs[rd_lo] as u64;
            let acc_val = (hi << 32) | lo;
            result = result.wrapping_add(acc_val);
        }

        // Troceamos el resultado de 64 bits y lo guardamos en los dos registros
        self.regs[rd_hi] = (result >> 32) as u32;
        self.regs[rd_lo] = (result & 0xFFFF_FFFF) as u32;

        if set_flags {
            // N flag es el bit 63 (el bit de signo en 64 bits)
            self.set_flag(CPSR_N, (result >> 63) != 0);
            // Z flag se activa si TODO el resultado de 64 bits es cero
            self.set_flag(CPSR_Z, result == 0);
            
            // Nota de hardware: En ARMv4, los bits C y V se vuelven "impredecibles" 
            // tras un Long Multiply. Lo estándar en emuladores es dejarlos como estaban.
        }

        // Estas instrucciones son más lentas en el hardware real.
        // Devolvemos 5 ciclos si acumula, o 4 si solo multiplica.
        if accumulate { 5 } else { 4 }
    }

    fn arm_halfword_transfer(&mut self, bus: &mut Bus, instr: u32) -> u32 {
        let pre = (instr & (1 << 24)) != 0;
        let up = (instr & (1 << 23)) != 0;
        let immediate = (instr & (1 << 22)) != 0;
        let writeback = (instr & (1 << 21)) != 0;
        let load = (instr & (1 << 20)) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let rd = ((instr >> 12) & 0xF) as usize;
        let s = (instr & (1 << 6)) != 0;
        let h = (instr & (1 << 5)) != 0;

        let offset = if immediate {
            let hi = (instr >> 8) & 0xF;
            let lo = instr & 0xF;
            (hi << 4) | lo
        } else {
            let rm = (instr & 0xF) as usize;
            self.regs[rm]
        };

        let base = self.regs[rn];
        let offset_addr = if up {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let addr = if pre { offset_addr } else { base };

        if load {
            let value = match (s, h) {
                (false, true) => bus.read16(addr & !1) as u32,
                (true, false) => (bus.read8(addr) as i8) as i32 as u32,
                (true, true) => (bus.read16(addr & !1) as i16) as i32 as u32,
                _ => self.regs[rd],
            };
            self.regs[rd] = value;
        } else if !s && h {
            // Guardar Halfword (STRH)
            let value = if rd == REG_PC {
                self.pc().wrapping_add(4)
            } else {
                self.regs[rd]
            };
            bus.write16(addr & !1, value as u16);
        }

        let base_machacado = load && (rd == rn);
        if (writeback || !pre) && !base_machacado {
            self.regs[rn] = offset_addr;
        }
        
        // ¡OJO! Si cargamos el PC con un LDRH, hay que alinearlo
        if rd == REG_PC && load {
            self.regs[REG_PC] &= !3;
        }

        3
    }

    fn arm_swap(&mut self, bus: &mut Bus, instr: u32) -> u32 {
        let byte = (instr & (1 << 22)) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let rd = ((instr >> 12) & 0xF) as usize;
        let rm = (instr & 0xF) as usize;

        let addr = self.regs[rn];
        let src = self.regs[rm];

        let old = if byte {
            let value = bus.read8(addr) as u32;
            bus.write8(addr, src as u8);
            value
        } else {
            let shift = (addr & 3) * 8;
            let value = bus.read32(addr & !3).rotate_right(shift);
            bus.write32(addr & !3, src);
            value
        };

        self.regs[rd] = old;
        4
    }

    fn arm_data_processing(&mut self, instr: u32) -> u32 {
        let immediate = (instr & (1 << 25)) != 0;
        let opcode = ((instr >> 21) & 0xF) as u8;
        let set_flags = (instr & (1 << 20)) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let rd = ((instr >> 12) & 0xF) as usize;

        let (op2, carry_out) = if immediate {
            let imm8 = instr & 0xFF;
            let rot = ((instr >> 8) & 0xF) * 2;
            let value = imm8.rotate_right(rot);
            let carry = if rot == 0 {
                self.flag(CPSR_C)
            } else {
                (value & 0x8000_0000) != 0
            };
            (value, carry)
        } else {
            self.decode_shifted_register_operand(instr)
        };

        let rn_val = self.read_arm_reg(rn);

        match opcode {
            0x0 => {
                let result = rn_val & op2;
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry_out);
                }
            }
            0x1 => {
                let result = rn_val ^ op2;
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry_out);
                }
            }
            0x2 => {
                let (result, carry, overflow) = sub_with_flags(rn_val, op2);
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry);
                    self.set_flag(CPSR_V, overflow);
                }
            }
            0x3 => {
                let (result, carry, overflow) = sub_with_flags(op2, rn_val);
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry);
                    self.set_flag(CPSR_V, overflow);
                }
            }
            0x4 => {
                let (result, carry, overflow) = add_with_flags(rn_val, op2);
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry);
                    self.set_flag(CPSR_V, overflow);
                }
            }
            0x5 => {
                let carry_in = if self.flag(CPSR_C) { 1 } else { 0 };
                let res64 = (rn_val as u64) + (op2 as u64) + (carry_in as u64);
                let result = res64 as u32;
                let carry = res64 > 0xFFFFFFFF;
                let overflow = (!(rn_val ^ op2) & (rn_val ^ result) & 0x8000_0000) != 0;
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry);
                    self.set_flag(CPSR_V, overflow);
                }
            }
            0x6 => {
                let carry_in = if self.flag(CPSR_C) { 1 } else { 0 };
                let res64 = (rn_val as u64).wrapping_sub(op2 as u64).wrapping_sub((1 - carry_in) as u64);
                let result = res64 as u32;
                let carry = (res64 >> 32) == 0;
                let overflow = (((rn_val ^ op2) & 0x8000_0000) != 0) && (((rn_val ^ result) & 0x8000_0000) != 0);
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry);
                    self.set_flag(CPSR_V, overflow);
                }
            }
            0x7 => {
                let carry_in = if self.flag(CPSR_C) { 1 } else { 0 };
                let res64 = (op2 as u64).wrapping_sub(rn_val as u64).wrapping_sub((1 - carry_in) as u64);
                let result = res64 as u32;
                let carry = (res64 >> 32) == 0;
                let overflow = (((op2 ^ rn_val) & 0x8000_0000) != 0) && (((op2 ^ result) & 0x8000_0000) != 0);
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry);
                    self.set_flag(CPSR_V, overflow);
                }
            }
            0x8 => {
                let result = rn_val & op2;
                self.set_nz(result);
                self.set_flag(CPSR_C, carry_out);
            }
            0x9 => {
                let result = rn_val ^ op2;
                self.set_nz(result);
                self.set_flag(CPSR_C, carry_out);
            }
            0xA => {
                let (result, carry, overflow) = sub_with_flags(rn_val, op2);
                self.set_nz(result);
                self.set_flag(CPSR_C, carry);
                self.set_flag(CPSR_V, overflow);
            }
            0xB => {
                let (result, carry, overflow) = add_with_flags(rn_val, op2);
                self.set_nz(result);
                self.set_flag(CPSR_C, carry);
                self.set_flag(CPSR_V, overflow);
            }
            0xC => {
                let result = rn_val | op2;
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry_out);
                }
            }
            0xD => {
                self.regs[rd] = op2;
                if set_flags {
                    self.set_nz(op2);
                    self.set_flag(CPSR_C, carry_out);
                }
            }
            0xE => {
                let result = rn_val & !op2;
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry_out);
                }
            }
            0xF => {
                let result = !op2;
                self.regs[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    self.set_flag(CPSR_C, carry_out);
                }
            }
            _ => {}
        }

        if rd == REG_PC && set_flags {
            self.restore_cpsr_from_spsr();
            if self.is_thumb() {
                self.regs[REG_PC] &= !1;
            } else {
                self.regs[REG_PC] &= !3;
            }
        } else if rd == REG_PC {
            self.regs[REG_PC] &= !3;
        }

        1
    }

    fn arm_single_data_transfer(&mut self, bus: &mut Bus, instr: u32) -> u32 {
        let immediate_offset = (instr & (1 << 25)) == 0;
        let pre_index = (instr & (1 << 24)) != 0;
        let up = (instr & (1 << 23)) != 0;
        let byte = (instr & (1 << 22)) != 0;
        let writeback = (instr & (1 << 21)) != 0;
        let load = (instr & (1 << 20)) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let rd = ((instr >> 12) & 0xF) as usize;

        let offset = if immediate_offset {
            instr & 0xFFF
        } else {
            let rm = (instr & 0xF) as usize;
            let shift_type = (instr >> 5) & 0b11;
            let shift_imm = (instr >> 7) & 0x1F;
            let value = self.read_arm_reg(rm);
            self.apply_arm_address_shift(value, shift_type, shift_imm)
        };

        let base = self.read_arm_reg(rn);
        let offset_addr = if up {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let addr = if pre_index { offset_addr } else { base };

        if load {
            let value = if byte {
                bus.read8(addr) as u32
            } else {
                let shift = (addr & 3) * 8; 
                bus.read32(addr & !3).rotate_right(shift)
            };
            self.regs[rd] = value;
        } else {
            // ¡EL CÓDIGO DE ESCRITURA (STR) QUE FALTABA!
            let value = if rd == REG_PC {
                self.pc().wrapping_add(4) 
            } else {
                self.regs[rd]
            };

            if byte {
                bus.write8(addr, value as u8);
            } else {
                bus.write32(addr & !3, value);
            }
        }

        let base_machacado = load && (rd == rn);
        if (writeback || !pre_index) && !base_machacado {
            self.regs[rn] = offset_addr;
        }

        if rd == REG_PC && load {
            self.regs[REG_PC] &= !3;
        }

        3
    }

    fn arm_block_transfer(&mut self, bus: &mut Bus, instr: u32) -> u32 {
        let pre = (instr & (1 << 24)) != 0;
        let up = (instr & (1 << 23)) != 0;
        let writeback = (instr & (1 << 21)) != 0;
        let load = (instr & (1 << 20)) != 0;
        let rn = ((instr >> 16) & 0xF) as usize;
        let reg_list = instr & 0xFFFF;
        let s_bit = (instr & (1 << 22)) != 0; // Este es el bit '^' mágico
        let pc_in_list = (reg_list & (1 << 15)) != 0;

        let count = reg_list.count_ones();
        if count == 0 {
            return 1;
        }

        let base = self.read_arm_reg(rn);
        let mut addr = match (up, pre) {
            (true, false) => base,                               // IA
            (true, true) => base.wrapping_add(4),               // IB
            (false, false) => base.wrapping_sub((count - 1) * 4), // DA
            (false, true) => base.wrapping_sub(count * 4),      // DB
        };

        for reg in 0..16 {
            if ((reg_list >> reg) & 1) == 0 {
                continue;
            }

            if load {
                let value = bus.read32(addr & !3);
                if s_bit && !pc_in_list {
                    self.write_user_reg(reg, value);
                } else {
                    self.regs[reg] = value;
                }
            } else {
                let value = if s_bit && !pc_in_list {
                    self.read_user_reg(reg)
                } else {
                    self.read_arm_reg(reg)
                };
                bus.write32(addr & !3, value);
            }

            addr = addr.wrapping_add(4);
        }

        if writeback {
            let base_cargado_de_memoria = load && ((reg_list >> rn) & 1) != 0;
            let bytes = count * 4;
            if !base_cargado_de_memoria {
                self.regs[rn] = if up {
                    base.wrapping_add(bytes)
                } else {
                    base.wrapping_sub(bytes)
                };
            }
        }

        // ¡EL RESCATE DE LA INTERRUPCIÓN!
        if load && s_bit && pc_in_list {
            self.restore_cpsr_from_spsr();
        }

        if load && ((reg_list >> REG_PC) & 1) != 0 {
            self.regs[REG_PC] &= !3;
        }

        3 + count
    }

    fn exec_thumb(&mut self, bus: &mut Bus, instr: u16) -> u32 {
        // Format 1: move shifted register.
        if (instr & 0xF800) <= 0x1000 {
            let op = (instr >> 11) & 0x3;
            let shift = ((instr >> 6) & 0x1F) as u32;
            let rs = ((instr >> 3) & 0x7) as usize;
            let rd = (instr & 0x7) as usize;
            let val = self.regs[rs];

            let (result, carry) = match op {
                0 => { // LSL (Logical Shift Left)
                    if shift == 0 {
                        (val, self.flag(CPSR_C))
                    } else {
                        (val << shift, ((val >> (32 - shift)) & 1) != 0)
                    }
                }
                1 => { // LSR (Logical Shift Right)
                    let amount = if shift == 0 { 32 } else { shift };
                    if amount < 32 {
                        (val >> amount, ((val >> (amount - 1)) & 1) != 0)
                    } else if amount == 32 {
                        // En ARM, LSR #32 pone el registro a 0 y el bit 31 al Carry.
                        (0, (val >> 31) != 0)
                    } else {
                        (0, false)
                    }
                }
                _ => { // ASR (Arithmetic Shift Right)
                    let amount = if shift == 0 { 32 } else { shift };
                    if amount < 32 {
                        (((val as i32) >> amount) as u32, ((val >> (amount - 1)) & 1) != 0)
                    } else {
                        // En ARM, ASR #32 extiende el signo del valor original.
                        let sign = (val >> 31) != 0;
                        (if sign { 0xFFFFFFFF } else { 0 }, sign)
                    }
                }
            };

            self.regs[rd] = result;
            self.set_nz(result);
            self.set_flag(CPSR_C, carry);
            return 1;
        }

        // Format 2: add/sub register or immediate3.
        if (instr & 0xF800) == 0x1800 {
            let immediate = ((instr >> 10) & 1) != 0;
            let sub = ((instr >> 9) & 1) != 0;
            let op2 = if immediate {
                ((instr >> 6) & 0x7) as u32
            } else {
                let rs = ((instr >> 6) & 0x7) as usize;
                self.regs[rs]
            };
            let rn = ((instr >> 3) & 0x7) as usize;
            let rd = (instr & 0x7) as usize;
            let lhs = self.regs[rn];
            let (result, carry, overflow) = if sub {
                sub_with_flags(lhs, op2)
            } else {
                add_with_flags(lhs, op2)
            };
            self.regs[rd] = result;
            self.set_nz(result);
            self.set_flag(CPSR_C, carry);
            self.set_flag(CPSR_V, overflow);
            return 1;
        }

        // Format 3: MOV/CMP/ADD/SUB immediate.
        if (instr & 0xE000) == 0x2000 {
            let op = (instr >> 11) & 0x3;
            let rd = ((instr >> 8) & 0x7) as usize;
            let imm = (instr & 0x00FF) as u32;

            match op {
                0 => {
                    self.regs[rd] = imm;
                    self.set_nz(imm);
                }
                1 => {
                    let (res, c, v) = sub_with_flags(self.regs[rd], imm);
                    self.set_nz(res);
                    self.set_flag(CPSR_C, c);
                    self.set_flag(CPSR_V, v);
                }
                2 => {
                    let (res, c, v) = add_with_flags(self.regs[rd], imm);
                    self.regs[rd] = res;
                    self.set_nz(res);
                    self.set_flag(CPSR_C, c);
                    self.set_flag(CPSR_V, v);
                }
                _ => {
                    let (res, c, v) = sub_with_flags(self.regs[rd], imm);
                    self.regs[rd] = res;
                    self.set_nz(res);
                    self.set_flag(CPSR_C, c);
                    self.set_flag(CPSR_V, v);
                }
            }
            return 1;
        }

        // Format 4: ALU ops.
        if (instr & 0xFC00) == 0x4000 {
            let op = (instr >> 6) & 0xF;
            let rs = ((instr >> 3) & 0x7) as usize;
            let rd = (instr & 0x7) as usize;
            let lhs = self.regs[rd];
            let rhs = self.regs[rs];

            match op {
                0x0 => {
                    let res = lhs & rhs;
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0x1 => {
                    let res = lhs ^ rhs;
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0x2 => {
                    let amount = rhs & 0xFF;
                    let res = lhs.wrapping_shl(amount);
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0x3 => {
                    let amount = rhs & 0xFF;
                    let res = lhs.wrapping_shr(amount);
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0x4 => {
                    let amount = rhs & 0xFF;
                    let res = ((lhs as i32) >> amount) as u32;
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0x5 => {
                    let carry_in = if self.flag(CPSR_C) { 1 } else { 0 };
                    let res64 = (lhs as u64) + (rhs as u64) + (carry_in as u64);
                    let res = res64 as u32;
                    let carry = res64 > 0xFFFFFFFF;
                    let overflow = (!(lhs ^ rhs) & (lhs ^ res) & 0x8000_0000) != 0;
                    self.regs[rd] = res;
                    self.set_nz(res);
                    self.set_flag(CPSR_C, carry);
                    self.set_flag(CPSR_V, overflow);
                }
                0x6 => {
                    let carry_in = if self.flag(CPSR_C) { 1 } else { 0 };
                    let res64 = (lhs as u64).wrapping_sub(rhs as u64).wrapping_sub((1 - carry_in) as u64);
                    let res = res64 as u32;
                    let carry = (res64 >> 32) == 0;
                    let overflow = (((lhs ^ rhs) & 0x8000_0000) != 0) && (((lhs ^ res) & 0x8000_0000) != 0);
                    self.regs[rd] = res;
                    self.set_nz(res);
                    self.set_flag(CPSR_C, carry);
                    self.set_flag(CPSR_V, overflow);
                }
                0x7 => {
                    let amount = rhs & 0xFF;
                    let res = lhs.rotate_right(amount & 31);
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0x8 => {
                    let res = lhs & rhs;
                    self.set_nz(res);
                }
                0x9 => {
                    let (res, c, v) = sub_with_flags(0, rhs);
                    self.regs[rd] = res;
                    self.set_nz(res);
                    self.set_flag(CPSR_C, c);
                    self.set_flag(CPSR_V, v);
                }
                0xA => {
                    let (res, c, v) = sub_with_flags(lhs, rhs);
                    self.set_nz(res);
                    self.set_flag(CPSR_C, c);
                    self.set_flag(CPSR_V, v);
                }
                0xB => {
                    let (res, c, v) = add_with_flags(lhs, rhs);
                    self.set_nz(res);
                    self.set_flag(CPSR_C, c);
                    self.set_flag(CPSR_V, v);
                }
                0xC => {
                    let res = lhs | rhs;
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0xD => {
                    let res = lhs.wrapping_mul(rhs);
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0xE => {
                    let res = lhs & !rhs;
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                0xF => {
                    let res = !rhs;
                    self.regs[rd] = res;
                    self.set_nz(res);
                }
                _ => {}
            }
            return 1;
        }

        // Format 5: high register ops / BX
        if (instr & 0xFC00) == 0x4400 {
            let op = (instr >> 8) & 0x3;
            let h1 = ((instr >> 7) & 1) as usize;
            let h2 = ((instr >> 6) & 1) as usize;
            let rs = (((instr >> 3) & 0x7) as usize) | (h2 << 3);
            let rd = ((instr & 0x7) as usize) | (h1 << 3);
            let rhs = if rs == REG_PC {
                (self.pc().wrapping_add(2)) & !3
            } else {
                self.regs[rs]
            };

            match op {
                0 => {
                    self.regs[rd] = self.regs[rd].wrapping_add(rhs);
                    if rd == REG_PC {
                        self.regs[REG_PC] &= !3;
                    }
                }
                1 => {
                    let (res, c, v) = sub_with_flags(self.regs[rd], rhs);
                    self.set_nz(res);
                    self.set_flag(CPSR_C, c);
                    self.set_flag(CPSR_V, v);
                }
                2 => {
                    self.regs[rd] = rhs;
                    if rd == REG_PC {
                        self.regs[REG_PC] &= !1;
                    }
                }
                _ => {
                    self.branch_exchange(rhs);
                }
            }
            return 3;
        }

        // PC-relative load.
        if (instr & 0xF800) == 0x4800 {
            let rd = ((instr >> 8) & 0x7) as usize;
            let imm = ((instr & 0xFF) as u32) << 2;
            // Thumb literal uses (address of current instruction + 4), word-aligned.
            let addr = ((self.pc().wrapping_add(2)) & !3).wrapping_add(imm);
            self.regs[rd] = bus.read32(addr & !3);
            return 3;
        }

        // Register-offset load/store and sign/halfword group.
        if (instr & 0xF000) == 0x5000 {
            let op = (instr >> 9) & 0x7;
            let rm = ((instr >> 6) & 0x7) as usize;
            let rb = ((instr >> 3) & 0x7) as usize;
            let rd = (instr & 0x7) as usize;
            let addr = self.regs[rb].wrapping_add(self.regs[rm]);

            match op {
                0b000 => bus.write32(addr & !3, self.regs[rd]),
                0b001 => self.regs[rd] = bus.read32(addr & !3),
                0b010 => bus.write8(addr, self.regs[rd] as u8),
                0b011 => self.regs[rd] = bus.read8(addr) as u32,
                0b100 => bus.write16(addr & !1, self.regs[rd] as u16),
                0b101 => self.regs[rd] = (bus.read8(addr) as i8) as i32 as u32,
                0b110 => self.regs[rd] = bus.read16(addr & !1) as u32,
                _ => self.regs[rd] = (bus.read16(addr & !1) as i16) as i32 as u32,
            }
            return 2;
        }

        // Load/store immediate.
        if (instr & 0xE000) == 0x6000 {
            let op = (instr >> 11) & 0x3;
            let offset5 = ((instr >> 6) & 0x1F) as u32;
            let rb = ((instr >> 3) & 0x7) as usize;
            let rd = (instr & 0x7) as usize;
            let base = self.regs[rb];

            match op {
                0 => {
                    let addr = base.wrapping_add(offset5 << 2);
                    bus.write32(addr & !3, self.regs[rd]);
                }
                1 => {
                    let addr = base.wrapping_add(offset5 << 2);
                    self.regs[rd] = bus.read32(addr & !3);
                }
                2 => {
                    let addr = base.wrapping_add(offset5);
                    bus.write8(addr, self.regs[rd] as u8);
                }
                _ => {
                    let addr = base.wrapping_add(offset5);
                    self.regs[rd] = bus.read8(addr) as u32;
                }
            }
            return 2;
        }

        // Halfword immediate load/store.
        if (instr & 0xF000) == 0x8000 {
            let load = ((instr >> 11) & 1) != 0;
            let offset5 = ((instr >> 6) & 0x1F) as u32;
            let rb = ((instr >> 3) & 0x7) as usize;
            let rd = (instr & 0x7) as usize;
            let addr = self.regs[rb].wrapping_add(offset5 << 1);
            if load {
                self.regs[rd] = bus.read16(addr & !1) as u32;
            } else {
                bus.write16(addr & !1, self.regs[rd] as u16);
            }
            return 2;
        }

        // SP-relative load/store.
        if (instr & 0xF000) == 0x9000 {
            let load = ((instr >> 11) & 1) != 0;
            let rd = ((instr >> 8) & 0x7) as usize;
            let imm = ((instr & 0xFF) as u32) << 2;
            let addr = self.regs[REG_SP].wrapping_add(imm);
            if load {
                self.regs[rd] = bus.read32(addr & !3);
            } else {
                bus.write32(addr & !3, self.regs[rd]);
            }
            return 2;
        }

        // Add to SP / PC.
        if (instr & 0xF000) == 0xA000 {
            let sp = ((instr >> 11) & 1) != 0;
            let rd = ((instr >> 8) & 0x7) as usize;
            let imm = ((instr & 0xFF) as u32) << 2;
            let base = if sp {
                self.regs[REG_SP]
            } else {
                // ADR Rd, label in Thumb also uses (current instruction + 4), aligned.
                (self.pc().wrapping_add(2)) & !3
            };
            self.regs[rd] = base.wrapping_add(imm);
            return 1;
        }

        // Add/subtract offset to SP.
        if (instr & 0xFF00) == 0xB000 {
            let sub = ((instr >> 7) & 1) != 0;
            let imm = ((instr & 0x7F) as u32) << 2;
            if sub {
                self.regs[REG_SP] = self.regs[REG_SP].wrapping_sub(imm);
            } else {
                self.regs[REG_SP] = self.regs[REG_SP].wrapping_add(imm);
            }
            return 1;
        }

        // Multiple load/store (LDMIA/STMIA).
        if (instr & 0xF000) == 0xC000 {
            let load = ((instr >> 11) & 1) != 0;
            let rb = ((instr >> 8) & 0x7) as usize;
            let reg_list = (instr & 0xFF) as u32;
            let mut addr = self.regs[rb];

            for reg in 0..8 {
                if ((reg_list >> reg) & 1) == 0 {
                    continue;
                }
                if load {
                    self.regs[reg as usize] = bus.read32(addr & !3);
                } else {
                    bus.write32(addr & !3, self.regs[reg as usize]);
                }
                addr = addr.wrapping_add(4);
            }

            // ¡EL ARREGLO ESTÁ AQUÍ! No machacar rb si se acaba de cargar.
            let base_loaded = load && ((reg_list >> rb) & 1) != 0;
            if !base_loaded {
                self.regs[rb] = addr;
            }
            return 2;
        }

        // PUSH/POP
        if (instr & 0xF600) == 0xB400 {
            let load = ((instr >> 11) & 1) != 0;
            let r = ((instr >> 8) & 1) != 0;
            let reg_list = (instr & 0xFF) as u32;

            if load {
                let mut addr = self.regs[REG_SP];
                for reg in 0..8 {
                    if ((reg_list >> reg) & 1) != 0 {
                        self.regs[reg as usize] = bus.read32(addr & !3);
                        addr = addr.wrapping_add(4);
                    }
                }
                if r {
                    let pc_val = bus.read32(addr & !3);
                    addr = addr.wrapping_add(4);

                    // ¡CRÍTICO: El Intercambio de Modo en el POP!
                    if (pc_val & 1) != 0 {
                        self.cpsr |= CPSR_THUMB;      // Mantenemos Thumb
                        self.regs[REG_PC] = pc_val & !1;
                    } else {
                        self.cpsr &= !CPSR_THUMB;     // Cambiamos a ARM
                        self.regs[REG_PC] = pc_val & !3; // ARM se alinea a 4 bytes
                    }
                }
                self.regs[REG_SP] = addr;
            } else {
                let mut count = reg_list.count_ones();
                if r {
                    count += 1;
                }
                let mut addr = self.regs[REG_SP].wrapping_sub(count * 4);
                for reg in 0..8 {
                    if ((reg_list >> reg) & 1) != 0 {
                        bus.write32(addr & !3, self.regs[reg as usize]);
                        addr = addr.wrapping_add(4);
                    }
                }
                if r {
                    bus.write32(addr & !3, self.regs[REG_LR]);
                }
                self.regs[REG_SP] = self.regs[REG_SP].wrapping_sub(count * 4);
            }
            return 3;
        }

        // Conditional branch.
        if (instr & 0xF000) == 0xD000 && (instr & 0x0F00) != 0x0F00 {
            let cond = ((instr >> 8) & 0xF) as u8;
            if self.condition_passed(cond) {
                let imm8 = (instr & 0xFF) as i8;
                let signed = ((imm8 as i16) << 1) as i32;
                let target = self.pc().wrapping_add(2).wrapping_add_signed(signed);
                self.set_pc(target);
            }
            return 3;
        }

        // SWI (Thumb)
        if (instr & 0xFF00) == 0xDF00 {
            let swi = (instr & 0x00FF) as u8;
            if bus.has_bios() {
                self.software_interrupt(true);
                return 3;
            }
            return self.hle_swi(bus, swi);
        }

        // Unconditional branch.
        if (instr & 0xF800) == 0xE000 {
            let imm11 = (instr & 0x07FF) as i16;
            let signed = ((imm11 << 5) >> 4) as i32;
            let target = self.pc().wrapping_add(2).wrapping_add_signed(signed);
            self.set_pc(target);
            return 3;
        }

        // Long branch with link (first part / second part).
        if (instr & 0xF800) == 0xF000 {
            let imm11 = (instr & 0x07FF) as u32;
            let signed = (((imm11 << 21) as i32) >> 9) as u32;
            // First half of Thumb BL uses PC = current instruction + 4.
            self.regs[REG_LR] = self.pc().wrapping_add(2).wrapping_add(signed);
            return 2;
        }

        if (instr & 0xF800) == 0xF800 {
            let imm11 = ((instr & 0x07FF) as u32) << 1;
            let next = self.pc();
            let target = self.regs[REG_LR].wrapping_add(imm11);
            // In this core, PC already points to the next Thumb instruction.
            self.regs[REG_LR] = next | 1;
            self.set_pc(target & !1);
            return 3;
        }

        self.unknown_thumb(instr)
    }

    fn decode_shifted_register_operand(&self, instr: u32) -> (u32, bool) {
        let rm = (instr & 0xF) as usize;
        let shift_type = (instr >> 5) & 0b11;
        let register_shift = (instr & (1 << 4)) != 0;
        let value = if register_shift && rm == REG_PC {
            // ARM quirk: for register-specified shifts, Rm=PC reads as current instruction + 12.
            self.pc().wrapping_add(8)
        } else {
            self.read_arm_reg(rm)
        };

        if register_shift {
            let rs = ((instr >> 8) & 0xF) as usize;
            let amount = self.read_arm_reg(rs) & 0xFF;

            return match shift_type {
                0 => {
                    if amount == 0 {
                        (value, self.flag(CPSR_C))
                    } else if amount < 32 {
                        let res = value << amount;
                        let carry = ((value >> (32 - amount)) & 1) != 0;
                        (res, carry)
                    } else if amount == 32 {
                        (0, (value & 1) != 0)
                    } else {
                        (0, false)
                    }
                }
                1 => {
                    if amount == 0 {
                        (value, self.flag(CPSR_C))
                    } else if amount < 32 {
                        let res = value >> amount;
                        let carry = ((value >> (amount - 1)) & 1) != 0;
                        (res, carry)
                    } else if amount == 32 {
                        (0, (value >> 31) != 0)
                    } else {
                        (0, false)
                    }
                }
                2 => {
                    if amount == 0 {
                        (value, self.flag(CPSR_C))
                    } else if amount < 32 {
                        let res = ((value as i32) >> amount) as u32;
                        let carry = ((value >> (amount - 1)) & 1) != 0;
                        (res, carry)
                    } else {
                        let sign = (value >> 31) != 0;
                        (if sign { u32::MAX } else { 0 }, sign)
                    }
                }
                _ => {
                    if amount == 0 {
                        (value, self.flag(CPSR_C))
                    } else {
                        let rot = amount & 31;
                        if rot == 0 {
                            (value, (value >> 31) != 0)
                        } else {
                            let res = value.rotate_right(rot);
                            (res, (res >> 31) != 0)
                        }
                    }
                }
            };
        }

        let shift_imm = (instr >> 7) & 0x1F;

        match shift_type {
            0 => {
                if shift_imm == 0 {
                    (value, self.flag(CPSR_C))
                } else {
                    let res = value << shift_imm;
                    let carry = ((value >> (32 - shift_imm)) & 1) != 0;
                    (res, carry)
                }
            }
            1 => {
                if shift_imm == 0 {
                    let carry = (value >> 31) != 0;
                    (0, carry)
                } else {
                    let res = value >> shift_imm;
                    let carry = ((value >> (shift_imm - 1)) & 1) != 0;
                    (res, carry)
                }
            }
            2 => {
                if shift_imm == 0 {
                    let carry = (value >> 31) != 0;
                    let res = if carry { u32::MAX } else { 0 };
                    (res, carry)
                } else {
                    let res = ((value as i32) >> shift_imm) as u32;
                    let carry = ((value >> (shift_imm - 1)) & 1) != 0;
                    (res, carry)
                }
            }
            _ => {
                if shift_imm == 0 {
                    let carry_in = if self.flag(CPSR_C) { 1u32 } else { 0u32 };
                    let res = (carry_in << 31) | (value >> 1);
                    let carry = (value & 1) != 0;
                    (res, carry)
                } else {
                    let res = value.rotate_right(shift_imm);
                    let carry = (res >> 31) != 0;
                    (res, carry)
                }
            }
        }
    }

    fn apply_arm_address_shift(&self, value: u32, shift_type: u32, shift_imm: u32) -> u32 {
        match shift_type {
            0 => value.wrapping_shl(shift_imm),
            1 => {
                if shift_imm == 0 {
                    0
                } else {
                    value >> shift_imm
                }
            }
            2 => {
                if shift_imm == 0 {
                    if (value >> 31) != 0 { u32::MAX } else { 0 }
                } else {
                    ((value as i32) >> shift_imm) as u32
                }
            }
            _ => {
                if shift_imm == 0 {
                    let carry_in = if self.flag(CPSR_C) { 1u32 } else { 0u32 };
                    (carry_in << 31) | (value >> 1)
                } else {
                    value.rotate_right(shift_imm)
                }
            }
        }
    }

    fn branch_exchange(&mut self, target: u32) {
        if self.biosless_irq_active && (target & !1) == BIOSLESS_IRQ_RETURN_MAGIC {
            let return_pc = self.biosless_irq_lr.wrapping_sub(4);
            self.biosless_irq_active = false;

            self.regs[0] = self.biosless_irq_saved[0];
            self.regs[1] = self.biosless_irq_saved[1];
            self.regs[2] = self.biosless_irq_saved[2];
            self.regs[3] = self.biosless_irq_saved[3];
            self.regs[12] = self.biosless_irq_saved[4];
            self.regs[REG_LR] = self.biosless_irq_lr;

            self.restore_cpsr_from_spsr();
            if self.is_thumb() {
                self.set_pc(return_pc & !1);
            } else {
                self.set_pc(return_pc & !3);
            }
            return;
        }

        // Real BIOS eventually hands control to cartridge code; if execution is in BIOS
        // and BX resolves to 0, treat that as boot handoff to ROM entry.
        if target == 0 && self.pc() < BIOS_SIZE as u32 {
            self.force_boot_to_rom();
            return;
        }

        if (target & 1) != 0 {
            self.cpsr |= CPSR_THUMB;
            self.set_pc(target & !1);
        } else {
            self.cpsr &= !CPSR_THUMB;
            self.set_pc(target & !3);
        }
    }

    fn condition_passed(&self, cond: u8) -> bool {
        let n = self.flag(CPSR_N);
        let z = self.flag(CPSR_Z);
        let c = self.flag(CPSR_C);
        let v = self.flag(CPSR_V);

        match cond {
            0x0 => z,
            0x1 => !z,
            0x2 => c,
            0x3 => !c,
            0x4 => n,
            0x5 => !n,
            0x6 => v,
            0x7 => !v,
            0x8 => c && !z,
            0x9 => !c || z,
            0xA => n == v,
            0xB => n != v,
            0xC => !z && (n == v),
            0xD => z || (n != v),
            0xE => true,
            _ => false,
        }
    }

    fn flag(&self, bit: u32) -> bool {
        (self.cpsr & bit) != 0
    }

    fn set_flag(&mut self, bit: u32, value: bool) {
        if value {
            self.cpsr |= bit;
        } else {
            self.cpsr &= !bit;
        }
    }

    fn set_nz(&mut self, value: u32) {
        self.set_flag(CPSR_N, (value >> 31) != 0);
        self.set_flag(CPSR_Z, value == 0);
    }

    fn restore_cpsr_from_spsr(&mut self) {
        let raw_restored = match self.mode {
            CpuMode::Irq => self.spsr_irq,
            CpuMode::Supervisor => self.spsr_svc,
            CpuMode::Fiq => self.spsr_fiq,
            CpuMode::Abort => self.spsr_abt,
            CpuMode::Undefined => self.spsr_und,
            _ => self.cpsr,
        };
        let restored = sanitize_cpsr_mode_bits(raw_restored, self.cpsr & 0x1F);

        let old_mode = self.mode;
        let new_mode = mode_from_cpsr(restored);
        self.cpsr = restored;
        if new_mode != old_mode {
            self.switch_mode(new_mode);
        }
    }

    fn switch_mode(&mut self, new_mode: CpuMode) {
        if self.mode == new_mode {
            return;
        }

        match self.mode {
            CpuMode::Fiq => {
                self.banked_r8_fiq = self.regs[8];
                self.banked_r9_fiq = self.regs[9];
                self.banked_r10_fiq = self.regs[10];
                self.banked_r11_fiq = self.regs[11];
                self.banked_r12_fiq = self.regs[12];
                self.banked_sp_fiq = self.regs[REG_SP];
                self.banked_lr_fiq = self.regs[REG_LR];
                self.regs[8] = self.banked_r8_sys;
                self.regs[9] = self.banked_r9_sys;
                self.regs[10] = self.banked_r10_sys;
                self.regs[11] = self.banked_r11_sys;
                self.regs[12] = self.banked_r12_sys;
                self.regs[REG_SP] = self.banked_sp_sys;
                self.regs[REG_LR] = self.banked_lr_sys;
            }
            CpuMode::Irq => {
                self.banked_sp_irq = self.regs[REG_SP];
                self.banked_lr_irq = self.regs[REG_LR];
                self.regs[REG_SP] = self.banked_sp_sys;
                self.regs[REG_LR] = self.banked_lr_sys;
            }
            CpuMode::Supervisor => {
                self.banked_sp_svc = self.regs[REG_SP];
                self.banked_lr_svc = self.regs[REG_LR];
                self.regs[REG_SP] = self.banked_sp_sys;
                self.regs[REG_LR] = self.banked_lr_sys;
            }
            CpuMode::Abort => {
                self.banked_sp_abt = self.regs[REG_SP];
                self.banked_lr_abt = self.regs[REG_LR];
                self.regs[REG_SP] = self.banked_sp_sys;
                self.regs[REG_LR] = self.banked_lr_sys;
            }
            CpuMode::Undefined => {
                self.banked_sp_und = self.regs[REG_SP];
                self.banked_lr_und = self.regs[REG_LR];
                self.regs[REG_SP] = self.banked_sp_sys;
                self.regs[REG_LR] = self.banked_lr_sys;
            }
            _ => {}
        }

        match new_mode {
            CpuMode::Fiq => {
                self.banked_r8_sys = self.regs[8];
                self.banked_r9_sys = self.regs[9];
                self.banked_r10_sys = self.regs[10];
                self.banked_r11_sys = self.regs[11];
                self.banked_r12_sys = self.regs[12];
                self.banked_sp_sys = self.regs[REG_SP];
                self.banked_lr_sys = self.regs[REG_LR];
                self.regs[8] = self.banked_r8_fiq;
                self.regs[9] = self.banked_r9_fiq;
                self.regs[10] = self.banked_r10_fiq;
                self.regs[11] = self.banked_r11_fiq;
                self.regs[12] = self.banked_r12_fiq;
                self.regs[REG_SP] = self.banked_sp_fiq;
                self.regs[REG_LR] = self.banked_lr_fiq;
            }
            CpuMode::Irq => {
                self.banked_sp_sys = self.regs[REG_SP];
                self.banked_lr_sys = self.regs[REG_LR];
                self.regs[REG_SP] = self.banked_sp_irq;
                self.regs[REG_LR] = self.banked_lr_irq;
            }
            CpuMode::Supervisor => {
                self.banked_sp_sys = self.regs[REG_SP];
                self.banked_lr_sys = self.regs[REG_LR];
                self.regs[REG_SP] = self.banked_sp_svc;
                self.regs[REG_LR] = self.banked_lr_svc;
            }
            CpuMode::Abort => {
                self.banked_sp_sys = self.regs[REG_SP];
                self.banked_lr_sys = self.regs[REG_LR];
                self.regs[REG_SP] = self.banked_sp_abt;
                self.regs[REG_LR] = self.banked_lr_abt;
            }
            CpuMode::Undefined => {
                self.banked_sp_sys = self.regs[REG_SP];
                self.banked_lr_sys = self.regs[REG_LR];
                self.regs[REG_SP] = self.banked_sp_und;
                self.regs[REG_LR] = self.banked_lr_und;
            }
            _ => {}
        }

        self.mode = new_mode;
    }

    fn software_interrupt(&mut self, from_thumb: bool) {
        self.spsr_svc = self.cpsr;
        self.switch_mode(CpuMode::Supervisor);
        self.regs[REG_LR] = if from_thumb {
            self.pc() | 1
        } else {
            self.pc()
        };
        self.cpsr &= !CPSR_THUMB;
        self.cpsr |= CPSR_IRQ_DISABLE;
        self.set_pc(0x0000_0008);
    }

    fn read_arm_reg(&self, index: usize) -> u32 {
        if index == REG_PC {
            self.pc().wrapping_add(4)
        } else {
            self.regs[index]
        }
    }

    fn write_cpsr_fields(&mut self, value: u32, field_mask: u8) {
        let mut mask = 0u32;
        if (field_mask & 0b0001) != 0 {
            mask |= 0x0000_00FF;
        }
        if (field_mask & 0b0010) != 0 {
            mask |= 0x0000_FF00;
        }
        if (field_mask & 0b0100) != 0 {
            mask |= 0x00FF_0000;
        }
        if (field_mask & 0b1000) != 0 {
            mask |= 0xFF00_0000;
        }

        // ARMv4T: allow only architecturally relevant CPSR bits and block T writes via MSR.
        mask &= 0xF000_00DF;

        // In User mode, only flag bits are writable.
        if self.mode == CpuMode::User {
            mask &= 0xF000_0000;
        }

        let old_mode = self.mode;
        let old_mode_bits = self.cpsr & 0x1F;
        let raw = (self.cpsr & !mask) | (value & mask);
        self.cpsr = sanitize_cpsr_mode_bits(raw, old_mode_bits);
        let new_mode = mode_from_cpsr(self.cpsr);
        if new_mode != old_mode {
            self.switch_mode(new_mode);
        }
    }

    fn current_spsr(&self) -> u32 {
        match self.mode {
            CpuMode::Irq => self.spsr_irq,
            CpuMode::Supervisor => self.spsr_svc,
            CpuMode::Fiq => self.spsr_fiq,
            CpuMode::Abort => self.spsr_abt,
            CpuMode::Undefined => self.spsr_und,
            _ => 0,
        }
    }

    fn read_user_reg(&self, reg: usize) -> u32 {
        match reg {
            8 => match self.mode {
                CpuMode::Fiq => self.banked_r8_sys,
                _ => self.regs[8],
            },
            9 => match self.mode {
                CpuMode::Fiq => self.banked_r9_sys,
                _ => self.regs[9],
            },
            10 => match self.mode {
                CpuMode::Fiq => self.banked_r10_sys,
                _ => self.regs[10],
            },
            11 => match self.mode {
                CpuMode::Fiq => self.banked_r11_sys,
                _ => self.regs[11],
            },
            12 => match self.mode {
                CpuMode::Fiq => self.banked_r12_sys,
                _ => self.regs[12],
            },
            REG_SP => match self.mode {
                CpuMode::Fiq | CpuMode::Irq | CpuMode::Supervisor | CpuMode::Abort | CpuMode::Undefined => self.banked_sp_sys,
                _ => self.regs[REG_SP],
            },
            REG_LR => match self.mode {
                CpuMode::Fiq | CpuMode::Irq | CpuMode::Supervisor | CpuMode::Abort | CpuMode::Undefined => self.banked_lr_sys,
                _ => self.regs[REG_LR],
            },
            REG_PC => self.read_arm_reg(REG_PC),
            _ => self.regs[reg],
        }
    }

    fn write_user_reg(&mut self, reg: usize, value: u32) {
        match reg {
            8 => match self.mode {
                CpuMode::Fiq => self.banked_r8_sys = value,
                _ => self.regs[8] = value,
            },
            9 => match self.mode {
                CpuMode::Fiq => self.banked_r9_sys = value,
                _ => self.regs[9] = value,
            },
            10 => match self.mode {
                CpuMode::Fiq => self.banked_r10_sys = value,
                _ => self.regs[10] = value,
            },
            11 => match self.mode {
                CpuMode::Fiq => self.banked_r11_sys = value,
                _ => self.regs[11] = value,
            },
            12 => match self.mode {
                CpuMode::Fiq => self.banked_r12_sys = value,
                _ => self.regs[12] = value,
            },
            REG_SP => match self.mode {
                CpuMode::Fiq | CpuMode::Irq | CpuMode::Supervisor | CpuMode::Abort | CpuMode::Undefined => self.banked_sp_sys = value,
                _ => self.regs[REG_SP] = value,
            },
            REG_LR => match self.mode {
                CpuMode::Fiq | CpuMode::Irq | CpuMode::Supervisor | CpuMode::Abort | CpuMode::Undefined => self.banked_lr_sys = value,
                _ => self.regs[REG_LR] = value,
            },
            REG_PC => self.regs[REG_PC] = value & !3,
            _ => self.regs[reg] = value,
        }
    }

    fn write_spsr_fields(&mut self, value: u32, field_mask: u8) {
        let mut mask = 0u32;
        if (field_mask & 0b0001) != 0 {
            mask |= 0x0000_00FF;
        }
        if (field_mask & 0b0010) != 0 {
            mask |= 0x0000_FF00;
        }
        if (field_mask & 0b0100) != 0 {
            mask |= 0x00FF_0000;
        }
        if (field_mask & 0b1000) != 0 {
            mask |= 0xFF00_0000;
        }

        // ARMv4T: reserved SPSR bits [27:8] are treated as zero/ignored.
        mask &= 0xF000_00FF;

        match self.mode {
            CpuMode::Irq => {
                self.spsr_irq = (self.spsr_irq & !mask) | (value & mask);
            }
            CpuMode::Supervisor => {
                self.spsr_svc = (self.spsr_svc & !mask) | (value & mask);
            }
            CpuMode::Fiq => {
                self.spsr_fiq = (self.spsr_fiq & !mask) | (value & mask);
            }
            CpuMode::Abort => {
                self.spsr_abt = (self.spsr_abt & !mask) | (value & mask);
            }
            CpuMode::Undefined => {
                self.spsr_und = (self.spsr_und & !mask) | (value & mask);
            }
            _ => {
                // Modes without SPSR ignore MSR SPSR writes.
            }
        }
    }

    fn unknown_arm(&self, instr: u32) -> u32 {
        if strict_unknown_enabled() {
            panic!(
                "Unknown ARM instruction {:08X} at {:08X}",
                instr,
                self.pc().wrapping_sub(4)
            );
        }
        1
    }

    fn unknown_thumb(&self, instr: u16) -> u32 {
        if strict_unknown_enabled() {
            panic!(
                "Unknown Thumb instruction {:04X} at {:08X}",
                instr,
                self.pc().wrapping_sub(2)
            );
        }
        1
    }

    fn hle_swi(&mut self, bus: &mut Bus, swi: u8) -> u32 {
        match swi {
            0x00 => {
                self.set_pc(GAMEPAK_ROM_START);
            }
            0x01 => {
                let flags = self.regs[0];
                if (flags & (1 << 0)) != 0 {
                    clear_region(bus, EWRAM_START, EWRAM_SIZE);
                }
                if (flags & (1 << 1)) != 0 {
                    // BIOS RegisterRamReset preserves the last 0x200 bytes of IWRAM.
                    clear_region(bus, IWRAM_START, IWRAM_SIZE - 0x200);
                }
                if (flags & (1 << 2)) != 0 {
                    clear_region(bus, PALETTE_RAM_START, PALETTE_RAM_SIZE);
                }
                if (flags & (1 << 3)) != 0 {
                    clear_region(bus, VRAM_START, VRAM_SIZE);
                }
                if (flags & (1 << 4)) != 0 {
                    clear_region(bus, OAM_START, OAM_SIZE);
                }
            }
            0x04 => {
                let ignore_existing = self.regs[0] != 0;
                let irq_mask = self.regs[1] as u16;
                self.hle_intr_wait(bus, ignore_existing, irq_mask);
            }
            0x05 => {
                // VBlankIntrWait(ignore_existing=true, irq_mask=VBlank)
                self.hle_intr_wait(bus, true, 1);
            }
            0x06 => {
                let num = self.regs[0] as i32;
                let den = self.regs[1] as i32;
                if den == 0 {
                    self.regs[0] = 0;
                    self.regs[1] = num as u32;
                    self.regs[3] = 0;
                } else {
                    let q = num.wrapping_div(den);
                    let r = num.wrapping_rem(den);
                    self.regs[0] = q as u32;
                    self.regs[1] = r as u32;
                    self.regs[3] = q.unsigned_abs();
                }
            }
            0x0B => {
                self.hle_cpuset(bus, false);
            }
            0x0C => {
                self.hle_cpuset(bus, true);
            }
            _ => {}
        }
        6
    }

    fn hle_intr_wait(&mut self, bus: &mut Bus, ignore_existing: bool, irq_mask: u16) {
        let mut pending = bus.read_io16(super::bus::REG_IF) & irq_mask;

        if ignore_existing {
            if pending != 0 {
                bus.write_io16(super::bus::REG_IF, pending);
            }
            pending = 0;
        }

        if pending == 0 {
            self.halted = true;
        } else {
            bus.write_io16(super::bus::REG_IF, pending);
        }
    }

    fn hle_cpuset(&mut self, bus: &mut Bus, fast: bool) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
        let mode = self.regs[2];
        let fill = (mode & (1 << 24)) != 0;
        let word = (mode & (1 << 26)) != 0;

        let mut units = mode & 0x1F_FFFF;
        if fast {
            units = units.saturating_mul(8);
        }
        if units == 0 {
            return;
        }

        if word {
            let fill_value = bus.read32(src & !3);
            for _ in 0..units {
                let value = if fill {
                    fill_value
                } else {
                    let v = bus.read32(src & !3);
                    src = src.wrapping_add(4);
                    v
                };
                bus.write32(dst & !3, value);
                dst = dst.wrapping_add(4);
            }
        } else {
            let fill_value = bus.read16(src & !1);
            for _ in 0..units {
                let value = if fill {
                    fill_value
                } else {
                    let v = bus.read16(src & !1);
                    src = src.wrapping_add(2);
                    v
                };
                bus.write16(dst & !1, value);
                dst = dst.wrapping_add(2);
            }
        }

        self.regs[0] = src;
        self.regs[1] = dst;
    }
}

fn strict_unknown_enabled() -> bool {
    static STRICT: OnceLock<bool> = OnceLock::new();
    *STRICT.get_or_init(|| {
        std::env::var("GBA_STRICT_UNKNOWN")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

fn trace_msr_enabled() -> bool {
    static TRACE: OnceLock<bool> = OnceLock::new();
    *TRACE.get_or_init(|| {
        std::env::var("GBA_TRACE_MSR")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

fn mode_from_cpsr(cpsr: u32) -> CpuMode {
    match cpsr & 0x1F {
        0x10 => CpuMode::User,
        0x11 => CpuMode::Fiq,
        0x12 => CpuMode::Irq,
        0x13 => CpuMode::Supervisor,
        0x17 => CpuMode::Abort,
        0x1B => CpuMode::Undefined,
        _ => CpuMode::System,
    }
}

fn sanitize_cpsr_mode_bits(cpsr: u32, fallback_mode_bits: u32) -> u32 {
    let mode = cpsr & 0x1F;
    let valid = matches!(mode, 0x10 | 0x11 | 0x12 | 0x13 | 0x17 | 0x1B | 0x1F);
    if valid {
        cpsr
    } else {
        (cpsr & !0x1F) | (fallback_mode_bits & 0x1F)
    }
}

fn clear_region(bus: &mut Bus, start: u32, size: usize) {
    for i in 0..size {
        bus.write8(start + i as u32, 0);
    }
}

fn add_with_flags(lhs: u32, rhs: u32) -> (u32, bool, bool) {
    let (res, carry) = lhs.overflowing_add(rhs);
    let overflow = (((lhs ^ rhs) & 0x8000_0000) == 0) && (((lhs ^ res) & 0x8000_0000) != 0);
    (res, carry, overflow)
}

fn sub_with_flags(lhs: u32, rhs: u32) -> (u32, bool, bool) {
    let (res, borrow) = lhs.overflowing_sub(rhs);
    let carry = !borrow;
    let overflow = (((lhs ^ rhs) & 0x8000_0000) != 0) && (((lhs ^ res) & 0x8000_0000) != 0);
    (res, carry, overflow)
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::bus::{EWRAM_START, GAMEPAK_ROM_START};

    #[test]
    fn cpu_defaults_to_rom_start() {
        let cpu = Cpu::new();
        assert_eq!(cpu.pc(), GAMEPAK_ROM_START);
    }

    #[test]
    fn branch_instruction_changes_pc() {
        let mut cpu = Cpu::new();
        cpu.set_pc(0x0800_0008);
        let b_instr = 0xEA00_0002;
        let mut bus = Bus::new();
        let cycles = cpu.exec_arm(&mut bus, b_instr);
        assert_eq!(cycles, 3);
        assert_eq!(cpu.pc(), 0x0800_0014);
    }

    #[test]
    fn thumb_mov_add_sub_works() {
        let mut cpu = Cpu::new();
        cpu.cpsr |= CPSR_THUMB;
        let mut bus = Bus::new();

        assert_eq!(cpu.exec_thumb(&mut bus, 0x2002), 1);
        assert_eq!(cpu.read_reg(0), 2);

        assert_eq!(cpu.exec_thumb(&mut bus, 0x3003), 1);
        assert_eq!(cpu.read_reg(0), 5);

        assert_eq!(cpu.exec_thumb(&mut bus, 0x3801), 1);
        assert_eq!(cpu.read_reg(0), 4);
    }

    #[test]
    fn arm_ldr_str_roundtrip() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new();
        cpu.write_reg(0, EWRAM_START);
        cpu.write_reg(1, 0xDEAD_BEEF);

        // STR r1, [r0, #0]
        let str_instr = 0xE580_1000;
        cpu.exec_arm(&mut bus, str_instr);

        cpu.write_reg(2, 0);
        // LDR r2, [r0, #0]
        let ldr_instr = 0xE590_2000;
        cpu.exec_arm(&mut bus, ldr_instr);

        assert_eq!(cpu.read_reg(2), 0xDEAD_BEEF);
    }
}
