use crate::emulator::core::bus::Bus;

#[cfg(feature = "audio")]
mod backend {
    use std::fmt;

    use rodio::buffer::SamplesBuffer;
    use rodio::{OutputStream, Sink};

    use super::Bus;

    const CPU_CLOCK_HZ: u64 = 16_777_216;
    const SAMPLE_RATE: u32 = 44_100;

    const REG_SOUND1CNT_H: u32 = 0x0400_0062;
    const REG_SOUND1CNT_X: u32 = 0x0400_0064;
    const REG_SOUND2CNT_L: u32 = 0x0400_0068;
    const REG_SOUND2CNT_H: u32 = 0x0400_006C;
    const REG_SOUND3CNT_L: u32 = 0x0400_0070;
    const REG_SOUND3CNT_H: u32 = 0x0400_0072;
    const REG_SOUND3CNT_X: u32 = 0x0400_0074;
    const REG_SOUNDCNT_L: u32 = 0x0400_0080;
    const REG_SOUNDCNT_H: u32 = 0x0400_0082;
    const REG_SOUNDCNT_X: u32 = 0x0400_0084;
    const WAVE_RAM_START: u32 = 0x0400_0090;

    #[derive(Debug, Default)]
    struct PsgState {
        ch1_phase: f32,
        ch2_phase: f32,
        ch3_phase: f32,
    }

    pub struct Apu {
        _stream: Option<OutputStream>,
        sink: Option<Sink>,
        muted: bool,
        sample_accum: u64,
        pending: Vec<f32>,
        psg: PsgState,
    }

