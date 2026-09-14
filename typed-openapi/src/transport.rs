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
//! Both `send` methods take `&self`, so a client is shared rather than
//! borrowed exclusively — which is what `ureq::Agent` and `reqwest::Client`
//! are built for, and what lets one client serve a whole command tree. An
//! adapter over a client that needs `&mut` to send holds it behind a `Mutex`,
//! and [`Recorder`] holds its script the same way; that is why a recorder is
//! scripted, sent through and read back without ever being `mut`.
//!
//! No adapter may turn a status code into an error: the status belongs to the
//! layer above, which needs the body that came with it. ureq does this by
//! default and must be built with `http_status_as_error(false)`.
//!
//! # Writing a script
//!
//! [`Recorder`] is the client the tests of this crate and of its example
//! adoption run against. It sends nothing, answers from a script, and keeps
//! every request it was given.
//!
//! An answer is queued either for one **route** — the method and the path of
//! the request as it goes out, with the query string left out of it — or for
//! **anything**. A request takes the next answer queued for its own route;
//! when that queue is empty it takes the next answer queued for anything; when
//! that is empty too the answer is `200 {}`.
//!
//! ```
//! use http::{Method, StatusCode};
//! use serde_json::json;
//! use typed_openapi::{Recorder, SyncClient};
//!
//! let client = Recorder::new()
//!     .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!([{"id": 5}]))
//!     .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!([]))
//!     .failing_route(Method::POST, "/vouchers", "the request never left")
//!     .answering(StatusCode::OK, &json!({}));
//!
//! let get = |uri: &str| http::Request::get(uri).body(Vec::new()).unwrap();
//! assert_eq!(client.send(get("/vouchers?page=1")).unwrap().body(), br#"[{"id":5}]"#);
//! assert_eq!(client.send(get("/vouchers?page=2")).unwrap().body(), b"[]");
//!
//! // One answer queued for anything is left, and nothing asked for it.
//! assert_eq!(client.unused(), 2);
//! assert_eq!(client.take().len(), 2);
//! ```
//!
//! Several answers queued for one route come back in the order they were
//! queued, which is what a pager needs — page, next page, empty page — and
//! what a retry policy needs for "a failure, then an answer". Queueing per
//! route is what keeps a scenario that crosses several endpoints from being
//! pinned to the order the code under test happens to send in.
//!
//! A scripted failure reaches the caller as [`RecorderError`] through whichever
//! `send` was called, so everything a caller does with a transport failure — a
//! retry policy, a ledger line for an attempt whose outcome never arrived — is
//! reachable from a test. The request that got it is recorded like any other.
//!
//! [`Recorder::take`] hands back what was sent, oldest first, and
//! [`Recorder::unused`] how many answers were never reached: a run that left
//! answers behind did not do what the test set it up to do, and that is worth
//! asserting on rather than inferring.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

use http::{Method, Request, Response, StatusCode, header};
use thiserror::Error;

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

/// The failure a script asked for, carrying the message it was queued with.
///
/// A message is the whole of it, deliberately. The two states a retry rule
/// tells apart — the request never left, and the request left and nothing came
/// back — are a reading of what a failure *means*, and this crate has no far
/// side to read: it sends nothing. A test that needs the distinction queues
/// two different messages and asserts on the one it got, and the reading stays
/// where the rule is.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct RecorderError {
    message: String,
}

impl RecorderError {
    /// The message this failure was queued with.
    ///
    /// The same text [`Display`](std::fmt::Display) renders, handed back
    /// unwrapped so that a test compares against the constant it scripted
    /// rather than against a rendering.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A client that sends nothing, answers from a script, and keeps every request
/// it was given.
///
/// It is both a [`SyncClient`] and an [`AsyncClient`] over one script, so one
/// fixture tests both call paths, and because it *answers* it can stand in for
/// the server through a multi-step chain — no socket, no runtime, no fixture
/// server. The module documentation describes how a script is written.
#[derive(Debug, Default)]
pub struct Recorder {
    held: Mutex<Held>,
}

/// What a recorder is holding: what is left to answer, and what has been sent.
///
/// One lock over both, so that what was recorded and what was answered cannot
/// disagree about the order they happened in.
#[derive(Debug, Default)]
struct Held {
    per_route: HashMap<Route, VecDeque<Answer>>,
    anything: VecDeque<Answer>,
    sent: Vec<HttpRequest>,
}

/// What an answer is queued against: a method and a path.
///
/// The query string is no part of it, so a pager walking one path with a
/// different `page` each time draws from one queue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Route {
    method: Method,
    path: String,
}

impl Route {
    fn new(method: Method, path: &str) -> Self {
        Self {
            method,
            path: path.to_owned(),
        }
    }

    fn of(request: &HttpRequest) -> Self {
        Self::new(request.method().clone(), request.uri().path())
    }
}

