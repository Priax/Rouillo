use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use shared::config;

pub type HttpSlot = Arc<Mutex<Option<ehttp::Result<ehttp::Response>>>>;

const TIMEOUT: Duration = Duration::from_secs(10);
const TIMEOUT_MESSAGE: &str = "le serveur ne répond pas";

pub fn new_slot() -> HttpSlot {
    Arc::new(Mutex::new(None))
}

pub fn poll(slot: &HttpSlot) -> Option<ehttp::Result<ehttp::Response>> {
    slot.lock().unwrap().take()
}

pub fn take(slot: &mut Option<HttpSlot>) -> Option<ehttp::Result<ehttp::Response>> {
    let result = slot.as_ref().and_then(poll)?;
    *slot = None;
    Some(result)
}

pub fn json<T: serde::de::DeserializeOwned>(resp: &ehttp::Response) -> Option<T> {
    serde_json::from_str(resp.text()?).ok()
}

pub fn error_message(resp: &ehttp::Response) -> String {
    serde_json::from_str::<serde_json::Value>(resp.text().unwrap_or_default())
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("Erreur {}", resp.status))
}

pub fn api_url(path: &str) -> String {
    format!("{}/api/{}", api_base(), path.trim_start_matches('/'))
}

fn api_base() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        String::new()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("PUYO_API").unwrap_or_else(|_| {
            if cfg!(debug_assertions) {
                format!("http://127.0.0.1:{}", config::SERVER_PORT)
            } else {
                config::API_URL_RELEASE.to_string()
            }
        })
    }
}

fn push_header(req: &mut ehttp::Request, key: &str, value: String) {
    req.headers.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    req.headers.headers.push((key.to_owned(), value));
}

fn send(mut req: ehttp::Request, token: Option<String>, slot: HttpSlot) {
    if let Some(t) = token {
        push_header(&mut req, "authorization", format!("Bearer {t}"));
    }
    let done = Arc::new(AtomicBool::new(false));
    {
        let (slot, done) = (Arc::clone(&slot), Arc::clone(&done));
        after(TIMEOUT, move || deliver(&slot, &done, Err(TIMEOUT_MESSAGE.to_owned())));
    }
    ehttp::fetch(req, move |r| deliver(&slot, &done, r));
}

fn deliver(slot: &HttpSlot, done: &AtomicBool, result: ehttp::Result<ehttp::Response>) {
    if !done.swap(true, Ordering::AcqRel) {
        *slot.lock().unwrap() = Some(result);
    }
}

#[cfg(target_arch = "wasm32")]
fn after(delay: Duration, f: impl FnOnce() + 'static) {
    crate::web::after(delay, f);
}

#[cfg(not(target_arch = "wasm32"))]
fn after(delay: Duration, f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        f();
    });
}

fn json_body(url: String, method: ehttp::Method, body: String) -> ehttp::Request {
    let mut req = ehttp::Request {
        method,
        body: body.into_bytes(),
        ..ehttp::Request::get(url)
    };
    push_header(&mut req, "content-type", "application/json".to_owned());
    req
}

pub fn get(url: String, token: Option<String>, slot: HttpSlot) {
    send(ehttp::Request::get(url), token, slot);
}

pub fn post_json(url: String, body: String, token: Option<String>, slot: HttpSlot) {
    send(json_body(url, ehttp::Method::POST, body), token, slot);
}

pub fn patch_json(url: String, body: String, token: Option<String>, slot: HttpSlot) {
    send(json_body(url, ehttp::Method::PATCH, body), token, slot);
}

pub fn delete_req(url: String, token: Option<String>, slot: HttpSlot) {
    let req = ehttp::Request {
        method: ehttp::Method::DELETE,
        ..ehttp::Request::get(url)
    };
    send(req, token, slot);
}

pub fn post_empty(url: String, token: Option<String>, slot: HttpSlot) {
    send(ehttp::Request::post(url, vec![]), token, slot);
}
