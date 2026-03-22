use crate::emulator::core::bus::Bus;

#[cfg(feature = "audio")]
mod backend {
    use rodio::{OutputStream, Sink, Source};

    use crate::emulator::core::bus::REG_KEYINPUT;
    use crate::emulator::input::BUTTON_A;
    use super::Bus;

    #[derive(Debug)]
    pub struct Apu {
        _stream: Option<OutputStream>,
        sink: Option<Sink>,
    }

    impl Apu {
        pub fn new() -> Self {
            if let Ok((stream, handle)) = OutputStream::try_default() {
                if let Ok(sink) = Sink::try_new(&handle) {
                    let source = rodio::source::SineWave::new(220.0)
                        .amplify(0.04)
                        .repeat_infinite();
                    sink.append(source);
                    sink.pause();
                    return Self {
                        _stream: Some(stream),
                        sink: Some(sink),
                    };
                }
            }

            Self {
                _stream: None,
                sink: None,
            }
        }

        pub fn tick(&mut self, bus: &Bus) {
            let keyinput = bus.read_io16(REG_KEYINPUT);
            let a_pressed = (keyinput & BUTTON_A) == 0;

            if let Some(sink) = &self.sink {
                if a_pressed {
                    sink.play();
                } else {
                    sink.pause();
                }
            }
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

        pub fn tick(&mut self, _bus: &Bus) {}
    }
}

pub use backend::Apu;
