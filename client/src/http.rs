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
        std::env::var("PUYO_API").unwrap_or_else(|_| format!("http://127.0.0.1:{}", config::SERVER_PORT))
    }
}

fn push_header(req: &mut ehttp::Request, key: &str, value: String) {
    req.headers.headers.push((key.to_owned(), value));
}

pub fn get(url: String, token: Option<String>, slot: HttpSlot) {
    let mut req = ehttp::Request::get(&url);
    if let Some(t) = token {
        push_header(&mut req, "authorization", format!("Bearer {t}"));
    }
    ehttp::fetch(req, move |r| {
        *slot.lock().unwrap() = Some(r);
    });
}

pub fn post_json(url: String, body: String, token: Option<String>, slot: HttpSlot) {
    let mut req = ehttp::Request::post(&url, body.into_bytes());
    push_header(&mut req, "content-type", "application/json".to_owned());
    if let Some(t) = token {
        push_header(&mut req, "authorization", format!("Bearer {t}"));
    }
    ehttp::fetch(req, move |r| {
        *slot.lock().unwrap() = Some(r);
    });
}

pub fn patch_json(url: String, body: String, token: Option<String>, slot: HttpSlot) {
    let mut req = ehttp::Request {
        method: "PATCH".to_owned(),
        body: body.into_bytes(),
        ..ehttp::Request::get(url)
    };
    push_header(&mut req, "content-type", "application/json".to_owned());
    if let Some(t) = token {
        push_header(&mut req, "authorization", format!("Bearer {t}"));
    }
    ehttp::fetch(req, move |r| {
        *slot.lock().unwrap() = Some(r);
    });
}

pub fn delete_req(url: String, token: Option<String>, slot: HttpSlot) {
    let mut req = ehttp::Request {
        method: "DELETE".to_owned(),
        ..ehttp::Request::get(url)
    };
    if let Some(t) = token {
        push_header(&mut req, "authorization", format!("Bearer {t}"));
    }
    ehttp::fetch(req, move |r| {
        *slot.lock().unwrap() = Some(r);
    });
}

pub fn post_empty(url: String, token: Option<String>, slot: HttpSlot) {
    let mut req = ehttp::Request::post(url, vec![]);
    if let Some(t) = token {
        push_header(&mut req, "authorization", format!("Bearer {t}"));
    }
    ehttp::fetch(req, move |r| {
        *slot.lock().unwrap() = Some(r);
    });
}
