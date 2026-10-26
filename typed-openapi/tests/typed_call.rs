//! A typed call sends the request the gate decided on, and its errors name the
//! operation.
//!
//! The gate takes an `Invocation`, which holds its own operation, so the verdict
//! and the request it is about cannot come from two operations. A `Call` plans
//! with what a caller answered and sends what it planned, so a request that is
//! printed as a dry run is the request that goes out when the write is
//! confirmed. The four errors that a call raises once it sends (`Status`,
//! `Decode`, `Transport` and `Encode`) say which operation it was, and no
//! message and no cause of theirs prints a byte the server sent: the body is a
//! field. The cause of a `Transport` is the client's own error and prints what
//! that client prints. `Request` comes before anything is sent and names no
//! operation.
//!
//! The inputs are the probes of SEV-249: `A1/errs` (`chains`) for the 500 with a
//! body that holds a credential, and the code reading of F-33 for the pairing.
//! The bodies of `refused` are the Code Reviewer's probe of the chain that a
//! `Decode` prints (SEV-283).

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
use typed_openapi::client::{Client, DecodeClass, Error as ClientError, Planned, to_json};
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
  /tags/{tag}:
    put:
      operationId: tagThing
      parameters:
        - { name: tag, in: path, required: true, schema: { type: string } }
        - { name: note, in: query, schema: { type: string } }
        - { name: X-Token, in: header, schema: { type: string } }
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                name: { type: string }
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

/// A type that names every property it takes and refuses the rest, so serde's
/// own message quotes a value, a key or a variant of the body it was given.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Strict {
    id: i64,
    kind: Option<Kind>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
enum Kind {
    A,
    B,
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
/// returns is that invocation's own, so a write is held back whatever a read
/// answers beside it.
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

/// A `Debug` goes to logs, and the request holds the path, the query, the header
/// values and the body the caller gave. `http::Request` prints a body as a list
/// of numbers and everything else as text, so the secret is in each of the four.
#[test]
fn the_debug_of_a_planned_call_shows_no_request() {
    let client = client();
    let doc = client.document();
    let call = client
        .call::<Thing>(
            doc.get("tagThing").expect("the write"),
            Values::new()
                .param("tag", "path-secret")
                .param("note", "query-secret")
                .param("X-Token", "header-secret")
                .json(json!({"name": "body-secret"})),
        )
        .expect("values");

    let planned: Planned<'_, Thing> = call.plan(&Answers::new()).expect("a plan");
    let debug = format!("{planned:?}");

    assert!(debug.contains("tagThing"), "{debug}");
    assert_eq!(
        planned.request().uri().path(),
        "/api/tags/path-secret",
        "the request does hold the path the secret stands in"
    );
    for secret in [
        "path-secret",
        "query-secret",
        "header-secret",
        "body-secret",
    ] {
        assert!(!debug.contains(secret), "{secret} is in {debug}");
    }
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
    assert!(
        matches!(
            &error,
            ClientError::Status { op, status, .. }
                if op == "getThing" && *status == StatusCode::NOT_FOUND
        ),
        "{error:?}"
    );
}

/// `A2` (`F-23`): a gateway's HTML page of 3,013 bytes. The message of a status
/// is the same for it as for an empty body, and the page is in the error.
#[test]
fn a_status_says_the_same_whatever_the_body_holds() {
    let client = client();
    let doc = client.document();
    let page = format!(
        "<html><body>{}</body></html>",
        "<p>upstream</p>".repeat(200)
    );
    let empty = Vec::new();

    let errors: Vec<ClientError> = [page.clone().into_bytes(), empty]
        .into_iter()
        .map(|body| {
            let mut response = http::Response::new(body);
            *response.status_mut() = StatusCode::BAD_GATEWAY;
            let recorder = Recorder::new().answering_with(response);
            client
                .call::<Thing>(doc.get("getThing").expect("the read"), id(5))
                .expect("values")
                .send(&recorder)
                .expect_err("a 502 is not a success")
        })
        .collect();

    let [with_page, with_nothing] = <[ClientError; 2]>::try_from(errors).expect("two errors");
    assert_eq!(
        with_page.to_string(),
        "getThing: the server answered 502 Bad Gateway"
    );
    assert_eq!(with_page.to_string(), with_nothing.to_string());
    assert!(
        matches!(
            &with_page,
            ClientError::Status { op, status, body }
                if op == "getThing" && *status == StatusCode::BAD_GATEWAY && *body == page
        ),
        "{with_page:?}"
    );
    assert!(
        matches!(
            &with_nothing,
            ClientError::Status { op, status, body }
                if op == "getThing" && *status == StatusCode::BAD_GATEWAY && body.is_empty()
        ),
        "{with_nothing:?}"
    );
}

/// The planned road on a runtime has its own call to the parser and to the
/// client, so it names the operation on its own: a status and a failing client.
#[test]
fn an_async_planned_call_names_the_operation_of_its_failure() {
    let client = client();
    let doc = client.document();
    let op = doc.get("getThing").expect("the read");
    let planned = || {
        client
            .call::<Thing>(op, id(5))
            .expect("values")
            .plan(&Answers::new())
            .expect("a plan")
    };

    let status = block_on(planned().send_async(
        &Recorder::new().answering(StatusCode::SERVICE_UNAVAILABLE, &json!({"error": "no"})),
    ))
    .expect_err("a 503 is not a success");
    let transport =
        block_on(planned().send_async(&Recorder::new().failing(Reach::NeverAnswered, "timed out")))
            .expect_err("the client fails");
    let decode = block_on(
        planned().send_async(&Recorder::new().answering(StatusCode::OK, &json!({"id": "x"}))),
    )
    .expect_err("the body is not a `Thing`");

    assert!(
        matches!(&status, ClientError::Status { op, status, .. }
            if op == "getThing" && *status == StatusCode::SERVICE_UNAVAILABLE),
        "{status:?}"
    );
    assert!(
        matches!(&transport, ClientError::Transport { op, .. } if op == "getThing"),
        "{transport:?}"
    );
    assert!(
        matches!(&decode, ClientError::Decode { op, .. } if op == "getThing"),
        "{decode:?}"
    );
}

/// A 200 whose body is `body`, read as a `Strict` by a typed call to `getThing`.
fn decode_failure(body: &str) -> ClientError {
    let client = client();
    let doc = client.document();
    let recorder = Recorder::new().answering_with(http::Response::new(body.as_bytes().to_vec()));

    client
        .call::<Strict>(doc.get("getThing").expect("the read"), id(5))
        .expect("values")
        .send(&recorder)
        .expect_err("the body is not a `Strict`")
}

/// Every message down the chain of `error`, outermost first, joined.
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    std::iter::successors(Some(error), |&layer| layer.source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" / ")
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
    assert!(
        error.source().is_some(),
        "where the read stopped is the cause"
    );
    assert!(error.body().is_some_and(|held| held.contains("\"x\"")));
}

