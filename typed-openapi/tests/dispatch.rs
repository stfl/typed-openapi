//! The trip from a command line to a sent request, and the gate in the middle.
//!
//! Everything here drives the real `tree` functions against a [`Recorder`], so
//! what the tests assert on is the request that would have gone out — or, for a
//! write nobody confirmed, the fact that nothing did.
//!
//! The fixtures under `tests/fixtures/` are this crate's own, not the example
//! adoption's. `examples/toy/spec/` holds a document with the same content
//! today and a different owner: there it is the vendor's, and the example is
//! free to evolve it. Pointing these tests at that copy would let a change to
//! the example break the library.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use clap::{ArgMatches, Command};
use typed_openapi::tree::{self, DispatchError, Outcome};
use typed_openapi::{Document, HttpRequest, Recorder, render};

const TOY: &str = include_str!("fixtures/toy.yaml");
const OVERLAY: &str = include_str!("fixtures/overlay.yaml");

const CREATE: &[&str] = &[
    "toy",
    "create-voucher",
    "--total",
    "12.50",
    "--currency",
    "EUR",
    "--status",
    "open",
];

fn document() -> Document {
    Document::load(TOY, OVERLAY).expect("the vendor's document plus the adopter's Overlay")
}

/// The operations as the whole CLI — the shape the README opens with.
fn root(doc: &Document) -> Command {
    Command::new("toy")
        .subcommand_required(true)
        .subcommands(tree::commands(doc))
}

fn parse(doc: &Document, args: &[&str]) -> ArgMatches {
    root(doc).get_matches_from(args)
}

/// The one request the recorder was given. "One" is half the assertion: a
/// command that sent twice, or not at all, fails here rather than in whatever
/// the caller went on to check.
fn only(client: &Recorder) -> HttpRequest {
    let mut sent = client.take();
    assert_eq!(sent.len(), 1, "exactly one request went out");
    sent.pop().expect("the length is one")
}

#[test]
fn operations_mounted_as_the_cli_itself_are_a_valid_clap_command() {
    // `debug_assert` is where clap reports a tree it would panic on at run
    // time — a duplicate argument id, two subcommands with one name. The
    // `raw`-mounted shape is checked in `document.rs`; this is the other one
    // the crate promises, and a name that only collides at the root would show
    // up here and nowhere else.
    root(&document()).debug_assert();
}

#[test]
fn a_read_is_sent_on_sight() {
    let doc = document();
    let client = Recorder::new();
    let matches = parse(&doc, &["toy", "get-voucher", "--id", "5"]);

    let outcome = tree::dispatch(&doc, doc.base(), &client, &matches).expect("a read dispatches");

    assert!(matches!(outcome, Outcome::Sent(_)), "a read is sent");
    let sent = only(&client);
    assert_eq!(sent.uri().path(), "/vouchers/5");
    assert_eq!(sent.method(), "GET");
}

#[test]
fn a_write_nobody_confirmed_sends_nothing() {
    let doc = document();
    let client = Recorder::new();
    let matches = parse(&doc, CREATE);

    let outcome = tree::dispatch(&doc, doc.base(), &client, &matches).expect("a write dispatches");

    let Outcome::DryRun(request) = outcome else {
        panic!("a write without --commit is a dry run");
    };
    assert!(
        client.take().is_empty(),
        "a dry run reaches the client with nothing"
    );
    assert_eq!(
        render(&request),
        "POST /vouchers HTTP/1.1\n\
         host: localhost:9999\n\
         content-type: application/json\n\
         \n\
         {\"total\":\"12.50\",\"currency\":\"EUR\",\"status\":\"open\"}\n"
    );
}

