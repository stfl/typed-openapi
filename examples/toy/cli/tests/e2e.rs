//! The CLI, end to end, against a client that records what it was given and
//! answers from a script. No socket, no fixture server, no runtime.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use api::{Api, OperationId};
use cli::app;
use cli::output::Output;
use http::StatusCode;
use typed_openapi::tree::{self, ArgError, Asked, DispatchError};
use typed_openapi::{BaseUrlError, Document, PlanError, Recorder, RequestError, ValueError};

/// The write the report quotes. Every field the document marks required is
/// required on the command line too, so an incomplete PUT is refused here
/// rather than accepted by the server.
const UPDATE: &[&str] = &[
    "toy",
    "raw",
    "vouchers",
    "update",
    "--id",
    "5",
    "--total",
    "12.50",
    "--currency",
    "EUR",
    "--status",
    "draft",
];

/// The chain, with the word its irreversible step stands behind.
///
/// `--enshrine` is required even for a voucher that turns out not to need
/// enshrining: which steps the chain reaches is the server's answer, and it
/// arrives long after the command line is gone.
const FINALIZE: &[&str] = &["toy", "finalize-voucher", "--id", "5", "--enshrine"];

fn api() -> Api {
    Api::new().expect("the embedded document loads")
}

fn voucher(status: &str) -> serde_json::Value {
    serde_json::json!({
        "id": 5, "total": "12.50", "currency": "EUR",
        "status": status, "internal_ref": "AB-7",
    })
}

/// Run one command line against a recorder, and hand back both what the CLI
/// printed and what it sent.
fn run(client: &Recorder, args: &[&str]) -> Output {
    let api = api();
    let matches = app::root(&api).get_matches_from(args);
    app::run(api, client, &matches).unwrap_or_else(|e| panic!("{args:?}: {e}"))
}

#[test]
fn a_write_without_commit_prints_the_request_and_sends_nothing() {
    let client = Recorder::new();
    let out = run(&client, UPDATE);
    assert_eq!(
        out.stdout,
        "PUT /vouchers/5 HTTP/1.1\n\
         host: localhost:9999\n\
         content-type: application/json\n\
         \n\
         {\"total\":\"12.50\",\"currency\":\"EUR\",\"status\":\"draft\"}\n"
    );
    assert_eq!(
        out.stderr,
        "dry run: nothing was sent. Add --commit to send it.\n"
    );
    assert!(client.take().is_empty(), "nothing reached the transport");
}

/// The promise the dry run makes, read back the hard way: what the dry run
/// printed is *parsed* into a method, a URL, headers and a body, and every one
/// of them matches the request `--commit` handed the transport.
///
/// Comparing the two renderings would only prove `render` is deterministic.
#[test]
fn commit_sends_exactly_what_the_dry_run_printed() {
    let dry = run(&Recorder::new(), UPDATE);
    let client = Recorder::new().answering(StatusCode::OK, &voucher("draft"));
    let committed: Vec<&str> = UPDATE.iter().copied().chain(["--commit"]).collect();
    let wet = run(&client, &committed);

    let sent = client.take();
    assert_eq!(sent.len(), 1, "one request, once");
    let printed = Printed::parse(&dry.stdout);
    assert_eq!(printed, Printed::of(&sent[0]));
    assert_eq!(printed.method, "PUT");
    assert_eq!(printed.target, "/vouchers/5");
    assert_eq!(
        printed.body,
        br#"{"total":"12.50","currency":"EUR","status":"draft"}"#.to_vec()
    );

    assert!(
        wet.stdout.contains("\"internal_ref\": \"AB-7\""),
        "{}",
        wet.stdout
    );
    assert!(wet.success);
}

/// `--base-url` is parsed where it arrives, so a value that is not a URL stops
/// the invocation and names itself. The alternative — dropping it and sending
/// to the server the user was overriding — is the one failure a CLI must not
/// have: the request goes somewhere, and it is not where they said.
#[test]
fn a_base_url_that_is_not_a_url_stops_the_invocation() {
    let api = api();
    let args: Vec<&str> = ["toy", "--base-url", "not a url"]
        .into_iter()
        .chain(UPDATE.iter().copied().skip(1))
        .collect();

    let error = app::root(&api)
        .try_get_matches_from(args)
        .expect_err("a malformed --base-url is refused");

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    let shown = error.to_string();
    assert!(shown.contains("not a url"), "{shown}");
    assert!(shown.contains("--base-url"), "{shown}");
}

