//! A typed call sends the request the gate decided on, and its errors name the
//! operation.
//!
//! The gate takes an `Invocation`, which holds its own operation, so the verdict
//! and the request it is about cannot come from two operations. A `Call` plans
//! with what a caller answered and sends what it planned, so a request that is
//! printed as a dry run is the request that goes out when the write is
//! confirmed. Four errors carry the operation as a field, `op`. `to_json` raises
//! `Encode` while a wrapper builds the arguments of a call, before the call
//! exists, and `Transport`, `Status` and `Decode` come once the request goes to
//! the client. No message of theirs and no cause of a `Decode` prints a byte the
//! server sent: the body is a field. The cause of a `Transport` is the client's
//! own error and prints what that client prints. `Values` names the operation in
//! the message of its `ValueError`. `Request` comes after the call exists and
//! before anything is sent, and names no operation.
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
use typed_openapi::client::{
    Client, DecodeClass, DecodeError, Error as ClientError, Planned, to_json,
};
use typed_openapi::{
    Answers, Document, Invocation, NoContent, Plan, Reach, Recorder, RecorderError, ValueError,
    Values, render,
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

/// `A2` (`F-23`): a gateway's HTML page of 3,026 bytes. The message of a status
/// is the same for it as for an empty body, and the page is in the error.
#[test]
fn a_status_says_the_same_whatever_the_body_holds() {
    let client = client();
    let doc = client.document();
    let page = format!(
        "<html><body>{}</body></html>",
        "<p>upstream</p>".repeat(200)
    );
    assert_eq!(page.len(), 3_026, "the doc names the size of the page");
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
    failure_as::<Strict>(body)
}

/// A 200 whose body is `body`, read as a `T` by a typed call to `getThing`.
fn failure_as<T: std::fmt::Debug + serde::de::DeserializeOwned>(
    body: impl AsRef<[u8]>,
) -> ClientError {
    let client = client();
    let doc = client.document();
    let recorder = Recorder::new().answering_with(http::Response::new(body.as_ref().to_vec()));

    client
        .call::<T>(doc.get("getThing").expect("the read"), id(5))
        .expect("values")
        .send(&recorder)
        .expect_err("the body is not a `T`")
}

/// The cause of a `Decode`, which the error holds.
fn cause(error: &ClientError) -> DecodeError {
    let ClientError::Decode { source, .. } = error else {
        panic!("a decode: {error:?}");
    };
    *source
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

/// A 200 that a `Strict` refuses, the words of its body (its values and its
/// keys) that serde can quote in its own message, and the column where serde
/// stopped.
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
            quoted: &["IBAN", "AT61", "token=abc", "id"],
            column: 48,
        },
        Refused {
            body: r#"{"id":1,"token=abc":1}"#.to_owned(),
            quoted: &["token=abc", "id"],
            column: 19,
        },
        Refused {
            body: r#"{"id":1,"kind":"secret-variant-token=abc"}"#.to_owned(),
            quoted: &["secret-variant", "token=abc", "id", "kind"],
            column: 41,
        },
        Refused {
            body: format!(r#"{{"id":"{}"}}"#, "x".repeat(3000)),
            quoted: &["xxxx", "id"],
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
        let source = cause(&decode_failure(&body));

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
            "the JSON reader refused the body at line 1 column 2",
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
        let source = cause(&decode_failure(body));

        assert_eq!(source.class, class, "{body}");
        assert_eq!((source.line, source.column), (line, column), "{body}");
        assert_eq!(source.to_string(), words);
    }
}

/// The column is the number of bytes of the line that the reader had read when
/// it stopped, wherever in the body that is. It is 0 where the reader had read
/// none of the line: the first line, a line that a body ends on, a line break
/// inside a string, a list where the type takes an object, and an object where
/// the type takes a list. A syntax error at the first byte of a line is 1,
/// because the reader had read that byte, and an object that lacks a property is
/// refused after its last byte.
#[test]
fn a_decode_column_is_the_bytes_of_the_line_the_reader_had_read() {
    for (body, class, line, column) in [
        ("{\"id\":\n", DecodeClass::Eof, 2, 0),
        ("\n", DecodeClass::Eof, 2, 0),
        ("{\"id\":\"a\nb\"}", DecodeClass::Syntax, 2, 0),
        ("{\"id\":\n[1]}", DecodeClass::Data, 2, 0),
        ("x", DecodeClass::Syntax, 1, 1),
        ("<html>", DecodeClass::Syntax, 1, 1),
        ("\nx", DecodeClass::Syntax, 2, 1),
        ("{\"id\":1,\nx}", DecodeClass::Syntax, 2, 1),
        ("{}", DecodeClass::Data, 1, 2),
    ] {
        let source = cause(&decode_failure(body));

        assert_eq!(source.class, class, "{body:?}");
        assert_eq!((source.line, source.column), (line, column), "{body:?}");
    }

    let source = cause(&failure_as::<Vec<Thing>>("{}"));

    assert_eq!(source.class, DecodeClass::Data);
    assert_eq!((source.line, source.column), (1, 0), "an object for a list");
}

/// The column counts bytes and not characters, so after a character of two bytes
/// it is one more than the characters before it, and where the reader stops on
/// the first byte of a character it falls inside that character.
#[test]
fn a_decode_column_counts_bytes_and_can_fall_inside_a_character() {
    let after = r#"{"id":"ä"}"#;
    let inside = r#"{"id":ä}"#;

    let after_cause = cause(&decode_failure(after));
    let inside_cause = cause(&decode_failure(inside));

    assert_eq!(after_cause.class, DecodeClass::Data);
    assert_eq!(
        after_cause.column,
        after.find('}').expect("the object closes"),
        "the bytes before the closing brace, which are one more than its characters"
    );
    assert_eq!(after.chars().take_while(|&c| c != '}').count(), 9);
    assert_eq!(inside_cause.class, DecodeClass::Syntax);
    assert_eq!((inside_cause.line, inside_cause.column), (1, 7));
    assert!(
        !inside.is_char_boundary(inside_cause.column),
        "column 7 is the first byte of `ä`, which has two"
    );
}

/// `Error::body()` holds the body with each byte that is not UTF-8 replaced by
/// U+FFFD, which is three bytes, so after such a byte the place that the numbers
/// give is earlier in it than in the body.
#[test]
fn a_decode_place_is_later_in_the_text_of_the_body_after_a_byte_that_is_not_utf8() {
    let body = b"{\"zz\":\"\xff\",\"id\":\"x\"}";

    let error = failure_as::<Thing>(body);

    let source = cause(&error);
    let held = error.body().expect("the text of the body");
    assert_eq!(source.class, DecodeClass::Data);
    assert_eq!((source.line, source.column), (1, 18));
    assert_eq!(body.get(source.column..), Some(&b"}"[..]), "the body");
    assert_eq!(held.len(), body.len() + 2, "one byte became three");
    assert_eq!(
        held.as_bytes().get(source.column..),
        Some(&b"x\"}"[..]),
        "the same column in the text of the body is two bytes early"
    );
}

/// An empty body is how "no content" arrives, and a call reads it as `null`:
/// `NoContent` takes it, and a type that refuses `null` fails in that text, not
/// in a body with nothing in it. A struct refuses it as `Data` at line 1 column 4,
/// after the end of `null`, an externally tagged enum as `Syntax` at its first
/// byte, and `raw` is empty.
#[test]
fn an_empty_body_is_read_as_null() {
    let client = client();
    let doc = client.document();
    let empty = || Recorder::new().answering_with(http::Response::new(Vec::new()));

    let none = client
        .call::<NoContent>(doc.get("getThing").expect("the read"), id(5))
        .expect("values")
        .send(&empty())
        .expect("`null` is no content");
    let error = failure_as::<Thing>("");

    assert_eq!(none, NoContent);
    let source = cause(&error);
    assert_eq!(source.class, DecodeClass::Data);
    assert_eq!((source.line, source.column), (1, 4));
    assert_eq!(
        error.to_string(),
        "getThing: the response is not the shape the document promises"
    );
    assert_eq!(
        source.to_string(),
        "the body does not fit the type at line 1 column 4"
    );
    assert!(
        matches!(&error, ClientError::Decode { op, raw, .. } if op == "getThing" && raw.is_empty()),
        "{error:?}"
    );
    assert_eq!(error.body(), Some(""));

    let tagged = cause(&failure_as::<Kind>(""));

    assert_eq!(tagged.class, DecodeClass::Syntax);
    assert_eq!((tagged.line, tagged.column), (1, 1));
}

/// `Syntax` is `serde_json`'s category for a refusal of the form of the text, and
/// it holds JSON that is valid: a number past the range of an `f64`, whatever the
/// type; a nesting past the recursion limit; `null` or a number where the type is
/// an externally tagged enum, in a field too; and a key that is not a number in
/// quotes where the keys of a map are numbers. A number past the range of the
/// type and not of an `f64` is `Data`.
#[test]
fn a_decode_cause_of_valid_json_the_reader_refuses_is_syntax() {
    /// A required field of an externally tagged enum.
    #[derive(Debug, Deserialize)]
    struct Needs {
        #[expect(dead_code, reason = "read by serde only")]
        kind: Kind,
    }
    let nested = format!("{}{}", "[".repeat(200), "]".repeat(200));

    let refused = [
        (decode_failure(r#"{"id":1e999}"#), 1, 11),
        (failure_as::<String>("1e999"), 1, 5),
        (failure_as::<serde_json::Value>(&nested), 1, 128),
        (failure_as::<Kind>("null"), 1, 1),
        (failure_as::<Kind>("5"), 1, 1),
        (failure_as::<Needs>(r#"{"kind":null}"#), 1, 9),
        (failure_as::<BTreeMap<i32, i32>>(r#"{"a":1}"#), 1, 2),
    ];

    for (error, line, column) in refused {
        let source = cause(&error);

        assert_eq!(source.class, DecodeClass::Syntax, "{source}");
        assert_eq!((source.line, source.column), (line, column), "{source}");
        assert_eq!(
            source.to_string(),
            format!("the JSON reader refused the body at line {line} column {column}")
        );
    }

    let past_the_type = cause(&failure_as::<i64>("99999999999999999999"));

    assert_eq!(past_the_type.class, DecodeClass::Data);
    assert_eq!((past_the_type.line, past_the_type.column), (1, 20));
}

/// `serde_json` has no place for some types that refuse a value it had read
/// whole, so the line and the column are 0: an untagged enum that no variant
/// matches, an internally tagged enum whose content is refused, and a type that
/// is read through `try_from` and refuses, at the top of a body, and the
/// untagged enum in a newtype struct and in an `Option` too. A struct that holds
/// the untagged enum reads it through its own field, and its read has a place.
#[test]
fn a_decode_cause_has_no_place_for_a_type_that_refuses_a_value_it_read_whole() {
    /// Taken from an integer only.
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum Number {
        Whole(#[expect(dead_code, reason = "read by serde only")] i64),
    }
    #[derive(Debug, Deserialize)]
    struct Newtype(#[expect(dead_code, reason = "read by serde only")] Number);
    #[derive(Debug, Deserialize)]
    struct Holder {
        #[expect(dead_code, reason = "read by serde only")]
        number: Number,
    }
    /// An enum that reads its tag first and refuses the content.
    #[derive(Debug, Deserialize)]
    #[serde(tag = "t")]
    enum Tagged {
        A {
            #[expect(dead_code, reason = "read by serde only")]
            x: i64,
        },
    }
    /// A natural number, refused by its own `try_from`.
    #[derive(Debug, Deserialize)]
    #[serde(try_from = "i64")]
    struct Natural(#[expect(dead_code, reason = "read by serde only")] i64);
    impl TryFrom<i64> for Natural {
        type Error = &'static str;

        fn try_from(value: i64) -> Result<Self, Self::Error> {
            if value < 0 {
                Err("below 0")
            } else {
                Ok(Self(value))
            }
        }
    }

    let none = [
        failure_as::<Number>(r#""x""#),
        failure_as::<Newtype>(r#""x""#),
        failure_as::<Option<Number>>(r#""x""#),
        failure_as::<Natural>("-1"),
        failure_as::<Tagged>(r#"{"t":"A","x":"s"}"#),
    ];
    let inside = failure_as::<Holder>(r#"{"number":"x"}"#);

    for error in none {
        let source = cause(&error);

        assert_eq!(source.class, DecodeClass::Data, "{error:?}");
        assert_eq!((source.line, source.column), (0, 0), "{error:?}");
        assert_eq!(
            source.to_string(),
            "the body does not fit the type at line 0 column 0"
        );
    }
    let inside = cause(&inside);
    assert_eq!(inside.class, DecodeClass::Data);
    assert_eq!((inside.line, inside.column), (1, 14));
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

/// A refused value is `Values`, which has no `op` field of its own: the message
/// of the `ValueError` it carries begins with the operation.
#[test]
fn a_refused_value_is_the_values_own_error() {
    let client = client();
    let doc = client.document();

    let error = client
        .call::<Thing>(doc.get("getThing").expect("the read"), Values::new())
        .expect_err("the id is required");

    assert!(
        matches!(
            &error,
            ClientError::Values(ValueError::MissingParam { op, name })
                if op == "getThing" && name == "id"
        ),
        "{error:?}"
    );
    assert_eq!(error.to_string(), "getThing: `id` is required");
    assert!(error.body().is_none());
}
