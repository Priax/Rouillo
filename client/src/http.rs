use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use shared::config;

use crate::connection::now_secs;

const TIMEOUT_SECS: f64 = 10.0;
const TIMEOUT_MESSAGE: &str = "le serveur ne répond pas";

type Slot = Arc<Mutex<Option<ehttp::Result<ehttp::Response>>>>;

/// A request on its way. Past its deadline it answers with the timeout, whatever comes after.
pub struct Request {
    answer: Slot,
    deadline: f64,
}

impl Request {
    fn send(mut req: ehttp::Request, token: Option<String>) -> Self {
        if let Some(t) = token {
            push_header(&mut req, "authorization", format!("Bearer {t}"));
        }
        let answer: Slot = Arc::default();
        let slot = Arc::clone(&answer);
        ehttp::fetch(req, move |result| {
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(result);
            }
            crate::wake::wake();
        });
        Self { answer, deadline: now_secs() + TIMEOUT_SECS }
    }

    /// Its body decoded, once it came.
    pub fn json<T: serde::de::DeserializeOwned>(self) -> Json<T> {
        Json { request: self, body: PhantomData }
    }

    /// Whether it went through, for an answer whose body is not needed.
    pub fn done(self) -> Done {
        Done(self)
    }
}

/// A request whose answer's body is decoded as `T`.
pub struct Json<T> {
    request: Request,
    body: PhantomData<fn() -> T>,
}

/// A request whose answer's body is not needed.
pub struct Done(Request);

/// What a request gives once it is over.
pub trait Answer {
    type Output;
    fn poll(&self) -> Option<Self::Output>;
}

impl Answer for Request {
    type Output = ehttp::Result<ehttp::Response>;

    fn poll(&self) -> Option<Self::Output> {
        let came = self.answer.lock().ok().and_then(|mut slot| slot.take());
        came.or_else(|| (now_secs() >= self.deadline).then(|| Err(TIMEOUT_MESSAGE.to_owned())))
    }
}

impl<T: serde::de::DeserializeOwned> Answer for Json<T> {
    type Output = Result<T, String>;

    fn poll(&self) -> Option<Self::Output> {
        self.request.poll().map(answer)
    }
}

impl Answer for Done {
    type Output = Result<(), String>;

    fn poll(&self) -> Option<Self::Output> {
        self.0.poll().map(|result| match result {
            Ok(resp) if resp.ok => Ok(()),
            Ok(resp) => Err(error_message(&resp)),
            Err(e) => Err(network_error(&e)),
        })
    }
}

/// The answer of the request in `slot` once it is over, which empties it.
pub fn take<A: Answer>(slot: &mut Option<A>) -> Option<A::Output> {
    let output = slot.as_ref()?.poll()?;
    *slot = None;
    Some(output)
}

pub fn json<T: serde::de::DeserializeOwned>(resp: &ehttp::Response) -> Option<T> {
    serde_json::from_str(resp.text()?).ok()
}

const INVALID_REPLY: &str = "Réponse serveur invalide.";

/// What a finished request gave: the body of a successful answer, decoded,
/// or the message to show the player.
fn answer<T: serde::de::DeserializeOwned>(result: ehttp::Result<ehttp::Response>) -> Result<T, String> {
    match result {
        Ok(resp) if resp.ok => json(&resp).ok_or_else(|| INVALID_REPLY.to_owned()),
        Ok(resp) => Err(error_message(&resp)),
        Err(e) => Err(network_error(&e)),
    }
}

pub fn network_error(e: &str) -> String {
    format!("Erreur réseau: {e}")
}

pub fn error_message(resp: &ehttp::Response) -> String {
    let Some(refusal) = json::<shared::api::Refusal>(resp) else {
        return format!("Erreur {}", resp.status);
    };
    translate(&refusal.code).map_or(refusal.error, str::to_owned)
}

fn translate(code: &str) -> Option<&'static str> {
    Some(match code {
        "bad_username" => "Le pseudo doit faire 3 à 24 caractères (lettres, chiffres ou _).",
        "bad_password" => "Le mot de passe doit faire au moins 8 caractères.",
        "bio_too_long" => "La bio ne doit pas dépasser 500 caractères.",
        "music_too_long" => "La musique préférée ne doit pas dépasser 200 caractères.",
        "self_friend_request" => "Vous ne pouvez pas vous ajouter vous-même en ami.",
        "unauthorized" => "Votre session a expiré, reconnectez-vous.",
        "bad_credentials" => "Pseudo ou mot de passe incorrect.",
        "wrong_password" => "Mot de passe incorrect.",
        "not_found" => "Introuvable.",
        "username_taken" => "Ce pseudo est déjà pris.",
        "friend_request_exists" => "Une demande d'ami existe déjà.",
        "favorites_full" => "Vous gardez déjà le maximum de replays: retirez-en un.",
        "too_many_requests" => "Trop de tentatives, réessayez plus tard.",
        "bad_image" => "Image illisible: envoyez un PNG, JPEG, WebP ou GIF.",
        "image_too_large" => "Image trop grande (5 Mo et 6000 pixels au plus).",
        "bad_query" | "invalid_body" => "Requête invalide.",
        "internal" => "Erreur du serveur, réessayez plus tard.",
        _ => return None,
    })
}

