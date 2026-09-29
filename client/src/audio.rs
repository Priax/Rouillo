#[derive(Clone, Copy)]
enum Wave {
    Sine,
    Square,
    Sawtooth,
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::RefCell;

    use web_sys::{AudioContext, OscillatorType};

    use super::Wave;

    thread_local! {
        static CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    }

    fn with_ctx<F: FnOnce(&AudioContext)>(f: F) {
        CTX.with(|cell| {
            let mut borrow = cell.borrow_mut();
            if borrow.is_none() {
                *borrow = AudioContext::new().ok();
            }
            if let Some(ref ctx) = *borrow {
                f(ctx);
            }
        });
    }

    pub fn play(freq: f32, duration: f64, wave: Wave, peak_gain: f32) {
        let osc_type = match wave {
            Wave::Sine => OscillatorType::Sine,
            Wave::Square => OscillatorType::Square,
            Wave::Sawtooth => OscillatorType::Sawtooth,
        };
        with_ctx(|ctx| {
            let _ = ctx.resume();
            let Ok(osc) = ctx.create_oscillator() else {
                return;
            };
            let Ok(gain) = ctx.create_gain() else { return };
            let t = ctx.current_time();

            osc.set_type(osc_type);
            osc.frequency().set_value(freq);
            gain.gain().set_value(peak_gain);
            let _ = gain.gain().linear_ramp_to_value_at_time(0.0, t + duration);

            let _ = osc.connect_with_audio_node(&gain);
            let _ = gain.connect_with_audio_node(&ctx.destination());

            let _ = osc.start();
            let _ = osc.stop_with_when(t + duration + 0.01);
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::cell::RefCell;
    use std::num::NonZero;
    use std::time::Duration;

    use rodio::{ChannelCount, MixerDeviceSink, SampleRate, Source};

    use super::Wave;

    const RATE: u32 = 48_000;

    thread_local! {
        static SINK: RefCell<Option<Option<MixerDeviceSink>>> = const { RefCell::new(None) };
    }

    struct Tone {
        wave: Wave,
        freq: f32,
        peak_gain: f32,
        sample: u32,
        samples: u32,
    }

    impl Iterator for Tone {
        type Item = rodio::Sample;

        fn next(&mut self) -> Option<Self::Item> {
            if self.sample >= self.samples {
                return None;
            }
            let t = self.sample as f32 / RATE as f32;
            let phase = (self.freq * t).fract();
            let value = match self.wave {
                Wave::Sine => (phase * std::f32::consts::TAU).sin(),
                Wave::Square => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Sawtooth => 2.0 * phase - 1.0,
            };
            let envelope = 1.0 - self.sample as f32 / self.samples as f32;
            self.sample += 1;
            Some(value * self.peak_gain * envelope)
        }
    }

    impl Source for Tone {
        fn current_span_len(&self) -> Option<usize> {
            None
        }

        fn channels(&self) -> ChannelCount {
            NonZero::<u16>::MIN
        }

        fn sample_rate(&self) -> SampleRate {
            NonZero::new(RATE).expect("the rate is not zero")
        }

        fn total_duration(&self) -> Option<Duration> {
            Some(Duration::from_secs_f64(self.samples as f64 / RATE as f64))
        }
    }

    pub fn play(freq: f32, duration: f64, wave: Wave, peak_gain: f32) {
        if cfg!(test) {
            return;
        }
        SINK.with(|cell| {
            let mut sink = cell.borrow_mut();
            let sink = sink.get_or_insert_with(|| {
                let mut opened = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
                opened.log_on_drop(false);
                Some(opened)
            });
            if let Some(sink) = sink {
                sink.mixer().add(Tone {
                    wave,
                    freq,
                    peak_gain,
                    sample: 0,
                    samples: (duration * RATE as f64) as u32,
                });
            }
        });
    }
}

use imp::play;

// --- Public API ---

pub fn play_move() {
    play(220.0, 0.05, Wave::Square, 0.12);
}

pub fn play_rotate() {
    play(300.0, 0.05, Wave::Square, 0.12);
}

pub fn play_lock() {
    play(90.0, 0.12, Wave::Sawtooth, 0.22);
}

pub fn play_pop(chain: u32) {
    let freq = (300.0 * 1.25_f32.powi(chain as i32 - 1)).min(1800.0);
    play(freq, 0.18, Wave::Sine, 0.30);
}

pub fn play_garbage() {
    play(65.0, 0.22, Wave::Sawtooth, 0.20);
}

/// The pointer arriving over a button: a faint, high tick.
pub fn play_ui_hover() {
    play(1400.0, 0.03, Wave::Sine, 0.04);
}

pub fn play_ui_click() {
    play(700.0, 0.07, Wave::Sine, 0.12);
}

pub fn play_all_clear() {
    play(880.0, 0.15, Wave::Sine, 0.35);
    play(1320.0, 0.35, Wave::Sine, 0.30);
}
