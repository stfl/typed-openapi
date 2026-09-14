//! The one seam between a decided request and the network.
//!
//! The vocabulary is `http::Request<Vec<u8>>` in, `http::Response<Vec<u8>>`
//! out, which is what every client-agnostic Rust crate converges on (oauth2,
//! rustify, atrium-xrpc, kube-core). There is no de-facto *trait*, so this
//! crate defines its own two — a sync one and an async one, because a single
//! trait cannot be both and `maybe-async` makes the flavour a global switch.
//!
//! This crate ships no adapter and depends on no HTTP client, in any feature
//! combination: an adopter's choice of client is theirs, and a crate that
//! pinned one would make it everyone's. `examples/toy/cli/src/client.rs` has
//! both adapters in full — a ureq 3 agent in nine lines, a `reqwest::Client`
//! in twelve — written to be copied rather than depended on.
//!
//! An adapter in an adopter's crate wraps the client in a newtype, because
//! [`SyncClient`] and the client are both foreign there. A crate that owns
//! either one writes `impl SyncClient for ureq::Agent` directly.
//!
//! No adapter may turn a status code into an error: the status belongs to the
//! layer above, which needs the body that came with it. ureq does this by
//! default and must be built with `http_status_as_error(false)`.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::{Mutex, PoisonError};

use http::{Request, Response, StatusCode, header};

/// What every adapter takes.
pub type HttpRequest = Request<Vec<u8>>;
/// What every adapter returns.
pub type HttpResponse = Response<Vec<u8>>;

/// Something that can send a request and wait for the answer.
pub trait SyncClient {
    type Error: std::error::Error + Send + Sync + 'static;

    fn send(&self, request: HttpRequest) -> Result<HttpResponse, Self::Error>;
}

/// The same, for a caller inside a runtime. `Send` on the future so that a
/// call can be `tokio::spawn`ed.
pub trait AsyncClient {
    type Error: std::error::Error + Send + Sync + 'static;

    fn send(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, Self::Error>> + Send;
}

/// A client that sends nothing, answers from a script, and keeps every request
/// it was given.
///
/// It is both a [`SyncClient`] and an [`AsyncClient`], so one fixture tests
/// both call paths, and because it *answers* it can stand in for the server
/// through a multi-step chain — no socket, no runtime, no fixture server.
#[derive(Debug, Default)]
pub struct Recorder {
    answers: Mutex<VecDeque<HttpResponse>>,
    sent: Mutex<Vec<HttpRequest>>,
}

impl Recorder {
    /// A recorder with an empty script: every request is answered `200 {}`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one scripted answer. They are used in order; once the script runs
    /// out, `200 {}` is the answer.
    #[must_use]
    pub fn answering(self, status: StatusCode, body: &serde_json::Value) -> Self {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(json_response(status, body));
        self
    }

    /// Every request sent so far, oldest first, and the recorder is left empty.
    pub fn take(&self) -> Vec<HttpRequest> {
        std::mem::take(&mut self.sent.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn answer(&self, request: HttpRequest) -> HttpResponse {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request);
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| json_response(StatusCode::OK, &serde_json::json!({})))
    }
}

impl SyncClient for Recorder {
    type Error = Infallible;

    fn send(&self, request: HttpRequest) -> Result<HttpResponse, Infallible> {
        Ok(self.answer(request))
    }
}

impl AsyncClient for Recorder {
    type Error = Infallible;

    fn send(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, Infallible>> + Send {
        std::future::ready(Ok(self.answer(request)))
    }
}

fn json_response(status: StatusCode, body: &serde_json::Value) -> HttpResponse {
    let mut response = Response::new(body.to_string().into_bytes());
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    response
}
