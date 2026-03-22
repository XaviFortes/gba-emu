use std::sync::OnceLock;

use crate::emulator::core::bus::{
    Bus, IRQ_VBLANK, OAM_START, PALETTE_RAM_START, REG_DISPCNT, REG_DISPSTAT, REG_VCOUNT,
};

pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;

const SCANLINE_CYCLES: u32 = 1_232;
const TOTAL_SCANLINES: u16 = 228;
const VISIBLE_SCANLINES: u16 = 160;
const REG_BG0CNT: u32 = 0x0400_0008;
const REG_BG1CNT: u32 = 0x0400_000A;
const REG_BG2CNT: u32 = 0x0400_000C;
const REG_BG3CNT: u32 = 0x0400_000E;
const REG_BG0HOFS: u32 = 0x0400_0010;
const REG_BG0VOFS: u32 = 0x0400_0012;
const REG_BG1HOFS: u32 = 0x0400_0014;
const REG_BG1VOFS: u32 = 0x0400_0016;
const REG_BG2HOFS: u32 = 0x0400_0018;
const REG_BG2VOFS: u32 = 0x0400_001A;
const REG_BG3HOFS: u32 = 0x0400_001C;
const REG_BG3VOFS: u32 = 0x0400_001E;
const REG_BG2PA: u32 = 0x0400_0020;
const REG_BG2PB: u32 = 0x0400_0022;
const REG_BG2PC: u32 = 0x0400_0024;
const REG_BG2PD: u32 = 0x0400_0026;
const REG_BG2X: u32 = 0x0400_0028;
const REG_BG2Y: u32 = 0x0400_002C;
const REG_BG3PA: u32 = 0x0400_0030;
const REG_BG3PB: u32 = 0x0400_0032;
const REG_BG3PC: u32 = 0x0400_0034;
const REG_BG3PD: u32 = 0x0400_0036;
const REG_BG3X: u32 = 0x0400_0038;
const REG_BG3Y: u32 = 0x0400_003C;

