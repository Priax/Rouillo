use std::cell::{Cell, RefCell};

use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use web_sys::{Event, EventTarget, FileReader, HtmlInputElement};

use crate::picker::{Picked, MAX_BYTES, TOO_HEAVY};

thread_local! {
    static BACK: Cell<bool> = const { Cell::new(false) };
    static AT_ROOT: Cell<bool> = const { Cell::new(true) };
    static PICKED: RefCell<Option<Picked>> = const { RefCell::new(None) };
}

fn picked(result: Picked) {
    PICKED.with(|p| *p.borrow_mut() = Some(result));
}

pub fn take_picked() -> Option<Picked> {
    PICKED.with(|p| p.borrow_mut().take())
}

/// Opens the browser's file chooser. It must run soon after a click: browsers
/// only open it in answer to the player.
pub fn pick_file() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(input) = document.create_element("input").ok().and_then(|e| e.dyn_into::<HtmlInputElement>().ok()) else {
        return;
    };
    input.set_type("file");
    input.set_accept("image/png,image/jpeg,image/webp,image/gif");
    let chooser = input.clone();
    listen(&input, "change", move |_: Event| {
        let Some(file) = chooser.files().and_then(|f| f.get(0)) else {
            picked(Picked::Cancelled);
            return;
        };
        if file.size() > MAX_BYTES as f64 {
            picked(Picked::Failed(TOO_HEAVY));
            return;
        }
        let Ok(reader) = FileReader::new() else { return };
        let done = reader.clone();
        listen(&reader, "load", move |_: Event| {
            let bytes = done
                .result()
                .map(|buf| js_sys::Uint8Array::new(&buf).to_vec())
                .map_or(Picked::Failed("Fichier illisible."), Picked::File);
            picked(bytes);
        });
        let _ = reader.read_as_array_buffer(&file);
    });
    input.click();
}

/// Makes a phone's back button (and the browser's) go back one screen: a spare
/// history entry catches it. On the main menu it leaves the site as usual.
pub fn start_back_button() {
    let Some(window) = web_sys::window() else { return };
    let Ok(history) = window.history() else { return };
    let _ = history.push_state(&JsValue::NULL, "");
    listen(&window, "popstate", move |_: Event| {
        if AT_ROOT.with(Cell::get) {
            let _ = history.back();
        } else {
            BACK.with(|b| b.set(true));
            let _ = history.push_state(&JsValue::NULL, "");
        }
    });
}

/// Whether the current screen is the one a back press leaves the site from.
pub fn set_at_root(at_root: bool) {
    AT_ROOT.with(|r| r.set(at_root));
}

pub fn take_back() -> bool {
    BACK.with(Cell::take)
}

pub fn after(delay: std::time::Duration, f: impl FnOnce() + 'static) {
    let Some(window) = web_sys::window() else { return };
    let callback = Closure::once_into_js(move || {
        f();
        crate::wake::wake();
    });
    let ms = i32::try_from(delay.as_millis()).unwrap_or(i32::MAX);
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), ms);
}

/// Every page event wakes the game's loop, which reads what it left.
pub fn listen<E: FromWasmAbi + 'static>(target: &EventTarget, event: &str, mut handler: impl FnMut(E) + 'static) {
    let handler = Closure::<dyn FnMut(E)>::new(move |event| {
        handler(event);
        crate::wake::wake();
    });
    let _ = target.add_event_listener_with_callback(event, handler.as_ref().unchecked_ref());
    handler.forget();
}
