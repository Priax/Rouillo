//! Wakes Aero's loop when something arrives between frames: a message, a response, text typed in the page.

use std::sync::OnceLock;

static WAKER: OnceLock<aero::Waker> = OnceLock::new();

pub fn init(waker: aero::Waker) {
    let _ = WAKER.set(waker);
}

pub fn wake() {
    if let Some(waker) = WAKER.get() {
        waker.wake();
    }
}
