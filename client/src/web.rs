use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use web_sys::{ClipboardEvent, CompositionEvent, Event, EventTarget, FileReader, HtmlInputElement, KeyboardEvent};

use crate::picker::{Picked, MAX_BYTES, TOO_HEAVY};
use crate::ui::Shortcuts;

thread_local! {
    static TYPED: RefCell<String> = const { RefCell::new(String::new()) };
    static ERASES: Cell<u32> = const { Cell::new(0) };
    static BACK: Cell<bool> = const { Cell::new(false) };
    static AT_ROOT: Cell<bool> = const { Cell::new(true) };
    /// A Backspace key already counted, whose removal of input text must not
    /// count again.
    static BACKSPACE_DOWN: Cell<bool> = const { Cell::new(false) };
    static WANT_TEXT: Cell<bool> = const { Cell::new(true) };
    static INPUT: RefCell<Option<HtmlInputElement>> = const { RefCell::new(None) };
    static SHORTCUTS: Cell<Shortcuts> = Cell::new(Shortcuts::default());
    static SELECTION: RefCell<String> = const { RefCell::new(String::new()) };
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
    let Some(input) = document
        .create_element("input")
        .ok()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    else {
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

pub fn take_shortcuts() -> Shortcuts {
    SHORTCUTS.with(Cell::take)
}

/// What the focused field has selected, for the browser's copy and cut.
pub fn set_selection(text: &str) {
    SELECTION.with(|s| {
        let mut s = s.borrow_mut();
        if *s != text {
            text.clone_into(&mut s);
        }
    });
}

fn give_selection(e: &ClipboardEvent) -> bool {
    let text = SELECTION.with(|s| s.borrow().clone());
    if text.is_empty() {
        return false;
    }
    if let Some(data) = e.clipboard_data() {
        let _ = data.set_data("text/plain", &text);
        e.prevent_default();
    }
    true
}

/// Whether the screen has a text field: only then does the hidden input keep
/// the focus, since a focused input brings up a phone's on-screen keyboard.
pub fn want_text(on: bool) {
    if WANT_TEXT.with(|w| w.replace(on)) == on {
        return;
    }
    INPUT.with(|input| {
        if let Some(input) = input.borrow().as_ref() {
            let _ = if on { input.focus() } else { input.blur() };
        }
    });
}

fn wanted() -> bool {
    WANT_TEXT.with(Cell::get)
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

pub fn take_erases() -> u32 {
    ERASES.with(Cell::take)
}

fn erased(n: u32) {
    ERASES.with(|e| e.set(e.get() + n));
}

/// What the hidden input holds between keys: a phone's keyboard sends no key
/// for Backspace, only removes text, so there must be text to remove.
const SENTINEL: &str = "~~~~";

fn reset(input: &HtmlInputElement) {
    input.set_value(SENTINEL);
    let end = SENTINEL.len() as u32;
    let _ = input.set_selection_range(end, end);
}

/// Splits what the hidden input holds into backspaces (sentinel characters
/// gone) and the text typed after it.
fn read_input(value: &str) -> (u32, &str) {
    let kept = value.chars().zip(SENTINEL.chars()).take_while(|(a, b)| a == b).count();
    let erased = SENTINEL.chars().count() - kept;
    (erased as u32, &value[kept..])
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
    reset(&input);
    let _ = input.focus();
    INPUT.with(|slot| *slot.borrow_mut() = Some(input.clone()));

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
            reset(&input);
        });
    }
    {
        let input = input.clone();
        listen(&input.clone(), "input", move |_: Event| {
            if !composing.get() {
                let value = input.value();
                let (gone, text) = read_input(&value);
                let counted = BACKSPACE_DOWN.with(Cell::take);
                erased(gone.saturating_sub(u32::from(counted)));
                typed(text);
                reset(&input);
            }
        });
    }

    for event in ["focus", "mouseup", "touchend"] {
        let input = input.clone();
        listen(&window, event, move |_: Event| {
            if wanted() {
                let _ = input.focus();
            }
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
    listen(&window, "paste", |e: ClipboardEvent| {
        if let Some(text) = e.clipboard_data().and_then(|d| d.get_data("text/plain").ok()) {
            e.prevent_default();
            typed(&text);
        }
    });
    listen(&window, "keyup", |_: KeyboardEvent| {
        BACKSPACE_DOWN.with(|b| b.set(false))
    });
    listen(&window, "copy", |e: ClipboardEvent| {
        give_selection(&e);
    });
    listen(&window, "cut", |e: ClipboardEvent| {
        if give_selection(&e) {
            SHORTCUTS.with(|s| s.set(Shortcuts { cut: true, ..s.get() }));
        }
    });
    listen(&window, "keydown", move |e: KeyboardEvent| {
        if e.key() == "Tab" {
            e.prevent_default();
        }
        let key = e.key();
        match key.as_str() {
            "Backspace" => {
                e.prevent_default();
                BACKSPACE_DOWN.with(|b| b.set(true));
                erased(1);
            }
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End" => e.prevent_default(),
            _ => {}
        }
        if e.ctrl_key() || e.meta_key() {
            let mut shortcuts = SHORTCUTS.with(Cell::get);
            let flag = match (key.to_ascii_lowercase().as_str(), e.shift_key()) {
                ("a", _) => Some(&mut shortcuts.select_all),
                ("z", true) | ("y", _) => Some(&mut shortcuts.redo),
                ("z", false) => Some(&mut shortcuts.undo),
                _ => None,
            };
            if let Some(flag) = flag {
                *flag = true;
                e.prevent_default();
                SHORTCUTS.with(|s| s.set(shortcuts));
            }
        }
        if !focused() {
            if wanted() {
                let _ = input.focus();
            }
            if key.chars().count() == 1 && !e.ctrl_key() && !e.meta_key() {
                typed(&key);
                e.prevent_default();
            }
        }
    });
}