/// What an operator's terminal shows for `error`, the way `main` prints it: the
/// error, then each cause under it.
fn shown(error: &app::Error) -> String {
    let causes = std::iter::successors(std::error::Error::source(error), |cause| cause.source())
        .map(|cause| format!("  caused by: {cause}"));
    std::iter::once(format!("toy: {error}"))
        .chain(causes)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `--base-url` with a credential or a key in it. Unrefused, a dry run prints the
/// userinfo in its `host:` line, a confirmed run sends it as a `Basic` header, and
/// the query is dropped in silence. The refusal says which part of the URL it is
/// and prints none of it, and it is the same for every verb and for a dry run.
#[test]
fn a_base_url_that_carries_a_credential_stops_the_invocation_and_prints_none_of_it() {
    for (base, variant, part, secrets) in [
        (
            "http://alice:s3cret@127.0.0.1:18473/",
            BaseUrlError::Userinfo,
            "userinfo",
            &["alice", "s3cret"][..],
        ),
        // The `http` crate reads this one as a path, and a dry run would print it.
        (
            "//alice:s3cret@127.0.0.1:18473/",
            BaseUrlError::Userinfo,
            "userinfo",
            &["alice", "s3cret"][..],
        ),
        (
            "http://127.0.0.1:18471/api?key=secret",
            BaseUrlError::Query,
            "query",
            &["key", "secret"][..],
        ),
    ] {
        for verb in [
            &["raw", "vouchers", "render", "--id", "5"][..],
            &["raw", "vouchers", "render", "--id", "5", "--commit"][..],
            &["raw", "vouchers", "list"][..],
            &["finalize-voucher", "--id", "5", "--enshrine"][..],
        ] {
            let client = Recorder::new();
            let api = api();
            let matches = app::root(&api).get_matches_from(
                ["toy", "--base-url", base]
                    .into_iter()
                    .chain(verb.iter().copied()),
            );

            let error = app::run(api, &client, &matches).expect_err("the base URL is refused");

            // Under `raw` the tree refuses it, and the hand-written verb's typed
            // calls refuse it through the client: one refusal, two doors.
            let (app::Error::Dispatch(DispatchError::Plan(PlanError::Request(
                RequestError::Base(refused),
            )))
            | app::Error::Api(typed_openapi::client::Error::Request(RequestError::Base(
                refused,
            )))) = &error
            else {
                panic!("{base} {verb:?}: {error:?}");
            };
            assert_eq!(*refused, variant, "{base} {verb:?}");
            let stderr = shown(&error);
            assert!(stderr.contains(part), "{base} {verb:?}: {stderr}");
            for secret in secrets {
                assert!(
                    !stderr.contains(secret),
                    "{base} {verb:?}: `{secret}` in {stderr}"
                );
            }
            assert!(client.take().is_empty(), "nothing reached the transport");
        }
    }
}

/// A value that starts with a dash is a value. clap reads `-5` as a flag of its
/// own unless the flag is told what it is for, and in an accounting API a negative
/// amount or an offset is ordinary.
#[test]
fn a_negative_number_and_a_text_with_a_leading_dash_are_separate_words() {
    let client = Recorder::new().answering(StatusCode::OK, &serde_json::json!([]));
    run(
        &client,
        &[
            "toy", "raw", "vouchers", "list", "--limit", "-5", "--type", "-x",
        ],
    );
    let sent = client.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].uri().path_and_query().map(ToString::to_string),
        Some("/vouchers?limit=-5&type=-x".to_owned())
    );

    // A body field too, and a credit note is an amount with a sign.
    let credit: Vec<&str> = UPDATE
        .iter()
        .copied()
        .map(|word| if word == "12.50" { "-12.50" } else { word })
        .collect();
    let out = run(&Recorder::new(), &credit);
    assert!(
        out.stdout
            .contains(r#"{"total":"-12.50","currency":"EUR","status":"draft"}"#),
        "{}",
        out.stdout
    );

    // The separate word is checked like the joined one.
    let api = api();
    let refused = app::root(&api)
        .try_get_matches_from(["toy", "raw", "vouchers", "list", "--limit", "-5.5"])
        .expect_err("`-5.5` is not an integer");
    assert_eq!(refused.kind(), clap::error::ErrorKind::ValueValidation);
}

/// The number that is checked is the number that is sent: `+5` is `5` on the wire,
/// and the raw text `%2B5` never is.
#[test]
fn a_number_is_sent_as_it_was_parsed() {
    let client = Recorder::new().answering(StatusCode::OK, &voucher("draft"));
    run(&client, &["toy", "raw", "vouchers", "get", "--id", "+005"]);
    run(
        &client,
        &["toy", "raw", "vouchers", "list", "--limit", "+007"],
    );
    let sent: Vec<String> = client
        .take()
        .iter()
        .filter_map(|request| request.uri().path_and_query().map(ToString::to_string))
        .collect();
    assert_eq!(sent, ["/vouchers/5", "/vouchers?limit=7"]);
}

