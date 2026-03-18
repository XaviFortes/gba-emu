use super::bus::Bus;

#[derive(Debug, Default)]
pub struct Timers;

impl Timers {
    pub fn new() -> Self {
        Self
    }

    pub fn tick(&mut self, bus: &mut Bus, cycles: u32) {
        bus.tick_timers(cycles);
    }
}
