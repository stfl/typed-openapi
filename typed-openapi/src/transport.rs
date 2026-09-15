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
//!
//! # An answer that carries headers
//!
//! `answering` and `answering_route` are the short spelling of the common
//! case, and they build a JSON response. A caller that branches on a *header* —
//! a pager following `Link`, a backoff reading `Retry-After`, a create reading
//! `Location` — queues a whole [`HttpResponse`] instead, through
//! [`Recorder::answering_with`] or [`Recorder::answering_route_with`].
//! [`json_response`] builds the one the short spelling builds, so "the same
//! answer, plus a header" is two lines:
//!
//! ```
//! use http::{Method, StatusCode, header};
//! use serde_json::json;
//! use typed_openapi::{Recorder, SyncClient, json_response};
//!
//! let mut page = json_response(StatusCode::OK, &json!([{"id": 5}]));
//! page.headers_mut().insert(
//!     header::LINK,
//!     header::HeaderValue::from_static(r#"</vouchers?page=2>; rel="next""#),
//! );
//!
//! let client = Recorder::new().answering_route_with(Method::GET, "/vouchers", page);
//! let answer = client.send(http::Request::get("/vouchers").body(Vec::new()).unwrap()).unwrap();
//!
//! assert_eq!(answer.headers()[header::LINK], r#"</vouchers?page=2>; rel="next""#);
//! ```
//!
//! # Refusing what nobody queued
//!
//! An empty script answers `200 {}`, which is what a test that only cares what
//! went *out* wants: it queues nothing and asserts on [`Recorder::take`].
//!
//! A test that does care what came back wants the opposite, and asks for it
//! with [`Recorder::strict`]. A strict recorder **panics** on a request no
//! queue has an answer for, naming the route it was asked for and what it is
//! still holding. That is a bug in the test rather than in the code under test,
//! and a panic puts it on the line that caused it — where a plausible, empty,
//! successful answer would surface as an assertion going red three layers away.
//! It is a panic and not a [`RecorderError`] for the same reason: a failure the
//! script asked for and a request the script forgot are different mistakes, and
//! a test scripted to expect the first must not pass on the second.
//!
//! Strictness says nothing about what a queued answer is. A strict recorder
//! answers, fails and records exactly as a lenient one does; only the empty
//! case differs. Both `send` methods do their work when they are called, so the
//! panic lands on the `send` line even on the async path, before anything is
//! awaited.

use std::collections::{HashMap, VecDeque};
use std::fmt;
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

/// A reference to a client is a client, which is what the `&self` receiver on
/// [`SyncClient::send`] already promises.
///
/// Without this, a caller holding `&dyn SyncClient<Error = E>` — the natural
/// way to hold one of two clients, a production one and a [`Recorder`] — cannot
/// hand it to anything in this crate, because every `send` here is generic over
/// `C: SyncClient` and the trait object is not one. The delegating newtype that
/// closes the gap carries no decision, so the crate closes it instead.
///
/// `?Sized` is the load-bearing half: bounded to `C: Sized` the impl covers
/// `&ConcreteClient` and still not `&dyn SyncClient<Error = E>`, which is the
/// case that wanted the newtype.
///
/// The cost, which is a decision rather than a side effect: `&C` is spoken for,
/// so a downstream crate cannot write `impl SyncClient for &Theirs`. It writes
/// the impl on `Theirs` and takes the reference from here.
impl<C: SyncClient + ?Sized> SyncClient for &C {
    type Error = C::Error;

    fn send(&self, request: HttpRequest) -> Result<HttpResponse, C::Error> {
        (**self).send(request)
    }
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

/// The same for the async flavour, for the same reason and at the same cost.
///
/// Two things differ. The future handed back is `C`'s own and the trait already
/// declares that one `Send`, so nothing here asks for `C: Sync`: what crosses a
/// `tokio::spawn` is the inner future, not the reference, and a client whose
/// answer is ready before the future is built is free to be `!Sync`. And the
/// trait returns `impl Future`, which makes it dyn-incompatible, so there is no
/// `&dyn AsyncClient` for `?Sized` to reach here — it is written this way so the
/// two impls are one shape rather than two rules to remember.
impl<C: AsyncClient + ?Sized> AsyncClient for &C {
    type Error = C::Error;

