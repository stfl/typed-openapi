//! A typed call sends the request the gate decided on, and its errors name the
//! operation.
//!
//! The gate takes an `Invocation`, which holds its own operation, so the verdict
//! and the request it is about cannot come from two operations. A `Call` plans
//! with what a caller answered and sends what it planned, so a request that is
//! printed as a dry run is the request that goes out when the write is
//! confirmed. Every error that a call raises when it runs says which operation
//! it was, and none of them prints bytes the server sent: the body is a field.
//!
//! The inputs are the probes of SEV-249: `A1/errs` (`chains`) for the 500 with a
//! body that holds a credential, and the code reading of F-33 for the pairing.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::collections::BTreeMap;
use std::error::Error as _;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use typed_openapi::client::{Client, Error as ClientError, Planned, to_json};
use typed_openapi::{
    Answers, Document, Invocation, Plan, Reach, Recorder, RecorderError, Values, render,
};

/// A read, a write, a write that stands behind a gate, and a write that takes a
/// body: the four things the gate tells apart.
const THINGS: &str = r#"
openapi: 3.0.3
info: { title: Things, version: "1" }
servers: [{ url: "http://things.example.test/api" }]
paths:
  /things/{id}:
    get:
      operationId: getThing
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses: { "200": { description: ok } }
    delete:
      operationId: deleteThing
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses: { "200": { description: ok } }
  /things/{id}/enshrine:
    post:
      operationId: enshrineThing
      x-cli-gates: [enshrine]
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses: { "200": { description: ok } }
  /things:
    post:
      operationId: createThing
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                name: { type: string }
      responses: { "200": { description: ok } }
"#;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Thing {
    id: i64,
}

fn document() -> Document {
    Document::load(THINGS, &[]).expect("the document loads")
}

/// A client that was built with no `?` and no `expect`: `Client::over` cannot
/// fail, so it has no `Result` to unwrap.
fn client() -> Client {
    Client::over(document())
}

fn id(value: i64) -> Values {
    Values::new().param("id", value)
}

/// A runtime in six lines: the recorder never yields, so the first poll is ready.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

/// The one request the recorder was given.
fn only(recorder: &Recorder) -> http::Request<Vec<u8>> {
    let mut sent = recorder.take();
    assert_eq!(sent.len(), 1, "one request went out");
    sent.pop().expect("one request")
}

/// The request as a reader sees it, and its body.
fn shown(request: &http::Request<Vec<u8>>) -> (String, Vec<u8>) {
    (render(request), request.body().clone())
}

/// The gate decides about the operation the invocation holds, and the request it
/// returns is that invocation's own. A write beside a read's verdict was `Send`
/// when the two came in as separate arguments.
#[test]
fn a_plan_is_decided_on_the_invocation_and_carries_its_own_request() {
    let doc = document();
    let read = Invocation::new(doc.get("getThing").expect("the read"), id(5)).expect("values");
    let write = Invocation::new(doc.get("deleteThing").expect("the write"), id(5)).expect("values");

    let reading = Plan::decide(&read, doc.base(), &Answers::new()).expect("a request");
    let writing = Plan::decide(&write, doc.base(), &Answers::new()).expect("a request");

    assert!(matches!(reading, Plan::Send(_)), "a read is sent on sight");
    assert!(
        matches!(writing, Plan::DryRun(_)),
        "a write is held back, whatever a read answers beside it"
    );
    for (plan, invocation) in [(&reading, &read), (&writing, &write)] {
        let own = invocation.request(doc.base()).expect("a request");
        assert_eq!(shown(plan.request()), shown(&own));
    }
    assert_ne!(
        plan_method(&reading),
        plan_method(&writing),
        "the two plans are about two operations"
    );

    let confirmed = Plan::decide(&write, doc.base(), &Answers::new().commit()).expect("a request");
    assert!(matches!(confirmed, Plan::Send(_)));
    assert_eq!(shown(confirmed.request()), shown(writing.request()));
}

fn plan_method(plan: &Plan) -> &http::Method {
    plan.request().method()
}

/// A request that cannot be built is no plan: the base is refused as it is on
/// every road.
#[test]
fn a_base_the_request_cannot_be_built_from_is_no_plan() {
    let doc = document();
    let read = Invocation::new(doc.get("getThing").expect("the read"), id(5)).expect("values");
    let base = "http://alice:s3cret@things.example.test/"
        .parse()
        .expect("a URI");

    let refused = Plan::decide(&read, &base, &Answers::new()).expect_err("userinfo is refused");

    assert!(
        matches!(
            refused,
            typed_openapi::RequestError::Base(typed_openapi::BaseUrlError::Userinfo)
        ),
        "{refused:?}"
    );
}