pub fn encode_query(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => char::from(b).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn api_url(path: &str) -> String {
    format!("{}/api/{}", api_base(), path.trim_start_matches('/'))
}

/// A path the API answered with, such as an image's, made absolute.
pub fn resource_url(path: &str) -> String {
    format!("{}{path}", api_base())
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

fn json_body(url: String, method: ehttp::Method, body: &impl serde::Serialize) -> ehttp::Request {
    let body = serde_json::to_vec(body).unwrap_or_default();
    let mut req = ehttp::Request { method, body, ..ehttp::Request::get(url) };
    push_header(&mut req, "content-type", "application/json".to_owned());
    req
}

pub fn get_user(user_id: shared::api::Uuid, token: Option<String>) -> Request {
    get(api_url(&format!("users/{user_id}")), token)
}

pub fn get(url: String, token: Option<String>) -> Request {
    Request::send(ehttp::Request::get(url), token)
}

pub fn post_json(url: String, body: &impl serde::Serialize, token: Option<String>) -> Request {
    Request::send(json_body(url, ehttp::Method::POST, body), token)
}

pub fn patch_json(url: String, body: &impl serde::Serialize, token: Option<String>) -> Request {
    Request::send(json_body(url, ehttp::Method::PATCH, body), token)
}

pub fn delete(url: String, token: Option<String>) -> Request {
    Request::send(ehttp::Request { method: ehttp::Method::DELETE, ..ehttp::Request::get(url) }, token)
}

pub fn put_bytes(url: String, body: Vec<u8>, token: Option<String>) -> Request {
    let mut req = ehttp::Request { method: ehttp::Method::PUT, body, ..ehttp::Request::get(url) };
    push_header(&mut req, "content-type", "application/octet-stream".to_owned());
    Request::send(req, token)
}

pub fn post_empty(url: String, token: Option<String>) -> Request {
    Request::send(ehttp::Request::post(url, vec![]), token)
}

#[cfg(test)]
impl Request {
    /// A request that never went out, answered by [`Request::answer`].
    pub fn unsent() -> Self {
        Self { answer: Arc::default(), deadline: f64::INFINITY }
    }

    pub fn answer(&self, status: u16, body: &str) {
        *self.answer.lock().unwrap() = Some(Ok(ehttp::Response {
            url: String::new(),
            ok: (200..300).contains(&status),
            status,
            status_text: String::new(),
            headers: ehttp::Headers::default(),
            bytes: body.as_bytes().to_vec(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, body: &str) -> ehttp::Response {
        ehttp::Response {
            url: String::new(),
            ok: false,
            status,
            status_text: String::new(),
            headers: ehttp::Headers::default(),
            bytes: body.as_bytes().to_vec(),
        }
    }

    fn ok(status: u16, body: &str) -> ehttp::Response {
        ehttp::Response { ok: (200..300).contains(&status), ..response(status, body) }
    }

    #[test]
    fn an_answer_is_its_body_or_the_message_to_show() {
        assert_eq!(answer::<Vec<u8>>(Ok(ok(200, "[1,2]"))), Ok(vec![1, 2]));
        assert_eq!(answer::<Vec<u8>>(Ok(ok(201, "[]"))), Ok(vec![]));
        assert_eq!(answer::<Vec<u8>>(Ok(ok(200, "{"))), Err(INVALID_REPLY.to_owned()));
        let refused = ok(409, r#"{"error":"Username already taken","code":"username_taken"}"#);
        assert_eq!(answer::<Vec<u8>>(Ok(refused)), Err("Ce pseudo est déjà pris.".to_owned()));
        assert_eq!(answer::<Vec<u8>>(Err("down".into())), Err(network_error("down")));
        assert_eq!(answer::<serde::de::IgnoredAny>(Ok(ok(200, "{}"))).map(|_| ()), Ok(()));
    }

    #[test]
    fn a_late_request_answers_with_the_timeout() {
        let late = Request { answer: Arc::default(), deadline: f64::NEG_INFINITY };
        assert_eq!(late.json::<u8>().poll(), Some(Err(network_error(TIMEOUT_MESSAGE))));
    }

    #[test]
    fn taking_an_answer_empties_its_slot() {
        let mut slot = Some(Request::unsent().done());
        assert!(take(&mut slot).is_none() && slot.is_some(), "nothing came yet");
        slot.as_ref().unwrap().0.answer(204, "");
        assert_eq!(take(&mut slot), Some(Ok(())));
        assert!(slot.is_none());
    }

    #[test]
    fn a_query_is_percent_encoded_byte_by_byte() {
        assert_eq!(encode_query("élo a_b"), "%C3%A9lo%20a_b");
        assert_eq!(encode_query("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn an_unknown_code_falls_back_to_the_server_text_then_the_status() {
        let resp = response(418, r#"{"error":"Teapot","code":"teapot"}"#);
        assert_eq!(error_message(&resp), "Teapot");
        assert_eq!(error_message(&response(502, "<html>")), "Erreur 502");
    }
}