    impl fmt::Debug for Apu {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Apu")
                .field("audio_device", &self.sink.is_some())
                .field("muted", &self.muted)
                .field("sample_accum", &self.sample_accum)
                .field("pending_samples", &self.pending.len())
                .field("psg", &self.psg)
                .finish()
        }
    }

    impl Apu {
        pub fn new() -> Self {
            if let Ok((stream, handle)) = OutputStream::try_default() {
                if let Ok(sink) = Sink::try_new(&handle) {
                    sink.set_volume(0.35);
                    return Self {
                        _stream: Some(stream),
                        sink: Some(sink),
                        muted: false,
                        sample_accum: 0,
                        pending: Vec::with_capacity(4096),
                        psg: PsgState::default(),
                    };
                }
            }

            Self {
                _stream: None,
                sink: None,
                muted: false,
                sample_accum: 0,
                pending: Vec::new(),
                psg: PsgState::default(),
            }
        }

        pub fn set_muted(&mut self, muted: bool) {
            self.muted = muted;
            if let Some(sink) = &self.sink {
                sink.set_volume(if muted { 0.0 } else { 0.35 });
            }
        }

        pub fn tick(&mut self, bus: &Bus, cycles: u32) {
            if self.sink.is_none() {
                return;
            }

            self.sample_accum = self
                .sample_accum
                .saturating_add(cycles as u64 * SAMPLE_RATE as u64);
            let frames = self.sample_accum / CPU_CLOCK_HZ;
            self.sample_accum %= CPU_CLOCK_HZ;

            if frames == 0 {
                return;
            }

            for _ in 0..frames {
                let (l, r) = if self.muted {
                    (0.0, 0.0)
                } else {
                    self.mix_frame(bus)
                };
                self.pending.push(l);
                self.pending.push(r);
            }

            // Keep queue short to avoid latency build-up.
            if self.pending.len() >= 2048 {
                let queue_len = self.sink.as_ref().map(|s| s.len()).unwrap_or(0);
                if queue_len < 6 {
                    let data = std::mem::take(&mut self.pending);
                    if let Some(sink) = &self.sink {
                        sink.append(SamplesBuffer::new(2, SAMPLE_RATE, data));
                    }
                }
            }
        }

        fn mix_frame(&mut self, bus: &Bus) -> (f32, f32) {
            if (bus.read16(REG_SOUNDCNT_X) & 0x0080) == 0 {
                return (0.0, 0.0);
            }

            let cnt_l = bus.read16(REG_SOUNDCNT_L);
            let cnt_h = bus.read16(REG_SOUNDCNT_H);

            let right_vol = (((cnt_l & 0x0007) as f32) + 1.0) / 8.0;
            let left_vol = ((((cnt_l >> 4) & 0x0007) as f32) + 1.0) / 8.0;

            let psg_ratio = match cnt_h & 0x0003 {
                0 => 0.25,
                1 => 0.50,
                2 => 1.00,
                _ => 1.00,
            };

            let ch1 = Self::sample_square(
                bus.read8(REG_SOUND1CNT_H + 1),
                bus.read8(REG_SOUND1CNT_H),
                bus.read8(REG_SOUND1CNT_X),
                bus.read8(REG_SOUND1CNT_X + 1),
                &mut self.psg.ch1_phase,
            );

            let ch2 = Self::sample_square(
                bus.read8(REG_SOUND2CNT_L + 1),
                bus.read8(REG_SOUND2CNT_L),
                bus.read8(REG_SOUND2CNT_H),
                bus.read8(REG_SOUND2CNT_H + 1),
                &mut self.psg.ch2_phase,
            );

            let ch3 = Self::sample_wave(bus, &mut self.psg.ch3_phase);

            let mut l = 0.0f32;
            let mut r = 0.0f32;

            if (cnt_l & (1 << 12)) != 0 {
                l += ch1;
            }
            if (cnt_l & (1 << 13)) != 0 {
                l += ch2;
            }
            if (cnt_l & (1 << 14)) != 0 {
                l += ch3;
            }

            if (cnt_l & (1 << 8)) != 0 {
                r += ch1;
            }
            if (cnt_l & (1 << 9)) != 0 {
                r += ch2;
            }
            if (cnt_l & (1 << 10)) != 0 {
                r += ch3;
            }

            l = (l * left_vol * psg_ratio * 0.5).clamp(-1.0, 1.0);
            r = (r * right_vol * psg_ratio * 0.5).clamp(-1.0, 1.0);
            (l, r)
        }

        fn sample_square(
            duty_len: u8,
            envelope: u8,
            freq_lo: u8,
            freq_hi: u8,
            phase: &mut f32,
        ) -> f32 {
            let init_volume = ((envelope >> 4) & 0x0F) as f32 / 15.0;
            if init_volume <= 0.0 {
                return 0.0;
            }

            let raw = (((freq_hi as u16) & 0x07) << 8) | freq_lo as u16;
            if raw >= 2048 {
                return 0.0;
            }

            let hz = 131_072.0f32 / (2048.0f32 - raw as f32).max(1.0);
            let duty = match (duty_len >> 6) & 0x03 {
                0 => 0.125,
                1 => 0.25,
                2 => 0.5,
                _ => 0.75,
            };

            *phase += hz / SAMPLE_RATE as f32;
            if *phase >= 1.0 {
                *phase -= 1.0;
            }

            if *phase < duty {
                init_volume
            } else {
                -init_volume
            }
        }

        fn sample_wave(bus: &Bus, phase: &mut f32) -> f32 {
            let cnt_l = bus.read8(REG_SOUND3CNT_L);
            if (cnt_l & 0x80) == 0 {
                return 0.0;
            }

            let cnt_h = bus.read8(REG_SOUND3CNT_H + 1);
            let vol_shift = match (cnt_h >> 5) & 0x03 {
                0 => return 0.0,
                1 => 0,
                2 => 1,
                _ => 2,
            };

            let freq_lo = bus.read8(REG_SOUND3CNT_X);
            let freq_hi = bus.read8(REG_SOUND3CNT_X + 1);
            let raw = (((freq_hi as u16) & 0x07) << 8) | freq_lo as u16;
            if raw >= 2048 {
                return 0.0;
            }

            let wave_step_hz = 2_097_152.0f32 / (2048.0f32 - raw as f32).max(1.0);
            *phase += wave_step_hz / SAMPLE_RATE as f32;

            while *phase >= 32.0 {
                *phase -= 32.0;
            }

            let sample_index = *phase as usize;
            let byte = bus.read8(WAVE_RAM_START + (sample_index as u32 / 2));
            let nibble = if (sample_index & 1) == 0 {
                (byte >> 4) & 0x0F
            } else {
                byte & 0x0F
            };

            let mut sample = (nibble as f32 / 15.0) * 2.0 - 1.0;
            sample /= 2.0f32.powi(vol_shift);
            sample
        }

    }
}

#[cfg(not(feature = "audio"))]
mod backend {
    use super::Bus;

    #[derive(Debug, Default)]
    pub struct Apu;

    impl Apu {
        pub fn new() -> Self {
            Self
        }

        pub fn set_muted(&mut self, _muted: bool) {}

        pub fn tick(&mut self, _bus: &Bus, _cycles: u32) {}

    }
}

pub use backend::Apu;