/// The typed road: plan with the answers, look at the request, send the plan.
/// What was looked at is what went out, byte for byte, and the answer is typed.
#[test]
fn a_call_sends_the_request_it_planned() {
    let client = client();
    let doc = client.document();
    let call = client
        .call::<Thing>(doc.get("deleteThing").expect("the write"), id(5))
        .expect("values");
    let recorder = Recorder::new().answering(StatusCode::OK, &json!({"id": 5}));

    let planned = call.plan(&Answers::new().commit()).expect("a plan");
    assert!(matches!(planned.plan(), Plan::Send(_)));
    let looked_at = shown(planned.request());
    let answer = planned.send(&recorder).expect("the server answers");

    assert_eq!(answer, Some(Thing { id: 5 }));
    let sent = only(&recorder);
    assert_eq!(shown(&sent), looked_at);
    assert_eq!(sent.method(), "DELETE");
}

/// A write that nobody confirmed is a plan with a request to print and nothing to
/// send: there is no answer, and the client is not asked.
#[test]
fn a_planned_dry_run_sends_nothing_and_has_no_answer() {
    let client = client();
    let doc = client.document();
    let call = client
        .call::<Thing>(doc.get("deleteThing").expect("the write"), id(5))
        .expect("values");
    let recorder = Recorder::new().strict();

    let planned = call.plan(&Answers::new()).expect("a plan");
    assert!(matches!(planned.plan(), Plan::DryRun(_)));
    assert_eq!(planned.request().method(), "DELETE");
    let answer = planned.send(&recorder).expect("nothing to fail");

    assert_eq!(answer, None);
    assert!(recorder.take().is_empty(), "nothing went out");
}

/// A read is sent on sight by the gate, so planning it asks for nothing.
#[test]
fn a_planned_read_is_sent_without_an_answer() {
    let client = client();
    let doc = client.document();
    let call = client
        .call::<Thing>(doc.get("getThing").expect("the read"), id(5))
        .expect("values");
    let recorder = Recorder::new().answering(StatusCode::OK, &json!({"id": 5}));

    let answer = call
        .plan(&Answers::new())
        .expect("a plan")
        .send(&recorder)
        .expect("the server answers");

    assert_eq!(answer, Some(Thing { id: 5 }));
}

/// A named gate is asked of a typed call as it is of a command line, beside the
/// confirmation and not in its place.
#[test]
fn a_planned_call_is_held_by_every_gate_its_operation_names() {
    let client = client();
    let doc = client.document();
    let op = doc.get("enshrineThing").expect("the gated write");
    let call = || client.call::<Thing>(op, id(5)).expect("values");

    for (answers, sent) in [
        (Answers::new(), false),
        (Answers::new().commit(), false),
        (Answers::new().gate("enshrine"), false),
        (Answers::new().commit().gate("enshrine"), true),
    ] {
        let planned = call().plan(&answers).expect("a plan");
        assert_eq!(matches!(planned.plan(), Plan::Send(_)), sent, "{answers:?}");
    }
}

/// The async road is the same decision and the same bytes.
#[test]
fn an_async_client_sends_the_request_that_was_planned() {
    let client = client();
    let doc = client.document();
    let op = doc.get("deleteThing").expect("the write");
    let recorder = Recorder::new().answering(StatusCode::OK, &json!({"id": 5}));

    let planned = client
        .call::<Thing>(op, id(5))
        .expect("values")
        .plan(&Answers::new().commit())
        .expect("a plan");
    let looked_at = shown(planned.request());
    let answer = block_on(planned.send_async(&recorder)).expect("the server answers");

    assert_eq!(answer, Some(Thing { id: 5 }));
    assert_eq!(shown(&only(&recorder)), looked_at);

    let held = client
        .call::<Thing>(op, id(5))
        .expect("values")
        .plan(&Answers::new())
        .expect("a plan");
    assert_eq!(
        block_on(held.send_async(&recorder)).expect("nothing to fail"),
        None
    );
    assert!(recorder.take().is_empty());
}

/// A `Debug` goes to logs, and the request holds the header values and the body
/// the caller gave.
#[test]
fn the_debug_of_a_planned_call_shows_no_request() {
    let client = client();
    let doc = client.document();
    let call = client
        .call::<Thing>(
            doc.get("createThing").expect("the write"),
            Values::new().json(json!({"name": "token=abc"})),
        )
        .expect("values");

    let planned: Planned<'_, Thing> = call.plan(&Answers::new()).expect("a plan");
    let debug = format!("{planned:?}");

    assert!(debug.contains("createThing"), "{debug}");
    assert!(!debug.contains("token=abc"), "{debug}");
}

