use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use web_sys::{CompositionEvent, Event, EventTarget, HtmlInputElement, KeyboardEvent};

thread_local! {
    static TYPED: RefCell<String> = const { RefCell::new(String::new()) };
}

pub fn take_typed() -> String {
    TYPED.with(|typed| std::mem::take(&mut *typed.borrow_mut()))
}

fn typed(text: &str) {
    TYPED.with(|typed| typed.borrow_mut().push_str(text));
}

pub fn after(delay: std::time::Duration, f: impl FnOnce() + 'static) {
    let Some(window) = web_sys::window() else { return };
    let callback = Closure::once_into_js(f);
    let ms = i32::try_from(delay.as_millis()).unwrap_or(i32::MAX);
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), ms);
}

pub fn listen<E: FromWasmAbi + 'static>(target: &EventTarget, event: &str, handler: impl FnMut(E) + 'static) {
    let handler = Closure::<dyn FnMut(E)>::new(handler);
    let _ = target.add_event_listener_with_callback(event, handler.as_ref().unchecked_ref());
    handler.forget();
}

fn hidden_input() -> Option<HtmlInputElement> {
    let document = web_sys::window()?.document()?;
    let input: HtmlInputElement = document.create_element("input").ok()?.dyn_into().ok()?;
    input.set_type("text");
    for (name, value) in [
        ("autocomplete", "off"),
        ("autocapitalize", "off"),
        ("autocorrect", "off"),
        ("spellcheck", "false"),
        ("aria-hidden", "true"),
        ("tabindex", "-1"),
    ] {
        let _ = input.set_attribute(name, value);
    }
    let style = input.style();
    for (name, value) in [
        ("position", "fixed"),
        ("top", "0"),
        ("left", "0"),
        ("width", "1px"),
        ("height", "1px"),
        ("padding", "0"),
        ("border", "0"),
        ("opacity", "0"),
        ("pointer-events", "none"),
    ] {
        let _ = style.set_property(name, value);
    }
    document.body()?.append_child(&input).ok()?;
    Some(input)
}

pub fn start_text_input() {
    let (Some(window), Some(input)) = (web_sys::window(), hidden_input()) else {
        return;
    };
    let _ = input.focus();

    let composing = Rc::new(Cell::new(false));
    {
        let composing = Rc::clone(&composing);
        listen(&input, "compositionstart", move |_: Event| composing.set(true));
    }
    {
        let (composing, input) = (Rc::clone(&composing), input.clone());
        listen(&input.clone(), "compositionend", move |e: CompositionEvent| {
            composing.set(false);
            typed(&e.data().unwrap_or_default());
            input.set_value("");
        });
    }
    {
        let input = input.clone();
        listen(&input.clone(), "input", move |_: Event| {
            if !composing.get() {
                typed(&input.value());
                input.set_value("");
            }
        });
    }

    for event in ["focus", "mouseup", "touchend"] {
        let input = input.clone();
        listen(&window, event, move |_: Event| {
            let _ = input.focus();
        });
    }
    let focused = {
        let input = input.clone();
        move || {
            web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.active_element())
                .is_some_and(|e| e == *input.as_ref())
        }
    };
    listen(&window, "keydown", move |e: KeyboardEvent| {
        if e.key() == "Tab" {
            e.prevent_default();
        }
        if !focused() {
            let _ = input.focus();
            let key = e.key();
            if key.chars().count() == 1 && !e.ctrl_key() && !e.meta_key() {
                typed(&key);
                e.prevent_default();
            }
        }
    });
}
