#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::RefCell;

    use web_sys::{AudioContext, OscillatorType};

    thread_local! {
        static CTX: RefCell<Option<AudioContext>> = RefCell::new(None);
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

    pub fn play(freq: f32, duration: f64, osc_type: OscillatorType, peak_gain: f32) {
        with_ctx(|ctx| {
            let _ = ctx.resume(); // re-wake after tab backgrounding
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

#[cfg(target_arch = "wasm32")]
use imp::play;

// --- Public API ---

pub fn play_move() {
    #[cfg(target_arch = "wasm32")]
    play(220.0, 0.05, web_sys::OscillatorType::Square, 0.12);
}

pub fn play_rotate() {
    #[cfg(target_arch = "wasm32")]
    play(300.0, 0.05, web_sys::OscillatorType::Square, 0.12);
}

pub fn play_lock() {
    #[cfg(target_arch = "wasm32")]
    play(90.0, 0.12, web_sys::OscillatorType::Sawtooth, 0.22);
}

pub fn play_pop(chain: u32) {
    #[cfg(target_arch = "wasm32")]
    {
        let freq = (300.0 * 1.25_f32.powi(chain as i32 - 1)).min(1800.0);
        play(freq, 0.18, web_sys::OscillatorType::Sine, 0.30);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = chain;
}

pub fn play_garbage() {
    #[cfg(target_arch = "wasm32")]
    play(65.0, 0.22, web_sys::OscillatorType::Sawtooth, 0.20);
}

pub fn play_all_clear() {
    #[cfg(target_arch = "wasm32")]
    {
        play(880.0, 0.15, web_sys::OscillatorType::Sine, 0.35);
        play(1320.0, 0.35, web_sys::OscillatorType::Sine, 0.30);
    }
}
