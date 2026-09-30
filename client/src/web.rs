#[cfg(any(target_arch = "wasm32", test))]
fn left_to_the_browser(key: &str, shortcut: bool) -> bool {
    let function_key = key
        .strip_prefix('F')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    shortcut || function_key
}

#[cfg(target_arch = "wasm32")]
pub fn keep_keys_in_the_game() {
    use wasm_bindgen::prelude::*;
    use web_sys::KeyboardEvent;

    let Some(window) = web_sys::window() else { return };
    let on_key = Closure::<dyn FnMut(KeyboardEvent)>::new(|e: KeyboardEvent| {
        // AltGr reads as Ctrl+Alt on Windows, yet it types a character.
        let shortcut = (e.ctrl_key() || e.meta_key() || e.alt_key()) && !e.get_modifier_state("AltGraph");
        if !left_to_the_browser(&e.key(), shortcut) {
            e.prevent_default();
        }
    });
    let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();
}

#[cfg(test)]
mod tests {
    use super::left_to_the_browser;

    #[test]
    fn typing_keys_stay_in_the_game() {
        for key in ["'", "/", " ", "a", "F", "Tab", "Backspace", "Enter", "ArrowDown"] {
            assert!(!left_to_the_browser(key, false), "{key}");
        }
    }

    #[test]
    fn shortcuts_and_function_keys_stay_with_the_browser() {
        assert!(left_to_the_browser("r", true));
        assert!(left_to_the_browser("ArrowLeft", true));
        for key in ["F5", "F11", "F12"] {
            assert!(left_to_the_browser(key, false), "{key}");
        }
    }
}