/// The toy's ids are integers, which refuse `..` as "not an integer" already, so
/// this runs the toy's shape of document with a free-text path parameter through
/// the same tree and the same recorder. `DELETE /things/..` is a request that a
/// server or a proxy reads as the collection above, so it is never built.
#[test]
fn a_path_value_that_is_a_dot_segment_stops_the_invocation_and_sends_nothing() {
    let document = Document::load(
        "openapi: 3.0.3\n\
         info: { title: Things, version: \"1\" }\n\
         servers: [{ url: 'http://localhost:9999' }]\n\
         paths:\n\
         \x20 /things/{name}:\n\
         \x20   delete:\n\
         \x20     operationId: deleteThing\n\
         \x20     parameters:\n\
         \x20       - { name: name, in: path, required: true, schema: { type: string } }\n\
         \x20     responses: { '204': { description: Deleted } }\n",
        &[],
    )
    .expect("a document with a free-text path parameter");
    let root = clap::Command::new("toy")
        .subcommand_required(true)
        .subcommands(tree::commands(&document));

    for value in ["..", ".", ""] {
        let client = Recorder::new();
        let matches = root
            .clone()
            .get_matches_from(["toy", "things", "delete", "--name", value, "--commit"]);
        let Asked::Run(selection) = tree::select(&document, &matches).expect("an operation") else {
            panic!("this command line runs the operation");
        };

        let error = selection
            .send(&client, document.base())
            .expect_err("the segment is refused");

        assert!(
            matches!(
                &error,
                DispatchError::Plan(PlanError::Value(ValueError::PathSegment { name, .. }))
                    if name == "name"
            ),
            "`{value}`: {error:?}"
        );
        assert!(error.to_string().contains("`name`"), "{error}");
        assert!(client.take().is_empty(), "nothing reached the transport");
    }
}

/// A request, as the dry run prints it and as the transport receives it — the
/// two forms this test compares.
#[derive(Debug, PartialEq, Eq)]
struct Printed {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Printed {
    /// Read back what a dry run wrote to stdout.
    fn parse(text: &str) -> Self {
        let (head, body) = text.split_once("\n\n").unwrap_or((text, ""));
        let mut lines = head.lines();
        let request_line = lines.next().expect("a request line");
        let mut words = request_line.split(' ');
        let method = words.next().expect("a method").to_owned();
        let target = words.next().expect("a target").to_owned();
        assert_eq!(words.next(), Some("HTTP/1.1"), "{request_line}");
        let headers = lines
            .map(|line| {
                let (name, value) = line.split_once(": ").expect("a header line");
                (name.to_owned(), value.to_owned())
            })
            .collect();
        Self {
            method,
            target,
            headers,
            body: body.trim_end_matches('\n').as_bytes().to_vec(),
        }
    }

    /// The same pieces, off the request the transport was handed. `host` is a
    /// header on the wire and the authority in `http::Uri`, so it is put back
    /// where the printed form has it.
    fn of(request: &http::Request<Vec<u8>>) -> Self {
        let uri = request.uri();
        let headers = uri
            .authority()
            .map(|authority| ("host".to_owned(), authority.to_string()))
            .into_iter()
            .chain(request.headers().iter().map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    value.to_str().expect("a printable header").to_owned(),
                )
            }))
            .collect();
        Self {
            method: request.method().to_string(),
            target: uri
                .path_and_query()
                .map_or_else(|| "/".to_owned(), ToString::to_string),
            headers,
            body: request.body().clone(),
        }
    }
}

/// The closest a runtime-built tree gets to a compile check, over the tree the
/// binary actually mounts: every operation from the document, the hand-written
/// verbs beside them, and the global flags over both.
#[test]
fn every_command_builds() {
    app::root(&api()).debug_assert();
}

#[test]
fn a_read_carries_no_flag_and_runs_on_sight() {
    let client = Recorder::new().answering(StatusCode::OK, &serde_json::json!([voucher("paid")]));
    let out = run(
        &client,
        &["toy", "raw", "vouchers", "list", "--status", "paid"],
    );
    let sent = client.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].uri().path_and_query().map(ToString::to_string),
        Some("/vouchers?status=paid".to_owned())
    );
    assert!(out.success);
}

/// HTTP says this is a GET. The document says it writes, and the document wins.
#[test]
fn the_writing_get_is_gated_like_any_other_write() {
    let client = Recorder::new();
    let out = run(&client, &["toy", "raw", "vouchers", "render", "--id", "5"]);
    assert!(
        out.stdout.starts_with("GET /vouchers/5/render"),
        "{}",
        out.stdout
    );
    assert!(client.take().is_empty());
}