    fn send(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, C::Error>> + Send {
        (**self).send(request)
    }
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
    /// What a request no queue has an answer for gets: `200 {}` when this is
    /// false, a panic when [`Recorder::strict`] has set it.
    strict: bool,
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

/// How a route reads in the refusal a strict recorder panics with, and the
/// spelling the two constructors that queue for one take it in.
impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.method, self.path)
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
            Answer::Response(json_response(status, body)),
        )
    }

    /// Queue `response` — headers and all — for the next request on
    /// `method path`.
    ///
    /// The long spelling of [`Self::answering_route`], for a caller that
    /// branches on something a status and a JSON body cannot carry.
    /// [`json_response`] builds what the short spelling builds, to add a header
    /// to.
    #[must_use]
    pub fn answering_route_with(self, method: Method, path: &str, response: HttpResponse) -> Self {
        self.queue_for(Route::new(method, path), Answer::Response(response))
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
        self.queue_for_anything(Answer::Response(json_response(status, body)))
    }

    /// Queue `response` — headers and all — for the next request whose own
    /// route has nothing left.
    ///
    /// The long spelling of [`Self::answering`], for a caller that branches on
    /// something a status and a JSON body cannot carry.
    #[must_use]
    pub fn answering_with(self, response: HttpResponse) -> Self {
        self.queue_for_anything(Answer::Response(response))
    }

    /// Queue a failure for the next request whose own route has nothing left.
    #[must_use]
    pub fn failing(self, message: &str) -> Self {
        self.queue_for_anything(Answer::Failure(message.to_owned()))
    }

    /// Refuse a request no queue has an answer for, instead of answering
    /// `200 {}`.
    ///
    /// The refusal is a panic naming the route and what the queues still hold,
    /// because the mistake is in the test rather than in the code under test
    /// and a panic reports it on the line that made it. Takes effect whenever
    /// it is called: a script is the same script either way.
    #[must_use]
    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
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
            None if self.strict => unscripted(&route, &held),
            None => Ok(json_response(StatusCode::OK, &serde_json::json!({}))),
        }
    }
}