/// One thing a recorder does when a request reaches it.
#[derive(Debug)]
enum Answer {
    Response(HttpResponse),
    Failure(String),
}

impl Recorder {
    /// A recorder with an empty script: every request is answered `200 {}`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a JSON response for the next request on `method path`.
    #[must_use]
    pub fn answering_route(
        self,
        method: Method,
        path: &str,
        status: StatusCode,
        body: &serde_json::Value,
    ) -> Self {
        self.queue_for(
            Route::new(method, path),
            Answer::Response(json(status, body)),
        )
    }

    /// Queue a failure for the next request on `method path`.
    #[must_use]
    pub fn failing_route(self, method: Method, path: &str, message: &str) -> Self {
        self.queue_for(
            Route::new(method, path),
            Answer::Failure(message.to_owned()),
        )
    }

    /// Queue a JSON response for the next request whose own route has nothing
    /// left.
    ///
    /// A scenario that does not care which endpoint answers what queues here
    /// and stays one line per answer.
    #[must_use]
    pub fn answering(self, status: StatusCode, body: &serde_json::Value) -> Self {
        self.queue_for_anything(Answer::Response(json(status, body)))
    }

    /// Queue a failure for the next request whose own route has nothing left.
    #[must_use]
    pub fn failing(self, message: &str) -> Self {
        self.queue_for_anything(Answer::Failure(message.to_owned()))
    }

    /// Every request sent so far, oldest first, and the recorder is left empty.
    ///
    /// It drains because `http::Request` is not `Clone`, so there is nothing to
    /// hand back a copy of; a test that wants to look twice binds the vector.
    pub fn take(&self) -> Vec<HttpRequest> {
        std::mem::take(&mut self.held().sent)
    }

    /// How many scripted answers are still queued, over every route and the
    /// anything queue together.
    #[must_use]
    pub fn unused(&self) -> usize {
        let held = self.held();
        held.anything.len() + held.per_route.values().map(VecDeque::len).sum::<usize>()
    }

    fn queue_for(mut self, route: Route, answer: Answer) -> Self {
        self.script()
            .per_route
            .entry(route)
            .or_default()
            .push_back(answer);
        self
    }

    fn queue_for_anything(mut self, answer: Answer) -> Self {
        self.script().anything.push_back(answer);
        self
    }

    /// The script, while the recorder is still being built and owned.
    fn script(&mut self) -> &mut Held {
        self.held.get_mut().unwrap_or_else(PoisonError::into_inner)
    }

    /// What the recorder is holding.
    ///
    /// A poisoned lock is taken anyway. The only way to poison it is for a test
    /// to panic while holding it, and a second failure reported as a poisoned
    /// mutex would bury the first one, which is the one that says what went
    /// wrong.
    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn answer(&self, request: HttpRequest) -> Result<HttpResponse, RecorderError> {
        let mut held = self.held();
        let route = Route::of(&request);
        held.sent.push(request);

        let for_the_route = held.per_route.get_mut(&route).and_then(VecDeque::pop_front);
        let queued = match for_the_route {
            Some(answer) => Some(answer),
            None => held.anything.pop_front(),
        };

        match queued {
            Some(Answer::Response(response)) => Ok(response),
            Some(Answer::Failure(message)) => Err(RecorderError { message }),
            None => Ok(json(StatusCode::OK, &serde_json::json!({}))),
        }
    }
}

impl SyncClient for Recorder {
    type Error = RecorderError;

    fn send(&self, request: HttpRequest) -> Result<HttpResponse, RecorderError> {
        self.answer(request)
    }
}

impl AsyncClient for Recorder {
    type Error = RecorderError;

