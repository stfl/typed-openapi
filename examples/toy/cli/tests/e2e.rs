//! The CLI, end to end, against a client that records what it was given and
//! answers from a script. No socket, no fixture server, no runtime.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use api::Api;
use cli::app;
use cli::output::Output;
use http::StatusCode;
use typed_openapi::Recorder;

/// The write the report quotes. Every field the document marks required is
/// required on the command line too, so an incomplete PUT is refused here
/// rather than accepted by the server.
const UPDATE: &[&str] = &[
    "toy",
    "raw",
    "update-voucher",
    "--id",
    "5",
    "--total",
    "12.50",
    "--currency",
    "EUR",
    "--status",
    "draft",
];

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
        &["toy", "raw", "list-vouchers", "--status", "paid"],
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
    let out = run(&client, &["toy", "raw", "render-voucher", "--id", "5"]);
    assert!(
        out.stdout.starts_with("GET /vouchers/5/render"),
        "{}",
        out.stdout
    );
    assert!(client.take().is_empty());
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
            "create-voucher",
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

/// The hole the CLI path used to have beside the typed one: a `--json-body`
/// file the document's own schema refuses is refused here, before a request is
/// built, rather than sent for the server to reject.
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
        "create-contact",
        "--json-body",
        path.to_str().expect("a UTF-8 path"),
        "--commit",
    ]);
    let error = app::run(api, &client, &matches).expect_err("`address` is required");
    let message = format!("{error}");
    assert_eq!(
        message,
        "createContact: the request body does not fit the schema the document declares"
    );
    assert_eq!(
        std::error::Error::source(&error).map(ToString::to_string),
        Some("missing field `address`".to_owned())
    );
    assert!(client.take().is_empty(), "nothing reached the transport");
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
            "create-contact",
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
        app::root(&api).try_get_matches_from(["toy", "raw", "create-contact", "--name", "Ada"]);
    assert!(refused.is_err(), "a flag the request builder would ignore");
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
            "upload-document-multipart",
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
    let out = run(&client, &["toy", "finalize-voucher", "--id", "5"]);
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

#[test]
fn the_chain_skips_the_irreversible_step_for_a_voucher_that_is_not_open() {
    let client = Recorder::new().answering(StatusCode::OK, &voucher("paid"));
    let out = run(&client, &["toy", "finalize-voucher", "--id", "5"]);
    assert!(!out.stdout.contains("enshrine"), "{}", out.stdout);
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
        &["toy", "--base-url", BASE, "finalize-voucher", "--id", "5"],
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

#[test]
fn commit_runs_the_whole_chain_in_order() {
    let client = Recorder::new()
        .answering(StatusCode::OK, &voucher("open"))
        .answering(StatusCode::OK, &voucher("paid"))
        .answering(StatusCode::OK, &voucher("paid"));
    let out = run(
        &client,
        &["toy", "finalize-voucher", "--id", "5", "--commit"],
    );
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
    for name in [
        "raw",
        "finalize-voucher",
        "update-voucher",
        "archive-voucher",
    ] {
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
            "toy",
            "raw",
            "update-voucher",
            "--id",
            "5",
            "--total",
            "12.5x",
        ],
        vec![
            "toy",
            "raw",
            "update-voucher",
            "--id",
            "5",
            "--status",
            "bogus",
        ],
        vec![
            "toy",
            "raw",
            "update-voucher",
            "--id",
            "x",
            "--total",
            "1.00",
        ],
        vec![
            "toy",
            "raw",
            "update-voucher",
            "--id",
            "5",
            "--total",
            "12.50",
        ],
    ] {
        assert!(
            app::root(&api).try_get_matches_from(&args).is_err(),
            "{args:?} should not parse"
        );
    }
}

/// A directory of this test binary's own, without taking a dependency for it.
fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "toy-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).expect("creating the scratch directory");
    dir
}