#[derive(Debug)]
pub struct Ppu {
    scanline_cycles: u32,
    framebuffer: Vec<u32>,
    frame_ready: bool,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            scanline_cycles: 0,
            framebuffer: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT],
            frame_ready: false,
        }
    }

    pub fn tick(&mut self, cycles: u32, bus: &mut Bus, render_video: bool) {
        self.scanline_cycles = self.scanline_cycles.wrapping_add(cycles);

        while self.scanline_cycles >= SCANLINE_CYCLES {
            self.scanline_cycles -= SCANLINE_CYCLES;

            let mut vcount = bus.read_io16(REG_VCOUNT);
            if render_video && vcount < VISIBLE_SCANLINES {
                self.render_scanline(bus, vcount as usize);
            }

            vcount = (vcount + 1) % TOTAL_SCANLINES;
            bus.set_vcount(vcount);

            if vcount == VISIBLE_SCANLINES {
                self.set_vblank(bus, true);
                self.frame_ready = true;
            } else if vcount == 0 {
                self.set_vblank(bus, false);
            }
        }
    }

    pub fn take_frame_ready(&mut self) -> bool {
        let ready = self.frame_ready;
        self.frame_ready = false;
        ready
    }

    pub fn framebuffer(&self) -> &[u32] {
        &self.framebuffer
    }

    fn set_vblank(&self, bus: &mut Bus, state: bool) {
        let mut dispstat = bus.read_io16(REG_DISPSTAT);
        if state {
            dispstat |= 1;
            if trace_bios_bus_enabled() {
                println!("[bios-ppu] enter-vblank vcount={}", bus.read_io16(REG_VCOUNT));
            }
            if !bus.has_bios() {
                // BIOS-less compatibility: Emerald startup expects this heartbeat
                // byte to be refreshed by interrupt-driven callback flow.
                bus.write8(0x0300_22B4, 1);
            }
            // Start VBlank-timed DMA channels (DMAxCNT_H start timing=01).
            bus.trigger_dma_timing(0b01);
            bus.request_interrupt(IRQ_VBLANK);
        } else {
            dispstat &= !1;
            if trace_bios_bus_enabled() {
                println!("[bios-ppu] exit-vblank vcount={}", bus.read_io16(REG_VCOUNT));
            }
        }
        bus.write_io16(REG_DISPSTAT, dispstat);
    }

    fn render_scanline(&mut self, bus: &Bus, y: usize) {
        let dispcnt = bus.read_io16(REG_DISPCNT);
        let mode = dispcnt & 0b111;
        let mut priority = [4u8; SCREEN_WIDTH];
        let mut obj_owner = [false; SCREEN_WIDTH];
        match mode {
            0 => {
                self.render_mode0_scanline(bus, y, dispcnt, &mut priority, &mut obj_owner);
            }
            1 => {
                // Mode 1 has text BG0/BG1 plus affine BG2.
                self.render_mode1_scanline(bus, y, dispcnt, &mut priority, &mut obj_owner);
            }
            2 => {
                self.render_mode2_scanline(bus, y, dispcnt, &mut priority, &mut obj_owner);
            }
            3 => self.render_mode3_scanline(bus, y, dispcnt, &mut priority, &mut obj_owner),
            4 => self.render_mode4_scanline(bus, y, dispcnt, &mut priority, &mut obj_owner),
            5 => self.render_mode5_scanline(bus, y, dispcnt, &mut priority, &mut obj_owner),
            _ => self.clear_scanline(y),
        }

        // Baseline OBJ pass (normal, non-affine sprites) for all visible modes.
        if (dispcnt & (1 << 12)) != 0 {
            self.render_obj_scanline(bus, y, dispcnt, &mut priority, &mut obj_owner);
        }
    }

    fn render_mode4_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        dispcnt: u16,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let row_start = y * SCREEN_WIDTH;
        let backdrop = bgr555_to_argb8888(bus.read16(PALETTE_RAM_START));
        for (i, px) in self.framebuffer[row_start..row_start + SCREEN_WIDTH]
            .iter_mut()
            .enumerate()
        {
            *px = backdrop;
            priority[i] = 4;
            obj_owner[i] = false;
        }

        if (dispcnt & (1 << 10)) == 0 {
            return;
        }

        let bg_prio = (bus.read_io16(REG_BG2CNT) & 0b11) as u8;
        let frame_base = if (dispcnt & (1 << 4)) != 0 { 0xA000 } else { 0x0000 };
        let vram = bus.vram();

        for x in 0..SCREEN_WIDTH {
            if bg_prio > priority[x] {
                continue;
            }
            let off = frame_base + row_start + x;
            let index = vram.get(off).copied().unwrap_or(0) as usize;
            let color = bus.read16(PALETTE_RAM_START + (index * 2) as u32);
            self.framebuffer[row_start + x] = bgr555_to_argb8888(color);
            priority[x] = bg_prio;
            obj_owner[x] = false;
        }
    }

    fn render_mode2_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        dispcnt: u16,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let row_start = y * SCREEN_WIDTH;
        let backdrop = bgr555_to_argb8888(bus.read16(PALETTE_RAM_START));
        for (i, px) in self.framebuffer[row_start..row_start + SCREEN_WIDTH]
            .iter_mut()
            .enumerate()
        {
            *px = backdrop;
            priority[i] = 4;
            obj_owner[i] = false;
        }

        let mut candidates: Vec<(u16, usize)> = Vec::new();
        for bg in [2usize, 3usize] {
            if (dispcnt & (1 << (8 + bg))) == 0 {
                continue;
            }
            let cnt = bus.read_io16(bg_cnt_addr(bg));
            let priority = cnt & 0b11;
            candidates.push((priority, bg));
        }

        candidates.sort_by_key(|(priority, _)| *priority);

        for (prio, bg) in candidates {
            self.render_affine_bg_scanline(bus, y, bg, prio as u8, priority, obj_owner);
        }
    }

    fn render_mode3_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        dispcnt: u16,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let vram = bus.vram();
        let row_start = y * SCREEN_WIDTH;
        let backdrop = bgr555_to_argb8888(bus.read16(PALETTE_RAM_START));
        for (i, px) in self.framebuffer[row_start..row_start + SCREEN_WIDTH]
            .iter_mut()
            .enumerate()
        {
            *px = backdrop;
            priority[i] = 4;
            obj_owner[i] = false;
        }

        if (dispcnt & (1 << 10)) == 0 {
            return;
        }

        let bg_prio = (bus.read_io16(REG_BG2CNT) & 0b11) as u8;

        for x in 0..SCREEN_WIDTH {
            if bg_prio > priority[x] {
                continue;
            }
            let off = (row_start + x) * 2;
            let color = u16::from_le_bytes([vram[off], vram[off + 1]]);
            self.framebuffer[row_start + x] = bgr555_to_argb8888(color);
            priority[x] = bg_prio;
            obj_owner[x] = false;
        }
    }

    fn render_mode5_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        dispcnt: u16,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let row_start = y * SCREEN_WIDTH;
        let backdrop = bgr555_to_argb8888(bus.read16(PALETTE_RAM_START));
        for (i, px) in self.framebuffer[row_start..row_start + SCREEN_WIDTH]
            .iter_mut()
            .enumerate()
        {
            *px = backdrop;
            priority[i] = 4;
            obj_owner[i] = false;
        }

        if (dispcnt & (1 << 10)) == 0 {
            return;
        }

        let bg_prio = (bus.read_io16(REG_BG2CNT) & 0b11) as u8;

        // Mode 5 is a 160x128 16bpp bitmap in BG2 only.
        if y >= 128 {
            return;
        }

        let frame_base = if (dispcnt & (1 << 4)) != 0 { 0xA000 } else { 0x0000 };
        let vram = bus.vram();

        for x in 0..160usize {
            if bg_prio > priority[x] {
                continue;
            }
            let off = frame_base + ((y * 160 + x) * 2);
            if off + 1 >= vram.len() {
                continue;
            }
            let color = u16::from_le_bytes([vram[off], vram[off + 1]]);
            self.framebuffer[row_start + x] = bgr555_to_argb8888(color);
            priority[x] = bg_prio;
            obj_owner[x] = false;
        }
    }

    fn render_mode0_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        dispcnt: u16,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let row_start = y * SCREEN_WIDTH;
        let backdrop = bgr555_to_argb8888(bus.read16(PALETTE_RAM_START));
        for (i, px) in self.framebuffer[row_start..row_start + SCREEN_WIDTH]
            .iter_mut()
            .enumerate()
        {
            *px = backdrop;
            priority[i] = 4;
            obj_owner[i] = false;
        }

        let mut candidates: Vec<(u16, usize)> = Vec::new();
        for bg in 0..4 {
            if (dispcnt & (1 << (8 + bg))) == 0 {
                continue;
            }
            let cnt = bus.read_io16(bg_cnt_addr(bg));
            let priority = cnt & 0b11;
            candidates.push((priority, bg));
        }

        candidates.sort_by_key(|(priority, _)| *priority);

        for (prio, bg) in candidates {
            self.render_text_bg_scanline(bus, y, bg, prio as u8, priority, obj_owner);
        }
    }

    fn render_mode1_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        dispcnt: u16,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let row_start = y * SCREEN_WIDTH;
        let backdrop = bgr555_to_argb8888(bus.read16(PALETTE_RAM_START));
        for (i, px) in self.framebuffer[row_start..row_start + SCREEN_WIDTH]
            .iter_mut()
            .enumerate()
        {
            *px = backdrop;
            priority[i] = 4;
            obj_owner[i] = false;
        }

        let mut candidates: Vec<(u16, usize)> = Vec::new();
        for bg in 0..2 {
            if (dispcnt & (1 << (8 + bg))) == 0 {
                continue;
            }
            let cnt = bus.read_io16(bg_cnt_addr(bg));
            let priority = cnt & 0b11;
            candidates.push((priority, bg));
        }

        if (dispcnt & (1 << (8 + 2))) != 0 {
            let cnt = bus.read_io16(bg_cnt_addr(2));
            let priority = cnt & 0b11;
            candidates.push((priority, 2));
        }

        candidates.sort_by_key(|(priority, _)| *priority);

        for (prio, bg) in candidates {
            if bg == 2 {
                self.render_affine_bg_scanline(bus, y, 2, prio as u8, priority, obj_owner);
            } else {
                self.render_text_bg_scanline(bus, y, bg, prio as u8, priority, obj_owner);
            }
        }
    }

    fn render_affine_bg_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        bg: usize,
        bg_prio: u8,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let (pa_addr, pb_addr, pc_addr, pd_addr, x_addr, y_addr) = match bg {
            2 => (REG_BG2PA, REG_BG2PB, REG_BG2PC, REG_BG2PD, REG_BG2X, REG_BG2Y),
            _ => (REG_BG3PA, REG_BG3PB, REG_BG3PC, REG_BG3PD, REG_BG3X, REG_BG3Y),
        };

        let bgcnt = bus.read_io16(bg_cnt_addr(bg));
        let char_base_block = ((bgcnt >> 2) & 0x3) as usize;
        let map_base_block = ((bgcnt >> 8) & 0x1F) as usize;
        let wrap = (bgcnt & (1 << 13)) != 0;
        let size = ((bgcnt >> 14) & 0x3) as usize;
        let bg_size = match size {
            0 => 128usize,
            1 => 256usize,
            2 => 512usize,
            _ => 1024usize,
        };

        let pa = bus.read_io16(pa_addr) as i16 as i32;
        let pb = bus.read_io16(pb_addr) as i16 as i32;
        let pc = bus.read_io16(pc_addr) as i16 as i32;
        let pd = bus.read_io16(pd_addr) as i16 as i32;
        let ref_x = read_affine_ref(bus, x_addr);
        let ref_y = read_affine_ref(bus, y_addr);

        let map_base = map_base_block * 0x800;
        let char_base = char_base_block * 0x4000;
        let tiles_per_row = bg_size / 8;
        let row_start = y * SCREEN_WIDTH;
        let y_i32 = y as i32;
        let vram = bus.vram();

        let mut cur_x = ref_x + pb * y_i32;
        let mut cur_y = ref_y + pd * y_i32;

        for x in 0..SCREEN_WIDTH {
            if bg_prio > priority[x] {
                cur_x = cur_x.wrapping_add(pa);
                cur_y = cur_y.wrapping_add(pc);
                continue;
            }

            let src_x = cur_x >> 8;
            let src_y = cur_y >> 8;

            cur_x = cur_x.wrapping_add(pa);
            cur_y = cur_y.wrapping_add(pc);

            let (sx, sy) = if wrap {
                (
                    src_x.rem_euclid(bg_size as i32) as usize,
                    src_y.rem_euclid(bg_size as i32) as usize,
                )
            } else {
                if src_x < 0 || src_y < 0 || src_x >= bg_size as i32 || src_y >= bg_size as i32 {
                    continue;
                }
                (src_x as usize, src_y as usize)
            };

            let tile_x = sx / 8;
            let tile_y = sy / 8;
            let map_index = tile_y * tiles_per_row + tile_x;
            let tile_off = map_base + map_index;
            let tile_id = match vram.get(tile_off) {
                Some(v) => *v as usize,
                None => continue,
            };

            let in_tile_x = sx & 7;
            let in_tile_y = sy & 7;
            let pixel_off = char_base + tile_id * 64 + in_tile_y * 8 + in_tile_x;
            let color_index = match vram.get(pixel_off) {
                Some(v) => *v as usize,
                None => continue,
            };

            if color_index == 0 {
                continue;
            }

            let color = bus.read16(PALETTE_RAM_START + (color_index * 2) as u32);
            self.framebuffer[row_start + x] = bgr555_to_argb8888(color);
            priority[x] = bg_prio;
            obj_owner[x] = false;
        }
    }

    fn render_text_bg_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        bg: usize,
        bg_prio: u8,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let bgcnt = bus.read_io16(bg_cnt_addr(bg));
        let hofs = (bus.read_io16(bg_hofs_addr(bg)) & 0x1FF) as usize;
        let vofs = (bus.read_io16(bg_vofs_addr(bg)) & 0x1FF) as usize;
        let char_base_block = ((bgcnt >> 2) & 0x3) as usize;
        let map_base_block = ((bgcnt >> 8) & 0x1F) as usize;
        let is_8bpp = (bgcnt & (1 << 7)) != 0;
        let size = ((bgcnt >> 14) & 0x3) as usize;

        let (width_tiles, height_tiles) = match size {
            0 => (32usize, 32usize),
            1 => (64usize, 32usize),
            2 => (32usize, 64usize),
            _ => (64usize, 64usize),
        };

        let map_x_base = hofs;
        let map_y = (y + vofs) % (height_tiles * 8);
        let tile_y = (map_y / 8) % height_tiles;
        let in_tile_y = map_y % 8;

        let row_start = y * SCREEN_WIDTH;
        let vram = bus.vram();

        for x in 0..SCREEN_WIDTH {
            if bg_prio > priority[x] {
                continue;
            }
            let map_x = (map_x_base + x) % (width_tiles * 8);
            let tile_x = (map_x / 8) % width_tiles;
            let in_tile_x = map_x % 8;

            let screen_block_x = tile_x / 32;
            let screen_block_y = tile_y / 32;
            let screen_block_index = map_base_block + screen_block_x + screen_block_y * (width_tiles / 32);
            let screen_base = screen_block_index * 0x800;
            let map_index = (tile_y % 32) * 32 + (tile_x % 32);
            let entry_off = screen_base + map_index * 2;

            if entry_off + 1 >= vram.len() {
                self.framebuffer[row_start + x] = 0xFF00_0000;
                continue;
            }

            let entry = u16::from_le_bytes([vram[entry_off], vram[entry_off + 1]]);
            let tile_id = (entry & 0x03FF) as usize;
            let hflip = (entry & (1 << 10)) != 0;
            let vflip = (entry & (1 << 11)) != 0;
            let palette_bank = ((entry >> 12) & 0xF) as usize;

            let px = if hflip { 7 - in_tile_x } else { in_tile_x };
            let py = if vflip { 7 - in_tile_y } else { in_tile_y };

            let color_index = if is_8bpp {
                let tile_base = char_base_block * 0x4000 + tile_id * 64;
                let off = tile_base + py * 8 + px;
                vram.get(off).copied().unwrap_or(0)
            } else {
                let tile_base = char_base_block * 0x4000 + tile_id * 32;
                let off = tile_base + py * 4 + (px / 2);
                let byte = vram.get(off).copied().unwrap_or(0);
                if (px & 1) == 0 {
                    byte & 0x0F
                } else {
                    byte >> 4
                }
            } as usize;

            let palette_index = if is_8bpp {
                color_index
            } else {
                palette_bank * 16 + color_index
            };

            if color_index == 0 {
                continue;
            }

            let color = bus.read16(PALETTE_RAM_START + (palette_index * 2) as u32);
            self.framebuffer[row_start + x] = bgr555_to_argb8888(color);
            priority[x] = bg_prio;
            obj_owner[x] = false;
        }
    }

    fn clear_scanline(&mut self, y: usize) {
        let row_start = y * SCREEN_WIDTH;
        let row_end = row_start + SCREEN_WIDTH;
        for px in &mut self.framebuffer[row_start..row_end] {
            *px = 0xFF10_1010;
        }
    }

    fn render_obj_scanline(
        &mut self,
        bus: &Bus,
        y: usize,
        dispcnt: u16,
        priority: &mut [u8; SCREEN_WIDTH],
        obj_owner: &mut [bool; SCREEN_WIDTH],
    ) {
        let one_dim_obj = (dispcnt & (1 << 6)) != 0;
        let vram = bus.vram();
        let row_start = y * SCREEN_WIDTH;
        let y_u16 = y as u16;

        for obj in 0..128usize {
            let base = OAM_START + (obj as u32) * 8;
            let attr0 = bus.read16(base);
            let attr1 = bus.read16(base + 2);
            let attr2 = bus.read16(base + 4);

            let obj_mode = (attr0 >> 8) & 0x3;
            let is_affine = (attr0 & (1 << 8)) != 0;
            let affine_double = is_affine && (attr0 & (1 << 9)) != 0;
            let mosaic = (attr0 & (1 << 12)) != 0;
            let is_8bpp = (attr0 & (1 << 13)) != 0;
            let shape = (attr0 >> 14) & 0x3;
            let size = (attr1 >> 14) & 0x3;

            // Skip prohibited/object-window and mosaic for now.
            if obj_mode == 0b10 || shape == 0b11 || mosaic {
                continue;
            }

            let (width, height) = match obj_dimensions(shape, size) {
                Some(v) => v,
                None => continue,
            };

            let obj_y = attr0 & 0x00FF;
            let mut obj_x = attr1 & 0x01FF;
            if obj_x >= 240 {
                obj_x = obj_x.wrapping_sub(512);
            }

            let draw_width = if affine_double { width * 2 } else { width };
            let draw_height = if affine_double { height * 2 } else { height };

            if !scanline_hits_obj(y_u16, obj_y, draw_height as u16) {
                continue;
            }

            let tile_index = (attr2 & 0x03FF) as usize;
            let obj_prio = ((attr2 >> 10) & 0x3) as u8;
            let palette_bank = ((attr2 >> 12) & 0xF) as usize;
            let hflip = (attr1 & (1 << 12)) != 0;
            let vflip = (attr1 & (1 << 13)) != 0;
            let units_per_tile = if is_8bpp { 2usize } else { 1usize };
            let row_units = if one_dim_obj {
                (width / 8) * units_per_tile
            } else {
                32usize
            };

            let aff_param_idx = ((attr1 >> 9) & 0x1F) as usize;
            let (pa, pb, pc, pd) = if is_affine {
                read_obj_affine_params(bus, aff_param_idx)
            } else {
                (0, 0, 0, 0)
            };

            let y_in_draw = y_u16.wrapping_sub(obj_y) as usize;

            for sx in 0..draw_width {
                let screen_x = (obj_x as i32) + (sx as i32);
                if !(0..SCREEN_WIDTH as i32).contains(&screen_x) {
                    continue;
                }

                let (in_obj_x, in_obj_y) = if is_affine {
                    let cx_draw = (draw_width / 2) as i32;
                    let cy_draw = (draw_height / 2) as i32;
                    let x_rel = sx as i32 - cx_draw;
                    let y_rel = y_in_draw as i32 - cy_draw;

                    let src_x = ((pa as i32 * x_rel + pb as i32 * y_rel) >> 8) + (width as i32 / 2);
                    let src_y = ((pc as i32 * x_rel + pd as i32 * y_rel) >> 8) + (height as i32 / 2);

                    if src_x < 0 || src_y < 0 || src_x >= width as i32 || src_y >= height as i32 {
                        continue;
                    }

                    (src_x as usize, src_y as usize)
                } else {
                    let mut x_src = sx;
                    let mut y_src = y_in_draw;
                    if hflip {
                        x_src = width - 1 - x_src;
                    }
                    if vflip {
                        y_src = height - 1 - y_src;
                    }
                    (x_src, y_src)
                };

                let tile_x = in_obj_x / 8;
                let tile_y = in_obj_y / 8;
                let pixel_x = in_obj_x % 8;
                let pixel_y = in_obj_y % 8;

                let tile_units = tile_index
                    .wrapping_add(tile_y * row_units)
                    .wrapping_add(tile_x * units_per_tile);
                let tile_base = 0x1_0000usize + tile_units * 32;

                let color_index = if is_8bpp {
                    let off = tile_base + pixel_y * 8 + pixel_x;
                    vram.get(off).copied().unwrap_or(0) as usize
                } else {
                    let off = tile_base + pixel_y * 4 + (pixel_x / 2);
                    let byte = vram.get(off).copied().unwrap_or(0);
                    if (pixel_x & 1) == 0 {
                        (byte & 0x0F) as usize
                    } else {
                        (byte >> 4) as usize
                    }
                };

                if color_index == 0 {
                    continue;
                }

                let px = screen_x as usize;
                let can_draw = obj_prio < priority[px]
                    || (obj_prio == priority[px] && !obj_owner[px]);
                if !can_draw {
                    continue;
                }

                let palette_index = if is_8bpp {
                    0x100 + color_index
                } else {
                    0x100 + palette_bank * 16 + color_index
                };
                let color = bus.read16(PALETTE_RAM_START + (palette_index * 2) as u32);

                self.framebuffer[row_start + px] = bgr555_to_argb8888(color);
                priority[px] = obj_prio;
                obj_owner[px] = true;
            }
        }
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

fn bgr555_to_argb8888(color: u16) -> u32 {
    let b = ((color >> 10) & 0x1F) as u32;
    let g = ((color >> 5) & 0x1F) as u32;
    let r = (color & 0x1F) as u32;

    let r8 = (r * 255) / 31;
    let g8 = (g * 255) / 31;
    let b8 = (b * 255) / 31;

    0xFF00_0000 | (r8 << 16) | (g8 << 8) | b8
}

fn bg_cnt_addr(bg: usize) -> u32 {
    match bg {
        0 => REG_BG0CNT,
        1 => REG_BG1CNT,
        2 => REG_BG2CNT,
        _ => REG_BG3CNT,
    }
}

fn bg_hofs_addr(bg: usize) -> u32 {
    match bg {
        0 => REG_BG0HOFS,
        1 => REG_BG1HOFS,
        2 => REG_BG2HOFS,
        _ => REG_BG3HOFS,
    }
}

fn bg_vofs_addr(bg: usize) -> u32 {
    match bg {
        0 => REG_BG0VOFS,
        1 => REG_BG1VOFS,
        2 => REG_BG2VOFS,
        _ => REG_BG3VOFS,
    }
}

fn read_affine_ref(bus: &Bus, addr: u32) -> i32 {
    let raw = bus.read32(addr);
    // BGxX/BGxY are signed 28-bit fixed-point values (s19.8) in IO.
    ((raw << 4) as i32) >> 4
}

fn obj_dimensions(shape: u16, size: u16) -> Option<(usize, usize)> {
    match (shape, size) {
        (0, 0) => Some((8, 8)),
        (0, 1) => Some((16, 16)),
        (0, 2) => Some((32, 32)),
        (0, 3) => Some((64, 64)),
        (1, 0) => Some((16, 8)),
        (1, 1) => Some((32, 8)),
        (1, 2) => Some((32, 16)),
        (1, 3) => Some((64, 32)),
        (2, 0) => Some((8, 16)),
        (2, 1) => Some((8, 32)),
        (2, 2) => Some((16, 32)),
        (2, 3) => Some((32, 64)),
        _ => None,
    }
}

fn scanline_hits_obj(scanline: u16, obj_y: u16, height: u16) -> bool {
    let end = obj_y.wrapping_add(height);
    if end >= 256 {
        scanline >= obj_y || scanline < (end & 0x00FF)
    } else {
        scanline >= obj_y && scanline < end
    }
}

fn read_obj_affine_params(bus: &Bus, param_index: usize) -> (i16, i16, i16, i16) {
    let base = OAM_START + (param_index as u32) * 32;
    let pa = bus.read16(base + 0x06) as i16;
    let pb = bus.read16(base + 0x0E) as i16;
    let pc = bus.read16(base + 0x16) as i16;
    let pd = bus.read16(base + 0x1E) as i16;
    (pa, pb, pc, pd)
}