/// What a strict recorder says about a request it was never given an answer
/// for.
///
/// The routes are sorted because they come out of a `HashMap`, and a message
/// that reads differently on every run is a message a reader stops trusting.
fn unscripted(route: &Route, held: &Held) -> ! {
    let mut queued: Vec<String> = held
        .per_route
        .iter()
        .filter(|(_, answers)| !answers.is_empty())
        .map(|(route, answers)| format!("{route} ({} left)", answers.len()))
        .collect();
    queued.sort();
    panic!(
        "the recorder was asked for `{route}` and has no answer for it. It is holding {queued:?} \
         and {} queued for anything. Queue one with `answering_route` or `answering`, or drop \
         `strict()` to let it answer 200 {{}}.",
        held.anything.len(),
    );
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

/// A JSON response, which is what the short spelling of a scripted answer
/// builds: `body` rendered into the body and `content-type: application/json`.
///
/// Public so that "the same answer, plus a header" does not start by rebuilding
/// this — take one, insert the header, and queue it with
/// [`Recorder::answering_with`] or [`Recorder::answering_route_with`].
#[must_use]
pub fn json_response(status: StatusCode, body: &serde_json::Value) -> HttpResponse {
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
        AsyncClient, HttpRequest, HttpResponse, Method, Recorder, RecorderError, Request,
        StatusCode, SyncClient, header, json_response,
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

    #[test]
    fn a_response_queued_whole_keeps_the_headers_it_was_built_with() {
        let mut page = json_response(StatusCode::OK, &json!([{"id": 5}]));
        page.headers_mut().insert(
            header::LINK,
            header::HeaderValue::from_static(r#"</vouchers?page=2>; rel="next""#),
        );
        let mut created = json_response(StatusCode::CREATED, &json!(null));
        created.headers_mut().insert(
            header::LOCATION,
            header::HeaderValue::from_static("/vouchers/6"),
        );

        let client = Recorder::new()
            .answering_route_with(Method::GET, "/vouchers", page)
            .answering_with(created);

        let listed = sent(&client, Method::GET, "/vouchers");
        assert_eq!(
            listed.headers()[header::LINK],
            r#"</vouchers?page=2>; rel="next""#
        );
        assert_eq!(body(&listed), json!([{"id": 5}]));

        // The anything queue carries a whole response too.
        let made = sent(&client, Method::POST, "/vouchers");
        assert_eq!(made.status(), StatusCode::CREATED);
        assert_eq!(made.headers()[header::LOCATION], "/vouchers/6");
    }

    #[test]
    fn the_long_spelling_of_an_answer_and_the_short_one_build_the_same_response() {
        let short = Recorder::new().answering(StatusCode::OK, &json!({"id": 5}));
        let long = Recorder::new().answering_with(json_response(StatusCode::OK, &json!({"id": 5})));

        let from_short = sent(&short, Method::GET, "/vouchers");
        let from_long = sent(&long, Method::GET, "/vouchers");

        assert_eq!(from_short.status(), from_long.status());
        assert_eq!(from_short.headers(), from_long.headers());
        assert_eq!(from_short.body(), from_long.body());
    }

    /// Two routes are held, so the sort in the refusal is load-bearing: a
    /// `HashMap` hands them over in a different order on every run, and this
    /// test flakes without it.
    #[test]
    #[should_panic(
        expected = "asked for `GET /vouchers` and has no answer for it. It is holding \
                    [\"POST /vouchers (2 left)\", \"PUT /vouchers/5 (1 left)\"] and 0 queued \
                    for anything"
    )]
    fn a_strict_recorder_refuses_what_nobody_queued_and_names_what_it_holds() {
        let client = Recorder::new()
            .strict()
            .answering_route(Method::POST, "/vouchers", StatusCode::CREATED, &json!(null))
            .answering_route(Method::POST, "/vouchers", StatusCode::CREATED, &json!(null))
            .answering_route(Method::PUT, "/vouchers/5", StatusCode::OK, &json!(null));

        let _refused = SyncClient::send(&client, request(Method::GET, "/vouchers"));
    }

    #[test]
    fn a_strict_recorder_answers_and_fails_from_the_script_like_a_lenient_one() {
        let client = Recorder::new()
            .strict()
            .answering_route(Method::GET, "/vouchers", StatusCode::OK, &json!("the list"))
            .failing_route(Method::POST, "/vouchers", "the request never left");

        assert_eq!(
            body(&sent(&client, Method::GET, "/vouchers")),
            json!("the list")
        );
        assert_eq!(
            SyncClient::send(&client, request(Method::POST, "/vouchers"))
                .expect_err("scripted")
                .message(),
            "the request never left"
        );
        assert_eq!(client.take().len(), 2, "both went out");
    }

    #[test]
    #[should_panic(expected = "asked for `GET /vouchers`")]
    fn the_async_path_refuses_when_send_is_called_rather_than_when_it_is_awaited() {
        let client = Recorder::new().strict();

        // Never awaited: the work happens in `send`, so the refusal lands on
        // this line rather than wherever the future is polled.
        let _refused = AsyncClient::send(&client, request(Method::GET, "/vouchers"));
    }

    /// An async client that is not `Sync`, and whose future is `Send` all the
    /// same because the answer is ready before the future is built.
    ///
    /// The `Cell` is the whole of it: an atomic counter here would make `Eager`
    /// `Sync` and the test below would stop asking its question.
    struct Eager(std::cell::Cell<usize>);

    impl AsyncClient for Eager {
        type Error = RecorderError;

        fn send(
            &self,
            _request: HttpRequest,
        ) -> impl Future<Output = Result<HttpResponse, RecorderError>> + Send {
            self.0.set(self.0.get() + 1);
            std::future::ready(Ok(json_response(
                StatusCode::OK,
                &json!({ "answered": self.0.get() }),
            )))
        }
    }

    /// A reference to a client is a client on the async path, and the client it
    /// refers to owes no `Sync`.
    ///
    /// What the blanket impl hands back is the future `Eager` itself declares
    /// `Send`, so the reference never has to cross a thread for the promise to
    /// hold — `is_send` is where that is pinned rather than assumed.
    #[test]
    fn a_reference_to_a_client_is_an_async_client_and_the_client_owes_no_sync() {
        fn is_send<F: Send>(future: F) -> F {
            future
        }

        let client = Eager(std::cell::Cell::new(0));
        let through_reference: &Eager = &client;

        let pending = is_send(AsyncClient::send(
            &through_reference,
            request(Method::GET, "/vouchers"),
        ));
        let answered = block_on(pending).expect("the eager client answers");

        assert_eq!(body(&answered), json!({ "answered": 1 }));
    }
}