/// A 200 that a `Strict` refuses, the words of its body that serde quotes in
/// its own message, and the column where serde stopped.
struct Refused {
    body: String,
    quoted: &'static [&'static str],
    column: usize,
}

/// The Code Reviewer's three bodies, which serde reads as `invalid type: string
/// "IBAN AT61 …, token=abc", expected i64`, ``unknown field `token=abc` `` and
/// ``unknown variant `secret-variant-token=abc` ``, and a string value of 3,000
/// characters.
fn refused() -> [Refused; 4] {
    [
        Refused {
            body: r#"{"id":"IBAN AT61 1904 3002 3457 3201, token=abc"}"#.to_owned(),
            quoted: &["IBAN", "AT61", "token=abc"],
            column: 48,
        },
        Refused {
            body: r#"{"id":1,"token=abc":1}"#.to_owned(),
            quoted: &["token=abc"],
            column: 19,
        },
        Refused {
            body: r#"{"id":1,"kind":"secret-variant-token=abc"}"#.to_owned(),
            quoted: &["secret-variant", "token=abc"],
            column: 41,
        },
        Refused {
            body: format!(r#"{{"id":"{}"}}"#, "x".repeat(3000)),
            quoted: &["xxxx"],
            column: 3008,
        },
    ]
}

/// F-23 with D7: what serde says about a value, a key or a variant of the body
/// reaches a log through `source()`, so the cause of a `Decode` keeps none of
/// it. The chain stays as short as for any other failure, whatever the size of
/// the value.
#[test]
fn a_decode_chain_holds_no_byte_of_the_body() {
    for Refused { body, quoted, .. } in refused() {
        let error = decode_failure(&body);

        let chain = chain(&error);

        assert!(
            matches!(&error, ClientError::Decode { op, .. } if op == "getThing"),
            "{error:?}"
        );
        for word in quoted {
            assert!(!chain.contains(word), "`{word}` is in {chain:?}");
        }
        assert!(chain.len() < 200, "{} bytes: {chain:?}", chain.len());
        assert_eq!(
            error.body(),
            Some(body.as_str()),
            "the body is readable from the error"
        );
    }
}

/// The cause of a `Decode` says what kind of failure it was and where the read
/// stopped, which is what serde's own message says about the body besides the
/// bytes of it.
#[test]
fn a_decode_cause_says_the_class_the_line_and_the_column() {
    for Refused { body, column, .. } in refused() {
        let error = decode_failure(&body);

        let ClientError::Decode { source, .. } = &error else {
            panic!("a decode: {error:?}");
        };

        assert_eq!(source.class, DecodeClass::Data, "{body}");
        assert_eq!((source.line, source.column), (1, column), "{body}");
        assert_eq!(
            source.to_string(),
            format!("the body does not fit the type at line 1 column {column}")
        );
    }
}

/// A body that is not JSON, one that ends early and one that is JSON of another
/// type are three classes, and a body of several lines puts the read on the
/// line it stopped at.
#[test]
fn a_decode_cause_tells_the_three_classes_apart() {
    for (body, class, line, column, words) in [
        (
            "not json",
            DecodeClass::Syntax,
            1,
            2,
            "the body is not JSON at line 1 column 2",
        ),
        (
            r#"{"id":"#,
            DecodeClass::Eof,
            1,
            6,
            "the body ends early at line 1 column 6",
        ),
        (
            "{\n  \"id\": 1,\n  \"kind\": \"C\"\n}",
            DecodeClass::Data,
            3,
            13,
            "the body does not fit the type at line 3 column 13",
        ),
    ] {
        let error = decode_failure(body);

        let ClientError::Decode { source, .. } = &error else {
            panic!("a decode: {error:?}");
        };

        assert_eq!(source.class, class, "{body}");
        assert_eq!((source.line, source.column), (line, column), "{body}");
        assert_eq!(source.to_string(), words);
    }
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