    fn send(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, RecorderError>> + Send {
        std::future::ready(self.answer(request))
    }
}

fn json(status: StatusCode, body: &serde_json::Value) -> HttpResponse {
    let mut response = Response::new(body.to_string().into_bytes());
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    response
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "a failed unwrap or a panicking index is a failing test"
    )]

    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::{
        AsyncClient, HttpRequest, HttpResponse, Method, Recorder, Request, StatusCode, SyncClient,
    };

    fn request(method: Method, uri: &str) -> HttpRequest {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Vec::new())
            .unwrap()
    }

    fn sent(client: &Recorder, method: Method, uri: &str) -> HttpResponse {
        SyncClient::send(client, request(method, uri)).unwrap()
    }

    fn body(response: &HttpResponse) -> serde_json::Value {
        serde_json::from_slice(response.body()).unwrap()
    }

    /// A runtime in six lines. The recorder never yields, so the first poll is
    /// ready — enough to drive the async path without taking tokio as a
    /// dependency of this crate.
    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
        }
    }

    #[test]
    fn answers_queued_for_one_route_come_back_in_the_order_they_were_queued() {
        let client = Recorder::new()
            .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!(["first"]))
            .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!(["second"]))
            .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!([]));

        // The query is no part of the route, so a pager's three calls to one
        // path draw from the one queue in the order it was filled.
        let pages: Vec<serde_json::Value> = ["?page=1", "?page=2", "?page=3"]
            .into_iter()
            .map(|query| body(&sent(&client, Method::GET, &format!("/vouchers{query}"))))
            .collect();

        assert_eq!(pages, [json!(["first"]), json!(["second"]), json!([])]);
    }

    #[test]
    fn a_route_is_answered_from_its_own_queue_and_not_from_another_routes() {
        // One path, two methods: the method is half the route, so a read and a
        // write of the same collection do not share a queue.
        let client = Recorder::new()
            .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!("the list"))
            .answering_route(
                Method::POST,
                "/vouchers",
                StatusCode::CREATED,
                &json!("the new one"),
            );

        assert_eq!(
            body(&sent(&client, Method::POST, "/vouchers")),
            json!("the new one")
        );
        assert_eq!(
            body(&sent(&client, Method::GET, "/vouchers")),
            json!("the list")
        );
    }

    #[test]
    fn a_route_with_nothing_queued_falls_back_to_anything_and_then_to_the_default() {
        let client = Recorder::new().answering(StatusCode::ACCEPTED, &json!("whatever you asked"));

        let from_anything = sent(&client, Method::GET, "/vouchers");
        assert_eq!(from_anything.status(), StatusCode::ACCEPTED);
        assert_eq!(body(&from_anything), json!("whatever you asked"));

        let from_nothing = sent(&client, Method::GET, "/vouchers");
        assert_eq!(from_nothing.status(), StatusCode::OK);
        assert_eq!(body(&from_nothing), json!({}));
    }

    #[test]
    fn a_route_with_something_queued_leaves_the_anything_queue_alone() {
        let client = Recorder::new()
            .answering_route(
                Method::GET,
                "/vouchers",
                StatusCode::OK,
                &json!("for the route"),
            )
            .answering(StatusCode::OK, &json!("for anything"));

        assert_eq!(
            body(&sent(&client, Method::GET, "/vouchers")),
            json!("for the route")
        );
        assert_eq!(client.unused(), 1);
    }

    #[test]
    fn a_scripted_failure_reaches_the_caller_and_the_request_is_recorded_anyway() {
        let client =
            Recorder::new().failing_route(Method::POST, "/vouchers", "the request never left");

        let failed =
            SyncClient::send(&client, request(Method::POST, "/vouchers")).expect_err("scripted");

        assert_eq!(failed.message(), "the request never left");
        assert_eq!(failed.to_string(), "the request never left");
        let sent = client.take();
        assert_eq!(sent.len(), 1, "a refused request went out like any other");
        assert_eq!(sent[0].uri().path(), "/vouchers");
    }

    #[test]
    fn two_failures_are_told_apart_by_the_messages_they_were_queued_with() {
        let client = Recorder::new()
            .failing_route(Method::POST, "/vouchers", "the request never left")
            .failing("the request left and nothing came back");

        let never = SyncClient::send(&client, request(Method::POST, "/vouchers"))
            .expect_err("the route's own failure");
        let silent = SyncClient::send(&client, request(Method::GET, "/vouchers"))
            .expect_err("the failure queued for anything");

        assert_eq!(never.message(), "the request never left");
        assert_eq!(silent.message(), "the request left and nothing came back");
    }

    #[test]
    fn what_is_left_unused_is_something_a_test_can_ask_about() {
        let client = Recorder::new()
            .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!("first"))
            .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!("second"))
            .answering(StatusCode::OK, &json!("third"));

        assert_eq!(client.unused(), 3);
        let _ = sent(&client, Method::GET, "/vouchers");
        assert_eq!(client.unused(), 2);
        let _ = sent(&client, Method::GET, "/elsewhere");
        assert_eq!(client.unused(), 1);
    }

    #[test]
    fn one_script_drives_the_sync_and_the_async_path_identically() {
        let script = || {
            Recorder::new()
                .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!("the list"))
                .failing_route(Method::POST, "/vouchers", "the request never left")
        };
        let synchronous = script();
        let asynchronous = script();

        let read = body(&sent(&synchronous, Method::GET, "/vouchers"));
        let wrote = SyncClient::send(&synchronous, request(Method::POST, "/vouchers"))
            .expect_err("scripted");

        let read_async = block_on(AsyncClient::send(
            &asynchronous,
            request(Method::GET, "/vouchers"),
        ))
        .unwrap();
        let wrote_async = block_on(AsyncClient::send(
            &asynchronous,
            request(Method::POST, "/vouchers"),
        ))
        .expect_err("scripted");

        assert_eq!(read, body(&read_async));
        assert_eq!(wrote.message(), wrote_async.message());

        let routes = |client: &Recorder| -> Vec<String> {
            client
                .take()
                .iter()
                .map(|request| format!("{} {}", request.method(), request.uri()))
                .collect()
        };
        assert_eq!(routes(&synchronous), routes(&asynchronous));
    }
}