/// The second question the document asks about this operation, beside "did you
/// mean to write?": mail leaves the building and reaches someone other than the
/// person at the keyboard, so `--commit` alone does not buy it.
#[test]
fn mailing_a_voucher_wants_its_own_word_beside_commit() {
    const RECIPIENT: &[&str] = &[
        "toy",
        "raw",
        "vouchers",
        "send-by-email",
        "--id",
        "5",
        "--recipient",
        "auditor@example.test",
    ];

    let api = api();
    let committed: Vec<&str> = RECIPIENT.iter().copied().chain(["--commit"]).collect();
    let refused = app::root(&api)
        .try_get_matches_from(&committed)
        .expect_err("sendVoucherByEmail stands behind --email");
    assert_eq!(
        refused.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    assert!(refused.to_string().contains("--email"), "{refused}");

    let client = Recorder::new().answering(StatusCode::ACCEPTED, &serde_json::json!(null));
    let both: Vec<&str> = committed.iter().copied().chain(["--email"]).collect();
    let out = run(&client, &both);

    let sent = client.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].uri().path(), "/vouchers/5/send-by-email");
    assert!(out.success);
}

#[test]
fn a_flag_beside_json_body_is_an_edit_not_a_value_that_is_dropped() {
    let dir = tempdir();
    let path = dir.join("voucher.json");
    std::fs::write(
        &path,
        serde_json::json!({"total": "1.00", "currency": "EUR", "status": "draft"}).to_string(),
    )
    .expect("writing the fixture");
    let client = Recorder::new();
    let out = run(
        &client,
        &[
            "toy",
            "raw",
            "vouchers",
            "create",
            "--json-body",
            path.to_str().expect("a UTF-8 path"),
            "--total",
            "12.50",
        ],
    );
    assert!(
        out.stdout
            .contains(r#"{"total":"12.50","currency":"EUR","status":"draft"}"#),
        "{}",
        out.stdout
    );
}

/// A `--json-body` file the document's own schema refuses is refused here,
/// before a request is built, rather than sent for the server to reject, as the
/// typed call is held to the same schema.
#[test]
fn a_json_body_that_does_not_fit_the_document_is_refused_with_nothing_sent() {
    let dir = tempdir();
    let path = dir.join("contact.json");
    std::fs::write(&path, serde_json::json!({"name": "Ada"}).to_string())
        .expect("writing the fixture");
    let client = Recorder::new();
    let api = api();
    let matches = app::root(&api).get_matches_from([
        "toy",
        "raw",
        "contacts",
        "create",
        "--json-body",
        path.to_str().expect("a UTF-8 path"),
        "--commit",
    ]);
    let error = app::run(api, &client, &matches).expect_err("`address` is required");
    let message = format!("{error}");
    assert_eq!(
        message,
        "createContact: the request body does not fit the schema the document declares at `.`"
    );
    assert_eq!(
        std::error::Error::source(&error).map(ToString::to_string),
        Some("missing field `address`".to_owned())
    );
    assert!(client.take().is_empty(), "nothing reached the transport");
}

/// The bodies the requirements name for an undeclared property, byte for byte.
const CONTACT: &str = r#"{ "name": "Acme Ltd", "address": { "street": "1 Example Way", "city": "Vienna" }, "undeclaredField": "this property is in no schema" }"#;
const NESTED: &str = r#"{ "name": "Acme Ltd", "address": { "street": "1 Example Way", "city": "Vienna", "extra": "x" } }"#;
const GOOD: &str =
    r#"{ "name": "Acme Ltd", "address": { "street": "1 Example Way", "city": "Vienna" } }"#;
const LABELS: &str = r#"{ "vip": "yes", "region": "east" }"#;

/// `args` with `--json-body` pointing at a file that holds `text`, run to its
/// end. The error is what an operator's terminal shows as `toy: …`, with the
/// cause under it, the way `main` prints it.
fn refused(client: &Recorder, args: &[&str], text: &str) -> (app::Error, String) {
    let dir = tempdir();
    let path = dir.join("body.json");
    std::fs::write(&path, text).expect("writing the fixture");
    let path = path.to_str().expect("a UTF-8 path");
    let api = api();
    let matches =
        app::root(&api).get_matches_from(args.iter().copied().chain(["--json-body", path]));
    let error = app::run(api, client, &matches).expect_err("the body is refused");
    let causes = std::iter::successors(std::error::Error::source(&error), |cause| cause.source())
        .map(|cause| format!("  caused by: {cause}"));
    let shown = std::iter::once(format!("toy: {error}"))
        .chain(causes)
        .collect::<Vec<_>>()
        .join("\n");
    (error, shown)
}

/// AC-7.1: a property no schema declares stops the request in a dry run.
#[test]
fn a_property_no_schema_declares_stops_the_dry_run() {
    let client = Recorder::new();

    let (error, stderr) = refused(&client, &["toy", "raw", "contacts", "create"], CONTACT);

    assert!(
        matches!(
            &error,
            app::Error::Body(api::BodyError::Undeclared { op, path })
                if op == "createContact" && path == "undeclaredField"
        ),
        "{error:?}"
    );
    assert!(stderr.contains("createContact"), "{stderr}");
    assert!(stderr.contains("undeclaredField"), "{stderr}");
    assert!(
        !stderr.contains("this property is in no schema"),
        "the refusal never prints the value: {stderr}"
    );
    assert!(client.take().is_empty(), "nothing reached the transport");
}

/// AC-7.2: the same refusal in a confirmed run, with nothing on the wire.
#[test]
fn a_property_no_schema_declares_stops_a_confirmed_run_too() {
    let client = Recorder::new();

    let (error, stderr) = refused(
        &client,
        &["toy", "raw", "contacts", "create", "--commit"],
        CONTACT,
    );

    assert!(
        matches!(&error, app::Error::Body(api::BodyError::Undeclared { .. })),
        "{error:?}"
    );
    assert!(stderr.contains("undeclaredField"), "{stderr}");
    assert!(
        client.take().is_empty(),
        "a request that the check refuses is never handed to the transport"
    );
}

/// AC-7.3: a property one level down is named by its path.
#[test]
fn a_nested_property_no_schema_declares_is_named_by_its_path() {
    let client = Recorder::new();

    let (error, stderr) = refused(&client, &["toy", "raw", "contacts", "create"], NESTED);

    assert!(
        matches!(
            &error,
            app::Error::Body(api::BodyError::Undeclared { path, .. }) if path == "address.extra"
        ),
        "{error:?}"
    );
    assert!(stderr.contains("`address.extra`"), "{stderr}");
    assert!(client.take().is_empty());
}

/// AC-7.4: a body that declares everything it carries is sent as it is.
#[test]
fn a_body_with_only_declared_properties_is_printed_as_given() {
    let dir = tempdir();
    let path = dir.join("good.json");
    std::fs::write(&path, GOOD).expect("writing the fixture");
    let client = Recorder::new();

    let out = run(
        &client,
        &[
            "toy",
            "raw",
            "contacts",
            "create",
            "--json-body",
            path.to_str().expect("a UTF-8 path"),
        ],
    );

    assert!(out.success, "{}", out.stderr);
    assert!(out.stdout.contains("POST /contacts"), "{}", out.stdout);
    assert!(
        out.stdout.contains(
            r#"{"name":"Acme Ltd","address":{"street":"1 Example Way","city":"Vienna"}}"#
        ),
        "{}",
        out.stdout
    );
}

/// AC-7.5: `additionalProperties` admits the labels, which no struct declares.
#[test]
fn a_property_an_open_schema_admits_is_sent_as_before() {
    let dir = tempdir();
    let path = dir.join("labels.json");
    std::fs::write(&path, LABELS).expect("writing the fixture");
    let path = path.to_str().expect("a UTF-8 path");
    let line = [
        "toy",
        "raw",
        "contacts",
        "labels",
        "--id",
        "1",
        "--json-body",
        path,
    ];

    let dry = run(&Recorder::new(), &line);
    assert!(dry.success, "{}", dry.stderr);
    assert!(
        dry.stdout.contains("PUT /contacts/1/labels"),
        "{}",
        dry.stdout
    );
    assert!(dry.stdout.contains("vip"), "{}", dry.stdout);
    assert!(dry.stdout.contains("region"), "{}", dry.stdout);

    let client = Recorder::new().answering(
        StatusCode::OK,
        &serde_json::json!({"vip": "yes", "region": "east"}),
    );
    let committed: Vec<&str> = line.iter().copied().chain(["--commit"]).collect();
    let wet = run(&client, &committed);
    assert!(wet.success, "{}", wet.stderr);
    assert_eq!(client.take().len(), 1, "an open body goes out once");
}

/// The open schema takes strings, and only strings: it is a map of them.
#[test]
fn an_open_schema_still_holds_its_values_to_their_type() {
    let client = Recorder::new();

    let (error, stderr) = refused(
        &client,
        &["toy", "raw", "contacts", "labels", "--id", "1"],
        r#"{ "vip": 1 }"#,
    );

    assert!(
        matches!(&error, app::Error::Body(api::BodyError::Mismatch { .. })),
        "{error:?}"
    );
    assert!(stderr.contains("setContactLabels"), "{stderr}");
    assert!(client.take().is_empty());
}

/// F-29, from the input of its probe: five values in the order the generated
/// `Voucher` declares its fields. Serde reads a struct from a list by place, so
/// the list is refused for what it is.
#[test]
fn a_list_in_the_place_of_an_object_stops_the_request() {
    let client = Recorder::new();

    let (error, stderr) = refused(
        &client,
        &["toy", "raw", "vouchers", "create", "--commit"],
        r#"["EUR",null,null,"draft","1.00"]"#,
    );

    assert!(
        matches!(
            &error,
            app::Error::Body(api::BodyError::NotAnObject { op, path, found })
                if op == "createVoucher" && path == "." && *found == "an array"
        ),
        "{error:?}"
    );
    assert!(stderr.contains("createVoucher"), "{stderr}");
    assert!(client.take().is_empty());
}

/// F-29 below the top: `address` is a list that holds the two values the
/// generated `Address` declares, which serde reads by place. The server would
/// read a list.
#[test]
fn a_list_in_the_place_of_a_nested_object_stops_the_request() {
    let client = Recorder::new();

    let (error, stderr) = refused(
        &client,
        &["toy", "raw", "contacts", "create", "--commit"],
        r#"{"name": "Acme Ltd", "address": ["Vienna", "1 Example Way"]}"#,
    );

    assert!(
        matches!(
            &error,
            app::Error::Body(api::BodyError::NotAnObject { op, path, found })
                if op == "createContact" && path == "address" && *found == "an array"
        ),
        "{error:?}"
    );
    assert!(stderr.contains("`address`"), "{stderr}");
    assert!(client.take().is_empty());
}

/// F-30, from the input of its probe: field flags beside a list.
#[test]
fn a_field_flag_beside_a_list_is_refused_and_not_dropped() {
    let client = Recorder::new();

    let (error, stderr) = refused(
        &client,
        &[
            "toy",
            "raw",
            "vouchers",
            "create",
            "--currency",
            "USD",
            "--total",
            "99.99",
            "--status",
            "paid",
        ],
        r#"["EUR",null,null,"draft","1.00"]"#,
    );

    assert!(
        matches!(
            &error,
            app::Error::Dispatch(DispatchError::Arg(ArgError::FlagBesideNonObject { .. }))
        ),
        "{error:?}"
    );
    assert!(stderr.contains("`--total`"), "{stderr}");
    assert!(client.take().is_empty());
}

#[test]
fn a_nested_body_goes_through_a_file_and_no_dead_flags_are_offered() {
    let dir = tempdir();
    let path = dir.join("contact.json");
    let contact = serde_json::json!({
        "name": "Ada", "address": {"street": "1 Main", "city": "Vienna"}
    });
    std::fs::write(&path, contact.to_string()).expect("writing the fixture");
    let client = Recorder::new().answering(StatusCode::CREATED, &contact);
    let out = run(
        &client,
        &[
            "toy",
            "raw",
            "contacts",
            "create",
            "--json-body",
            path.to_str().expect("a UTF-8 path"),
            "--commit",
        ],
    );
    assert_eq!(client.take().len(), 1);
    assert!(out.success);

    // There is no `--name`, because there is no per-field flag set for a body
    // this CLI will not take apart.
    let api = api();
    let refused =
        app::root(&api).try_get_matches_from(["toy", "raw", "contacts", "create", "--name", "Ada"]);
    assert!(refused.is_err(), "a flag the request builder would ignore");
}

/// The other half of a body that goes through a file: what goes *in* the file.
///
/// A body with no per-field flags has nothing on `--help` saying what it wants,
/// and this is the flag that says it. The skeleton goes to stdout on its own,
/// so the redirect a user reaches for leaves a file holding JSON and nothing
/// else — and that file, filled in, is what `--json-body` takes. The round trip
/// is the test: a template the generated type would refuse is a template that
/// sent a user somewhere they cannot get back from.
///
/// `createContact` is a `POST` whose body the document requires, so a request
/// built for this command line would have been refused for having no body.
/// That it answers at all is the evidence nothing was built.
#[test]
fn the_template_is_the_file_json_body_wants() {
    let asked = Recorder::new();
    let out = run(
        &asked,
        &["toy", "raw", "contacts", "create", "--json-body-template"],
    );

    assert!(
        asked.take().is_empty(),
        "nothing is sent to find out what a body looks like"
    );
    assert!(out.success);
    assert_eq!(
        out.stdout,
        "{\n  \"name\": \"\",\n  \"address\": {\n    \"street\": \"\",\n    \"city\": \"Vienna\"\n  }\n}\n",
        "the required keys, nested, with the document's own example where it states one"
    );

    // Filled in and handed back. `toy raw` holds a JSON body to the generated
    // `Contact` before anything goes out, so a request that leaves here is a
    // template that fits the document it was built from.
    let dir = tempdir();
    let path = dir.join("contact.json");
    let filled = out
        .stdout
        .replace("\"name\": \"\"", "\"name\": \"Ada\"")
        .replace("\"street\": \"\"", "\"street\": \"1 Main\"");
    std::fs::write(&path, &filled).expect("writing the fixture");

    let client = Recorder::new().answering(
        StatusCode::CREATED,
        &serde_json::json!({"id": 1, "name": "Ada",
                            "address": {"street": "1 Main", "city": "Vienna"}}),
    );
    let sent = run(
        &client,
        &[
            "toy",
            "raw",
            "contacts",
            "create",
            "--json-body",
            path.to_str().expect("a UTF-8 path"),
            "--commit",
        ],
    );
    assert!(sent.success, "{}", sent.stderr);
    assert_eq!(client.take().len(), 1);
}

#[test]
fn the_multipart_upload_is_assembled_from_parts() {
    let dir = tempdir();
    let path = dir.join("doc.bin");
    std::fs::write(&path, b"PDF").expect("writing the fixture");
    let client = Recorder::new().answering(StatusCode::CREATED, &serde_json::json!(null));
    run(
        &client,
        &[
            "toy",
            "raw",
            "documents-multipart",
            "create",
            "--file",
            &format!("file={}", path.to_str().expect("a UTF-8 path")),
            "--field",
            "kind=invoice",
            "--commit",
        ],
    );
    let sent = client.take();
    let body = String::from_utf8_lossy(sent[0].body()).into_owned();
    assert!(body.contains("name=\"kind\""), "{body}");
    assert!(body.contains("filename=\"doc.bin\""), "{body}");
    assert!(body.contains("PDF"), "{body}");
}

#[test]
fn the_chain_prints_every_request_it_would_make() {
    let client = Recorder::new().answering(StatusCode::OK, &voucher("open"));
    let out = run(&client, FINALIZE);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|line| line.contains("HTTP/1.1"))
        .collect();
    assert_eq!(
        lines,
        [
            "GET /vouchers/5 HTTP/1.1",
            "POST /vouchers/5/enshrine HTTP/1.1",
            "GET /vouchers/5/render HTTP/1.1",
        ]
    );
    // Only the read ran.
    assert_eq!(client.take().len(), 1);
    assert!(out.stderr.contains("the writes did not"), "{}", out.stderr);
}

