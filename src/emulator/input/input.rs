use crate::emulator::core::bus::Bus;

pub const BUTTON_A: u16 = 1 << 0;
pub const BUTTON_B: u16 = 1 << 1;
pub const BUTTON_SELECT: u16 = 1 << 2;
pub const BUTTON_START: u16 = 1 << 3;
pub const BUTTON_RIGHT: u16 = 1 << 4;
pub const BUTTON_LEFT: u16 = 1 << 5;
pub const BUTTON_UP: u16 = 1 << 6;
pub const BUTTON_DOWN: u16 = 1 << 7;
pub const BUTTON_R: u16 = 1 << 8;
pub const BUTTON_L: u16 = 1 << 9;

#[derive(Debug, Default)]
pub struct Input {
    held_mask: u16,
}

impl Input {
    pub fn new() -> Self {
        Self { held_mask: 0 }
    }

    pub fn set_held_mask(&mut self, held_mask: u16) {
        self.held_mask = held_mask & 0x03FF;
    }

    pub fn tick(&mut self, bus: &mut Bus) {
        bus.set_keyinput_from_held_mask(self.held_mask);
    }
}
