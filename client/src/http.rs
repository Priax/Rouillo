use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use shared::config;

pub type HttpSlot = Arc<Mutex<Option<ehttp::Result<ehttp::Response>>>>;

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
    ehttp::fetch(req, move |r| {
        *slot.lock().unwrap() = Some(r);
    });
}

fn json_body(url: String, method: &str, body: String) -> ehttp::Request {
    let mut req = ehttp::Request {
        method: method.to_owned(),
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
    send(json_body(url, "POST", body), token, slot);
}

pub fn patch_json(url: String, body: String, token: Option<String>, slot: HttpSlot) {
    send(json_body(url, "PATCH", body), token, slot);
}

pub fn delete_req(url: String, token: Option<String>, slot: HttpSlot) {
    let req = ehttp::Request {
        method: "DELETE".to_owned(),
        ..ehttp::Request::get(url)
    };
    send(req, token, slot);
}

pub fn post_empty(url: String, token: Option<String>, slot: HttpSlot) {
    send(ehttp::Request::post(url, vec![]), token, slot);
}