/// The hand-written verb is held to the same word the generated subcommand is,
/// and asks for it at the parser: the chain may enshrine, so `--enshrine` is
/// typed before the fetch that decides whether it will.
#[test]
fn the_chain_cannot_be_asked_for_without_naming_its_irreversible_step() {
    let api = api();
    let refused = app::root(&api)
        .try_get_matches_from(["toy", "finalize-voucher", "--id", "5"])
        .expect_err("the chain stands behind --enshrine");

    assert_eq!(
        refused.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    assert!(refused.to_string().contains("--enshrine"), "{refused}");
}

/// And the flags it asks for are the document's words rather than this crate's.
/// A verb that spelled them itself would go on offering `--enshrine` after an
/// Overlay renamed the gate, and the chain would quietly become unanswerable.
#[test]
fn the_chain_offers_every_gate_the_operation_it_calls_names() {
    let api = api();
    let root = app::root(&api);
    let verb = root
        .find_subcommand("finalize-voucher")
        .expect("the verb is mounted");

    for gate in api.operation(OperationId::EnshrineVoucher).gates() {
        assert!(
            verb.get_arguments()
                .any(|arg| arg.get_long() == Some(gate.as_str())),
            "the chain calls an operation gated on `{gate}` and offers no flag for it"
        );
    }
}

#[test]
fn the_chain_skips_the_irreversible_step_for_a_voucher_that_is_not_open() {
    let client = Recorder::new().answering(StatusCode::OK, &voucher("paid"));
    let out = run(&client, FINALIZE);
    assert!(
        !out.stdout.contains("/vouchers/5/enshrine"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("/vouchers/5/render"), "{}", out.stdout);
}

/// `--base-url` is global, and every verb that sends honours it — not only
/// `raw`. The chain is the case worth asserting: it builds its requests from
/// the typed handle rather than from `ArgMatches`, so a flag that reached only
/// the `raw` path would leave the chain talking to the document's own server
/// while the user watched a dry run that named theirs.
#[test]
fn the_chain_sends_to_the_base_url_it_was_given() {
    const BASE: &str = "http://ledger.example:8080";

    // The dry run: the fetch the chain depends on goes there, and all three
    // requests are printed against it.
    let dry = Recorder::new().answering(StatusCode::OK, &voucher("open"));
    let printed = run(
        &dry,
        &[
            "toy",
            "--base-url",
            BASE,
            "finalize-voucher",
            "--id",
            "5",
            "--enshrine",
        ],
    );
    let hosts: Vec<&str> = printed
        .stdout
        .lines()
        .filter(|line| line.starts_with("host: "))
        .collect();
    assert_eq!(
        hosts, ["host: ledger.example:8080"; 3],
        "{}",
        printed.stdout
    );
    assert_eq!(
        dry.take()[0].uri().to_string(),
        format!("{BASE}/vouchers/5")
    );

    // And `--commit` sends every one of them there.
    let wet = Recorder::new()
        .answering(StatusCode::OK, &voucher("open"))
        .answering(StatusCode::OK, &voucher("paid"))
        .answering(StatusCode::OK, &voucher("paid"));
    run(
        &wet,
        &[
            "toy",
            "--base-url",
            BASE,
            "finalize-voucher",
            "--id",
            "5",
            "--enshrine",
            "--commit",
        ],
    );
    let sent: Vec<String> = wet.take().iter().map(|r| r.uri().to_string()).collect();
    assert_eq!(
        sent,
        [
            format!("{BASE}/vouchers/5"),
            format!("{BASE}/vouchers/5/enshrine"),
            format!("{BASE}/vouchers/5/render"),
        ]
    );
}

/// The chain decides on one request for each write and sends that one: the
/// requests the dry run printed are, parsed back into method, target, headers and
/// body, the requests `--commit` handed the transport. A chain that built a
/// request to decide on and another to send would be equal here only while
/// building is pure.
#[test]
fn the_chain_sends_the_requests_it_printed() {
    let script = || {
        Recorder::new()
            .answering(StatusCode::OK, &voucher("open"))
            .answering(StatusCode::OK, &voucher("paid"))
            .answering(StatusCode::OK, &voucher("paid"))
    };
    let dry = run(&script(), FINALIZE);
    let wet = script();
    let committed: Vec<&str> = FINALIZE.iter().copied().chain(["--commit"]).collect();
    run(&wet, &committed);

    let printed: Vec<Printed> = dry.stdout.split("\n\n").map(Printed::parse).collect();
    let sent: Vec<Printed> = wet.take().iter().map(Printed::of).collect();

    assert_eq!(printed.len(), 3, "the fetch and the two writes");
    assert_eq!(printed, sent);
    assert_eq!(
        printed
            .iter()
            .map(|request| format!("{} {}", request.method, request.target))
            .collect::<Vec<_>>(),
        [
            "GET /vouchers/5",
            "POST /vouchers/5/enshrine",
            "GET /vouchers/5/render",
        ]
    );
}

#[test]
fn commit_runs_the_whole_chain_in_order() {
    let client = Recorder::new()
        .answering(StatusCode::OK, &voucher("open"))
        .answering(StatusCode::OK, &voucher("paid"))
        .answering(StatusCode::OK, &voucher("paid"));
    let committed: Vec<&str> = FINALIZE.iter().copied().chain(["--commit"]).collect();
    let out = run(&client, &committed);
    let sent: Vec<String> = client
        .take()
        .iter()
        .map(|r| format!("{} {}", r.method(), r.uri().path()))
        .collect();
    assert_eq!(
        sent,
        [
            "GET /vouchers/5",
            "POST /vouchers/5/enshrine",
            "GET /vouchers/5/render",
        ]
    );
    assert!(
        out.stdout.contains("\"status\": \"paid\""),
        "{}",
        out.stdout
    );
}

#[test]
fn a_static_completion_script_covers_the_whole_tree() {
    let script = run(&Recorder::new(), &["toy", "completions", "bash"]).stdout;
    for name in ["raw", "finalize-voucher", "vouchers", "update", "archive"] {
        assert!(script.contains(name), "`{name}` is missing from the script");
    }
}

#[test]
fn the_document_refuses_bad_values_before_a_request_is_built() {
    let api = api();
    for args in [
        // Not an amount; not a `VoucherStatus`; not an integer; and a PUT
        // missing two fields the document marks required.
        vec![
            "toy", "raw", "vouchers", "update", "--id", "5", "--total", "12.5x",
        ],
        vec![
            "toy", "raw", "vouchers", "update", "--id", "5", "--status", "bogus",
        ],
        vec![
            "toy", "raw", "vouchers", "update", "--id", "x", "--total", "1.00",
        ],
        vec![
            "toy", "raw", "vouchers", "update", "--id", "5", "--total", "12.50",
        ],
        // A VAT rate that the `enum` of `0`, `7` and `19` does not list.
        vec![
            "toy",
            "raw",
            "ledger",
            "entries",
            "--account",
            "4400",
            "--amount",
            "12.50",
            "--vat-rate",
            "5",
        ],
    ] {
        assert!(
            app::root(&api).try_get_matches_from(&args).is_err(),
            "{args:?} should not parse"
        );
    }
}

/// The rates an operator may type are on the help page, because the same code
/// that refuses `5` writes that line.
#[test]
fn the_help_of_a_numeric_enum_lists_its_values() {
    let api = api();
    let page = app::root(&api)
        .try_get_matches_from(["toy", "raw", "ledger", "entries", "--help"])
        .expect_err("--help stops the parse")
        .to_string();
    assert!(
        page.contains("VAT rate in percent (one of 0, 7, 19)"),
        "{page}"
    );
    assert!(
        app::root(&api)
            .try_get_matches_from([
                "toy",
                "raw",
                "ledger",
                "entries",
                "--account",
                "4400",
                "--amount",
                "12.50",
                "--vat-rate",
                "19",
            ])
            .is_ok()
    );
}

/// A directory of this test binary's own, without taking a dependency for it.
fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "toy-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("creating the scratch directory");
    dir
}
