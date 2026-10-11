//! Games over WebTransport, when the server offers it: messages go both ways
//! on one stream, each with its length first, and game states may come as
//! datagrams. The browser's own WebTransport is driven from a few lines of
//! JavaScript; the native client uses quinn.

use serde::Deserialize;

/// Where the server takes WebTransport sessions, as `/api/transport` says.
#[derive(Clone, Debug, Deserialize)]
pub struct Offer {
    pub url: String,
    /// The SHA-256 of the server's certificate when it made one up.
    #[serde(default)]
    pub cert_hash: Option<Vec<u8>>,
}

pub enum Event {
    Opened,
    Message(Vec<u8>),
    Closed(String),
}

pub struct Link(imp::Link);

impl Link {
    pub fn dial(offer: &Offer) -> Self {
        let url = format!("{}?v={}", offer.url, shared::PROTOCOL_VERSION);
        Self(imp::Link::dial(&url, offer.cert_hash.clone()))
    }

    pub fn send(&mut self, bytes: Vec<u8>) {
        self.0.send(bytes);
    }

    /// Sends `bytes` as a datagram, which may be lost; not before the link
    /// opened.
    pub fn send_datagram(&mut self, bytes: Vec<u8>) {
        self.0.send_datagram(bytes);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.0.try_recv()
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use wasm_bindgen::prelude::*;

    use super::Event;

    #[wasm_bindgen(inline_js = r"
export class WtLink {
  constructor(url, hash, events) {
    this.events = events;
    this.queue = [];
    this.writer = null;
    this.closed = false;
    const options = hash ? { serverCertificateHashes: [{ algorithm: 'sha-256', value: hash }] } : {};
    this.run(url, options).catch((e) => this.end(String((e && e.message) || e)));
  }
  async run(url, options) {
    if (typeof WebTransport === 'undefined') throw new Error('WebTransport absent');
    this.wt = new WebTransport(url, options);
    await this.wt.ready;
    const stream = await this.wt.createBidirectionalStream();
    this.writer = stream.writable.getWriter();
    for (const bytes of this.queue) this.write(bytes);
    this.queue = [];
    this.emit(0, null);
    this.wt.closed.then(() => this.end('fermée par le serveur'), (e) => this.end(String(e)));
    this.readDatagrams(this.wt.datagrams.readable.getReader()).catch(() => {});
    await this.readFrames(stream.readable.getReader());
    this.end('flux fermé par le serveur');
  }
  async readDatagrams(reader) {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      this.emit(1, value);
    }
  }
  async readFrames(reader) {
    let buf = new Uint8Array(0);
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      const joined = new Uint8Array(buf.length + value.length);
      joined.set(buf);
      joined.set(value, buf.length);
      buf = joined;
      while (buf.length >= 4) {
        const len = new DataView(buf.buffer, buf.byteOffset, 4).getUint32(0, true);
        if (buf.length < 4 + len) break;
        this.emit(1, buf.slice(4, 4 + len));
        buf = buf.subarray(4 + len);
      }
    }
  }
  emit(kind, payload) {
    if (!this.closed) this.events(kind, payload);
  }
  send(bytes) {
    if (this.closed) return;
    if (this.writer) this.write(bytes);
    else this.queue.push(bytes);
  }
  sendDatagram(bytes) {
    if (this.closed || !this.writer) return;
    if (!this.datagrams) this.datagrams = this.wt.datagrams.writable.getWriter();
    this.datagrams.write(bytes).catch(() => {});
  }
  write(bytes) {
    const frame = new Uint8Array(4 + bytes.length);
    new DataView(frame.buffer).setUint32(0, bytes.length, true);
    frame.set(bytes, 4);
    this.writer.write(frame).catch((e) => this.end(String(e)));
  }
  end(reason) {
    if (this.closed) return;
    this.closed = true;
    this.events(2, reason);
    this.close();
  }
  close() {
    this.closed = true;
    try {
      if (this.wt) this.wt.close();
    } catch (_) {}
  }
}
")]
    extern "C" {
        type WtLink;

        #[wasm_bindgen(constructor)]
        fn new(url: &str, hash: Option<Vec<u8>>, events: &Closure<dyn FnMut(u32, JsValue)>) -> WtLink;

        #[wasm_bindgen(method)]
        fn send(this: &WtLink, bytes: Vec<u8>);

        #[wasm_bindgen(method, js_name = sendDatagram)]
        fn send_datagram(this: &WtLink, bytes: Vec<u8>);

        #[wasm_bindgen(method)]
        fn close(this: &WtLink);
    }

    pub struct Link {
        js: WtLink,
        events: Rc<RefCell<VecDeque<Event>>>,
        _callback: Closure<dyn FnMut(u32, JsValue)>,
    }

    impl Link {
        pub fn dial(url: &str, hash: Option<Vec<u8>>) -> Self {
            let events = Rc::new(RefCell::new(VecDeque::new()));
            let queue = Rc::clone(&events);
            let callback = Closure::<dyn FnMut(u32, JsValue)>::new(move |kind: u32, payload: JsValue| {
                let event = match kind {
                    0 => Event::Opened,
                    1 => Event::Message(js_sys::Uint8Array::new(&payload).to_vec()),
                    _ => Event::Closed(payload.as_string().unwrap_or_default()),
                };
                queue.borrow_mut().push_back(event);
            });
            Self {
                js: WtLink::new(url, hash, &callback),
                events,
                _callback: callback,
            }
        }

        pub fn send(&mut self, bytes: Vec<u8>) {
            self.js.send(bytes);
        }

        pub fn send_datagram(&mut self, bytes: Vec<u8>) {
            self.js.send_datagram(bytes);
        }

        pub fn try_recv(&self) -> Option<Event> {
            self.events.borrow_mut().pop_front()
        }
    }

    impl Drop for Link {
        fn drop(&mut self) {
            self.js.close();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::sync::{mpsc, OnceLock};

    use tokio::sync::mpsc as async_mpsc;
    use web_transport_quinn::{ClientBuilder, RecvStream};

    use super::Event;

    fn runtime() -> &'static tokio::runtime::Runtime {
        static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("webtransport")
                .enable_all()
                .build()
                .expect("a runtime for WebTransport")
        })
    }

    enum Out {
        Stream(Vec<u8>),
        Datagram(Vec<u8>),
    }

    pub struct Link {
        out: async_mpsc::UnboundedSender<Out>,
        events: mpsc::Receiver<Event>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Link {
        pub fn dial(url: &str, hash: Option<Vec<u8>>) -> Self {
            let (events_tx, events) = mpsc::channel();
            let (out, out_rx) = async_mpsc::unbounded_channel();
            let url = url.to_owned();
            let task = runtime().spawn(async move {
                let reason = run(&url, hash, out_rx, &events_tx).await;
                let _ = events_tx.send(Event::Closed(reason));
            });
            Self { out, events, task }
        }

        pub fn send(&mut self, bytes: Vec<u8>) {
            let _ = self.out.send(Out::Stream(bytes));
        }

        pub fn send_datagram(&mut self, bytes: Vec<u8>) {
            let _ = self.out.send(Out::Datagram(bytes));
        }

        pub fn try_recv(&self) -> Option<Event> {
            self.events.try_recv().ok()
        }
    }

    impl Drop for Link {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn run(
        url: &str,
        hash: Option<Vec<u8>>,
        mut out: async_mpsc::UnboundedReceiver<Out>,
        events: &mpsc::Sender<Event>,
    ) -> String {
        let client = match hash {
            Some(hash) => ClientBuilder::new().with_server_certificate_hashes(vec![hash]),
            None => ClientBuilder::new().with_system_roots(),
        };
        let client = match client {
            Ok(client) => client,
            Err(e) => return e.to_string(),
        };
        let Ok(url) = url.parse::<url::Url>() else {
            return format!("adresse invalide: {url}");
        };
        let session = match client.connect(url).await {
            Ok(session) => session,
            Err(e) => return e.to_string(),
        };
        let (mut send, recv) = match session.open_bi().await {
            Ok(stream) => stream,
            Err(e) => return e.to_string(),
        };
        let _ = events.send(Event::Opened);
        let reader = tokio::spawn(read_frames(recv, events.clone()));
        let reason = loop {
            tokio::select! {
                next = out.recv() => {
                    let bytes = match next {
                        Some(Out::Stream(bytes)) => bytes,
                        Some(Out::Datagram(bytes)) => {
                            let _ = session.send_datagram(bytes.into());
                            continue;
                        }
                        None => break "fermée".to_owned(),
                    };
                    if let Err(e) = send.write_all(&shared::frame_header(bytes.len())).await {
                        break e.to_string();
                    }
                    if let Err(e) = send.write_all(&bytes).await {
                        break e.to_string();
                    }
                }
                datagram = session.read_datagram() => match datagram {
                    Ok(bytes) => {
                        let _ = events.send(Event::Message(bytes.to_vec()));
                    }
                    Err(e) => break e.to_string(),
                },
                _ = session.closed() => break "fermée par le serveur".to_owned(),
            }
        };
        reader.abort();
        session.close(0, b"");
        reason
    }

    async fn read_frames(mut recv: RecvStream, events: mpsc::Sender<Event>) {
        let mut header = [0; shared::FRAME_HEADER];
        while recv.read_exact(&mut header).await.is_ok() {
            let len = shared::frame_len(header);
            if len > shared::MAX_DECODED_SIZE {
                return;
            }
            let mut buf = vec![0; len];
            if recv.read_exact(&mut buf).await.is_err() || events.send(Event::Message(buf)).is_err() {
                return;
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// Against a running server: `PUYO_WT_OFFER` holds what its
    /// `/api/transport` says.
    #[test]
    #[ignore = "needs a server with WebTransport"]
    fn a_ping_comes_back_over_webtransport() {
        let offer: Offer =
            serde_json::from_str(&std::env::var("PUYO_WT_OFFER").expect("PUYO_WT_OFFER")).expect("an offer");
        let mut link = Link::dial(&offer);
        let ping = shared::encode(&shared::ClientMessage::Ping { id: 42 }).expect("encode");
        link.send(ping);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut opened = false;
        while std::time::Instant::now() < deadline {
            match link.try_recv() {
                Some(Event::Opened) => opened = true,
                Some(Event::Message(bytes)) => {
                    assert!(opened, "a message before the link opened");
                    if matches!(shared::decode(&bytes), Some(shared::ServerMessage::Pong { id: 42 })) {
                        return;
                    }
                }
                Some(Event::Closed(reason)) => panic!("closed: {reason}"),
                None => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        panic!("no pong");
    }
}