/// `A1/errs` (`chains`): a 500 whose body holds a credential. The message names
/// the operation and the status and prints none of the body, and the body is
/// still there for a caller that wants it.
#[test]
fn a_status_names_the_operation_and_the_status_and_prints_no_body() {
    let client = client();
    let doc = client.document();
    let body = "upstream said: IBAN AT61 1904 3002 3457 3201, token=abc";
    let recorder =
        Recorder::new().answering(StatusCode::INTERNAL_SERVER_ERROR, &json!({ "error": body }));

    let error = client
        .call::<Thing>(doc.get("getThing").expect("the read"), id(5))
        .expect("values")
        .send(&recorder)
        .expect_err("a 500 is not a success");

    assert_eq!(
        error.to_string(),
        "getThing: the server answered 500 Internal Server Error"
    );
    assert!(
        matches!(
            &error,
            ClientError::Status { op, status, body: held }
                if op == "getThing"
                    && *status == StatusCode::INTERNAL_SERVER_ERROR
                    && held.contains(body)
        ),
        "{error:?}"
    );
    assert!(error.source().is_none(), "a status has no cause to print");
    assert!(
        error.body().is_some_and(|held| held.contains("token=abc")),
        "the body is readable from the error"
    );
}

/// The same refusal on the async road, which has its own call to the parser.
#[test]
fn an_async_status_names_the_operation_too() {
    let client = client();
    let doc = client.document();
    let recorder = Recorder::new().answering(StatusCode::NOT_FOUND, &json!({"error": "no"}));

    let error = block_on(
        client
            .call::<Thing>(doc.get("getThing").expect("the read"), id(404))
            .expect("values")
            .send_async(&recorder),
    )
    .expect_err("a 404 is not a success");

    assert_eq!(
        error.to_string(),
        "getThing: the server answered 404 Not Found"
    );
}

/// A success whose body is not the type the document promises names the
/// operation and keeps the text it could not read.
#[test]
fn a_decode_names_the_operation_and_keeps_the_text() {
    let client = client();
    let doc = client.document();
    let recorder = Recorder::new().answering(StatusCode::OK, &json!({"id": "x"}));

    let error = client
        .call::<Thing>(doc.get("getThing").expect("the read"), id(5))
        .expect("values")
        .send(&recorder)
        .expect_err("the body is not a `Thing`");

    assert_eq!(
        error.to_string(),
        "getThing: the response is not the shape the document promises"
    );
    assert!(
        matches!(
            &error,
            ClientError::Decode { op, raw, .. } if op == "getThing" && raw.contains("\"x\"")
        ),
        "{error:?}"
    );
    assert!(error.source().is_some(), "serde's reading is the cause");
    assert!(error.body().is_some_and(|held| held.contains("\"x\"")));
}

/// A failure of the client names the operation, on the planned road and on the
/// plain one, and the client's own error is the cause.
#[test]
fn a_transport_failure_names_the_operation_and_carries_the_clients_error() {
    let client = client();
    let doc = client.document();
    let op = doc.get("getThing").expect("the read");
    let recorder = Recorder::new().failing(Reach::NeverLeft, "connection refused");

    let plain = client
        .call::<Thing>(op, id(5))
        .expect("values")
        .send(&recorder)
        .expect_err("the client fails");
    let planned = client
        .call::<Thing>(op, id(5))
        .expect("values")
        .plan(&Answers::new())
        .expect("a plan")
        .send(&Recorder::new().failing(Reach::NeverAnswered, "timed out"))
        .expect_err("the client fails");

    for (error, reach) in [(&plain, Reach::NeverLeft), (&planned, Reach::NeverAnswered)] {
        assert_eq!(error.to_string(), "getThing: the transport failed");
        let ClientError::Transport { op, source } = error else {
            panic!("a transport failure: {error:?}");
        };
        assert_eq!(op, "getThing");
        let failure = source
            .downcast_ref::<RecorderError>()
            .expect("the box holds the client's own error");
        assert_eq!(failure.reach(), reach);
        assert!(error.body().is_none(), "no response arrived");
    }
}

/// A typed argument that does not serialise is refused with the operation it was
/// the body of, which the generated wrapper names.
#[test]
fn an_argument_that_does_not_serialise_names_the_operation() {
    let not_json = BTreeMap::from([((1, 2), 3)]);

    let error = to_json("createThing", &not_json).expect_err("a JSON key is a string");

    assert_eq!(
        error.to_string(),
        "createThing: cannot serialise the request body"
    );
    assert!(
        matches!(&error, ClientError::Encode { op, .. } if op == "createThing"),
        "{error:?}"
    );
    assert!(
        error.source().is_some(),
        "serde_json's reading is the cause"
    );
    assert!(error.body().is_none());
}

/// What a call raises before it runs is not a call-time error and has no
/// operation of its own: the values name it already.
#[test]
fn a_refused_value_is_the_values_own_error() {
    let client = client();
    let doc = client.document();

    let error = client
        .call::<Thing>(doc.get("getThing").expect("the read"), Values::new())
        .expect_err("the id is required");

    assert!(matches!(error, ClientError::Values(_)), "{error:?}");
    assert!(error.body().is_none());
}