#[test]
fn commit_sends_exactly_what_the_dry_run_printed() {
    let doc = document();
    let dry = {
        let client = Recorder::new();
        let matches = parse(&doc, CREATE);
        match tree::dispatch(&doc, doc.base(), &client, &matches).expect("dispatches") {
            Outcome::DryRun(request) => render(&request),
            Outcome::Sent(_) => panic!("no --commit was given"),
        }
    };

    let client = Recorder::new();
    let confirmed: Vec<&str> = CREATE.iter().copied().chain(["--commit"]).collect();
    let matches = parse(&doc, &confirmed);
    let outcome = tree::dispatch(&doc, doc.base(), &client, &matches).expect("dispatches");

    assert!(matches!(outcome, Outcome::Sent(_)), "--commit sends");
    // The promise the gate makes: a dry run is not a description of the
    // request, it is the request.
    assert_eq!(render(&only(&client)), dry);
}

#[test]
fn a_gated_get_is_held_back_like_any_write() {
    let doc = document();
    let client = Recorder::new();
    // A GET the document marks `x-cli-writes`. HTTP cannot say this operation
    // writes, so the method alone would send it.
    let matches = parse(&doc, &["toy", "render-voucher", "--id", "5"]);

    let outcome = tree::dispatch(&doc, doc.base(), &client, &matches).expect("dispatches");

    assert!(matches!(outcome, Outcome::DryRun(_)));
    assert!(client.take().is_empty());
}

#[test]
fn the_shortcut_and_the_seam_reach_the_same_request() {
    let doc = document();

    let long_way = {
        let client = Recorder::new();
        let matches = parse(&doc, &["toy", "get-voucher", "--id", "5"]);
        let selected = tree::select(&doc, &matches).expect("the subcommand names an operation");
        // What the seam exists for: an adopter reads the operation and the
        // values here, and holds the body to a type this crate cannot see.
        assert_eq!(selected.operation().id(), "getVoucher");
        assert!(!selected.confirmed(), "a read carries no --commit flag");
        selected.send(&client, doc.base()).expect("sends");
        only(&client)
    };

    let short_way = {
        let client = Recorder::new();
        let matches = parse(&doc, &["toy", "get-voucher", "--id", "5"]);
        tree::dispatch(&doc, doc.base(), &client, &matches).expect("sends");
        only(&client)
    };

    assert_eq!(render(&long_way), render(&short_way));
}

#[test]
fn the_tree_mounts_under_any_name_and_never_looks_above_itself() {
    let doc = document();
    let client = Recorder::new();
    // Not `raw`, and not the root: `select` is handed the matches of whatever
    // command the operations hang off, and reads only below them.
    let matches = Command::new("app")
        .subcommand(Command::new("passthrough").subcommands(tree::commands(&doc)))
        .get_matches_from(["app", "passthrough", "get-voucher", "--id", "5"]);
    let mounted = matches
        .subcommand_matches("passthrough")
        .expect("the subcommand parsed");

    tree::dispatch(&doc, doc.base(), &client, mounted).expect("dispatches under any name");

    assert_eq!(only(&client).uri().path(), "/vouchers/5");
}

#[test]
fn a_name_the_document_does_not_describe_is_refused_by_name() {
    let doc = document();
    // Built by hand rather than parsed, because clap would reject the name
    // before `select` ever saw it. This is the path a caller reaches by
    // mounting a subcommand of their own beside the generated ones.
    let matches = Command::new("toy")
        .subcommand(Command::new("not-an-operation"))
        .get_matches_from(["toy", "not-an-operation"]);

    let error = tree::select(&doc, &matches).expect_err("no such operation");

    assert!(matches!(&error, DispatchError::Unknown(name) if name == "not-an-operation"));
    assert_eq!(
        error.to_string(),
        "no operation named `not-an-operation` in the document"
    );
}

#[test]
fn no_subcommand_at_all_is_its_own_error() {
    let doc = document();
    let matches = Command::new("toy").get_matches_from(["toy"]);

    let error = tree::select(&doc, &matches).expect_err("nothing was named");

    assert!(matches!(error, DispatchError::NoCommand));
    assert_eq!(error.to_string(), "no command given");
}
