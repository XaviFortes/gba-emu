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
    const REG_SOUND1CNT_X_HI: u32 = REG_SOUND1CNT_X + 1;
    const REG_SOUND2CNT_L: u32 = 0x0400_0068;
    const REG_SOUND2CNT_H: u32 = 0x0400_006C;
    const REG_SOUND2CNT_H_HI: u32 = REG_SOUND2CNT_H + 1;
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
        ch1_vol: f32,
        ch2_vol: f32,
        ch1_env_samples: u32,
        ch2_env_samples: u32,
        ch1_sweep_samples: u32,
        ch1_shadow_freq: u16,
        prev_ch1_x_hi: u8,
        prev_ch2_x_hi: u8,
    }

    pub struct Apu {
        _stream: Option<OutputStream>,
        sink: Option<Sink>,
        muted: bool,
        master_volume: f32,
        sample_accum: u64,
        pending: Vec<f32>,
        psg: PsgState,
    }

    impl fmt::Debug for Apu {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Apu")
                .field("audio_device", &self.sink.is_some())
                .field("muted", &self.muted)
                .field("master_volume", &self.master_volume)
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
                        master_volume: 0.8,
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
                master_volume: 0.8,
                sample_accum: 0,
                pending: Vec::new(),
                psg: PsgState::default(),
            }
        }

        pub fn backend_info(&self) -> String {
            if self.sink.is_some() {
                "rodio/cpal default output".to_string()
            } else {
                "audio device unavailable".to_string()
            }
        }

        pub fn set_muted(&mut self, muted: bool) {
            self.muted = muted;
            if let Some(sink) = &self.sink {
                let vol = if muted { 0.0 } else { 0.35 * self.master_volume };
                sink.set_volume(vol);
            }
        }

        pub fn set_master_volume(&mut self, volume: f32) {
            self.master_volume = volume.clamp(0.0, 1.0);
            if let Some(sink) = &self.sink {
                let vol = if self.muted {
                    0.0
                } else {
                    0.35 * self.master_volume
                };
                sink.set_volume(vol);
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

            self.update_ch1_timers(bus);
            self.update_ch2_timers(bus);

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
                bus.read8(REG_SOUND1CNT_X),
                bus.read8(REG_SOUND1CNT_X + 1),
                &mut self.psg.ch1_phase,
                self.psg.ch1_vol,
            );

            let ch2 = Self::sample_square(
                bus.read8(REG_SOUND2CNT_L + 1),
                bus.read8(REG_SOUND2CNT_H),
                bus.read8(REG_SOUND2CNT_H + 1),
                &mut self.psg.ch2_phase,
                self.psg.ch2_vol,
            );

            let ch3 = Self::sample_wave(bus, &mut self.psg.ch3_phase);
            let (ds_a_raw, ds_b_raw) = bus.direct_sound_samples();
            let ds_a = ds_a_raw as f32 / 128.0;
            let ds_b = ds_b_raw as f32 / 128.0;
            let ds_a_gain = if (cnt_h & (1 << 2)) != 0 { 1.0 } else { 0.5 };
            let ds_b_gain = if (cnt_h & (1 << 3)) != 0 { 1.0 } else { 0.5 };

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

            if (cnt_h & (1 << 9)) != 0 {
                l += ds_a * ds_a_gain;
            }
            if (cnt_h & (1 << 8)) != 0 {
                r += ds_a * ds_a_gain;
            }
            if (cnt_h & (1 << 13)) != 0 {
                l += ds_b * ds_b_gain;
            }
            if (cnt_h & (1 << 12)) != 0 {
                r += ds_b * ds_b_gain;
            }

            l = (l * left_vol * psg_ratio * 0.5).clamp(-1.0, 1.0);
            r = (r * right_vol * psg_ratio * 0.5).clamp(-1.0, 1.0);
            (l, r)
        }

        fn update_ch1_timers(&mut self, bus: &Bus) {
            let env = bus.read8(REG_SOUND1CNT_H);
            let x_hi = bus.read8(REG_SOUND1CNT_X_HI);
            let x_lo = bus.read8(REG_SOUND1CNT_X);

            let triggered = (x_hi & 0x80) != 0 && self.psg.prev_ch1_x_hi != x_hi;
            self.psg.prev_ch1_x_hi = x_hi;

            if triggered {
                self.psg.ch1_vol = ((env >> 4) & 0x0F) as f32 / 15.0;
                self.psg.ch1_env_samples = 0;
                self.psg.ch1_sweep_samples = 0;
                self.psg.ch1_shadow_freq = (((x_hi as u16) & 0x07) << 8) | x_lo as u16;
            }

            let env_step = env & 0x07;
            if env_step != 0 {
                let env_period = (SAMPLE_RATE / 64).saturating_mul(env_step as u32);
                self.psg.ch1_env_samples = self.psg.ch1_env_samples.saturating_add(1);
                if self.psg.ch1_env_samples >= env_period {
                    self.psg.ch1_env_samples = 0;
                    let increase = (env & 0x08) != 0;
                    if increase {
                        self.psg.ch1_vol = (self.psg.ch1_vol + (1.0 / 15.0)).min(1.0);
                    } else {
                        self.psg.ch1_vol = (self.psg.ch1_vol - (1.0 / 15.0)).max(0.0);
                    }
                }
            }

            let sweep = bus.read8(0x0400_0060);
            let sweep_time = (sweep >> 4) & 0x07;
            let sweep_shift = sweep & 0x07;
            if sweep_time != 0 && sweep_shift != 0 {
                let sweep_period = (SAMPLE_RATE / 128).saturating_mul(sweep_time as u32);
                self.psg.ch1_sweep_samples = self.psg.ch1_sweep_samples.saturating_add(1);
                if self.psg.ch1_sweep_samples >= sweep_period {
                    self.psg.ch1_sweep_samples = 0;
                    let delta = self.psg.ch1_shadow_freq >> sweep_shift;
                    if (sweep & 0x08) != 0 {
                        self.psg.ch1_shadow_freq = self.psg.ch1_shadow_freq.saturating_sub(delta);
                    } else {
                        self.psg.ch1_shadow_freq = self.psg.ch1_shadow_freq.saturating_add(delta);
                    }
                }
            }
        }

        fn update_ch2_timers(&mut self, bus: &Bus) {
            let env = bus.read8(REG_SOUND2CNT_L);
            let x_hi = bus.read8(REG_SOUND2CNT_H_HI);
            let triggered = (x_hi & 0x80) != 0 && self.psg.prev_ch2_x_hi != x_hi;
            self.psg.prev_ch2_x_hi = x_hi;

            if triggered {
                self.psg.ch2_vol = ((env >> 4) & 0x0F) as f32 / 15.0;
                self.psg.ch2_env_samples = 0;
            }

            let env_step = env & 0x07;
            if env_step != 0 {
                let env_period = (SAMPLE_RATE / 64).saturating_mul(env_step as u32);
                self.psg.ch2_env_samples = self.psg.ch2_env_samples.saturating_add(1);
                if self.psg.ch2_env_samples >= env_period {
                    self.psg.ch2_env_samples = 0;
                    let increase = (env & 0x08) != 0;
                    if increase {
                        self.psg.ch2_vol = (self.psg.ch2_vol + (1.0 / 15.0)).min(1.0);
                    } else {
                        self.psg.ch2_vol = (self.psg.ch2_vol - (1.0 / 15.0)).max(0.0);
                    }
                }
            }
        }

        fn sample_square(
            duty_len: u8,
            freq_lo: u8,
            freq_hi: u8,
            phase: &mut f32,
            volume: f32,
        ) -> f32 {
            if volume <= 0.0 {
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
                volume
            } else {
                -volume
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

        pub fn backend_info(&self) -> String {
            "audio feature disabled".to_string()
        }

        pub fn set_muted(&mut self, _muted: bool) {}

        pub fn set_master_volume(&mut self, _volume: f32) {}

        pub fn tick(&mut self, _bus: &Bus, _cycles: u32) {}

    }
}

pub use backend::Apu;
