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
const CORRECTIONS: &str = include_str!("fixtures/corrections.yaml");
const CLI: &str = include_str!("fixtures/cli.yaml");

/// The layers, in the order a bless step applies them.
const OVERLAYS: &[&str] = &[CORRECTIONS, CLI];

const CREATE: &[&str] = &[
    "toy",
    "vouchers",
    "create",
    "--total",
    "12.50",
    "--currency",
    "EUR",
    "--status",
    "open",
];

fn document() -> Document {
    Document::load(TOY, OVERLAYS).expect("the vendor's document plus the adopter's Overlay")
}

/// The groups as the whole CLI — the shape the README opens with.
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
    let matches = parse(&doc, &["toy", "vouchers", "get", "--id", "5"]);

    let outcome = tree::dispatch(&doc, doc.base(), &client, &matches).expect("a read dispatches");

    assert!(matches!(outcome, Outcome::Sent(_)), "a read is sent");
    let sent = only(&client);
    assert_eq!(sent.uri().path(), "/vouchers/5");
    assert_eq!(sent.method(), "GET");
}

/// A `dispatch`-only CLI has no seam to vet a body in, so what the document
/// says about a value is the only thing between a typo and the wire. It is
/// enough: the flag's value parser is the document's own rule, and the rule
/// lives in a schema `total` points at rather than in any Rust here.
#[test]
fn a_value_the_documents_rule_refuses_never_becomes_a_request() {
    let doc = document();
    let refused = root(&doc)
        .try_get_matches_from([
            "toy",
            "vouchers",
            "create",
            "--total",
            "1,50",
            "--currency",
            "EUR",
            "--status",
            "open",
        ])
        .expect_err("a comma is not a decimal point");
    let refused = refused.to_string();
    assert!(
        refused.contains(r"invalid value '1,50' for '--total <STRING>'"),
        "{refused}"
    );
    assert!(
        refused.contains(r"`1,50` does not match ^-?[0-9]+(\.[0-9]{1,2})?$"),
        "{refused}"
    );

    // And the same flag takes an amount, so the rule is a rule rather than a
    // refusal of everything.
    let client = Recorder::new();
    let outcome = tree::dispatch(&doc, doc.base(), &client, &parse(&doc, CREATE))
        .expect("a valid amount dispatches");
    let Outcome::DryRun(request) = outcome else {
        panic!("a create is a write, so it is a dry run without --commit");
    };
    assert!(
        render(&request).contains(r#""total":"12.50""#),
        "{}",
        render(&request)
    );
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
    let matches = parse(&doc, &["toy", "vouchers", "render", "--id", "5"]);

    let outcome = tree::dispatch(&doc, doc.base(), &client, &matches).expect("dispatches");

    assert!(matches!(outcome, Outcome::DryRun(_)));
    assert!(client.take().is_empty());
}

#[test]
fn the_shortcut_and_the_seam_reach_the_same_request() {
    let doc = document();

    let long_way = {
        let client = Recorder::new();
        let matches = parse(&doc, &["toy", "vouchers", "get", "--id", "5"]);
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
        let matches = parse(&doc, &["toy", "vouchers", "get", "--id", "5"]);
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
        .get_matches_from(["app", "passthrough", "vouchers", "get", "--id", "5"]);
    let mounted = matches
        .subcommand_matches("passthrough")
        .expect("the subcommand parsed");

    tree::dispatch(&doc, doc.base(), &client, mounted).expect("dispatches under any name");

    assert_eq!(only(&client).uri().path(), "/vouchers/5");
}

#[test]
fn a_name_the_document_does_not_describe_is_refused_by_both_halves() {
    let doc = document();
    // Built by hand rather than parsed, because clap would reject the names
    // before `select` ever saw them. This is the path a caller reaches by
    // mounting a subcommand of their own beside the generated ones.
    let matches = Command::new("toy")
        .subcommand(Command::new("vouchers").subcommand(Command::new("not-an-operation")))
        .get_matches_from(["toy", "vouchers", "not-an-operation"]);

    let error = tree::select(&doc, &matches).expect_err("no such operation");

    assert!(matches!(&error, DispatchError::Unknown { group, command }
            if group == "vouchers" && command == "not-an-operation"));
    assert_eq!(
        error.to_string(),
        "no operation named `vouchers not-an-operation` in the document"
    );
}

/// The tree the document builds: one subcommand per resource it groups its
/// paths under, and one under that per operation. The names are the document's
/// paths and methods, not its `operationId`s — `PUT /vouchers/{id}` is
/// `vouchers update` however the vendor spelled `updateVoucher`.
#[test]
fn operations_are_mounted_under_the_resource_their_path_names() {
    let doc = document();
    let tree: Vec<(String, Vec<String>)> = tree::commands(&doc)
        .iter()
        .map(|group| {
            (
                group.get_name().to_owned(),
                group
                    .get_subcommands()
                    .map(|op| op.get_name().to_owned())
                    .collect(),
            )
        })
        .collect();

    assert_eq!(
        tree,
        [
            (
                "vouchers".to_owned(),
                vec![
                    "list".to_owned(),
                    "create".to_owned(),
                    "get".to_owned(),
                    "update".to_owned(),
                    "enshrine".to_owned(),
                    "render".to_owned(),
                    "archive".to_owned(),
                ]
            ),
            ("contacts".to_owned(), vec!["create".to_owned()]),
            ("documents".to_owned(), vec!["create".to_owned()]),
            ("documents-multipart".to_owned(), vec!["create".to_owned()]),
        ]
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

/// A group with nothing named under it is the same answer. The tree `commands`
/// builds requires the second name, so a user meets clap's own help instead;
/// this is the path a caller reaches by mounting a group of their own.
#[test]
fn a_group_with_no_operation_under_it_is_the_same_error() {
    let doc = document();
    let matches = Command::new("toy")
        .subcommand(Command::new("vouchers"))
        .get_matches_from(["toy", "vouchers"]);

    assert!(matches!(
        tree::select(&doc, &matches).expect_err("no operation was named"),
        DispatchError::NoCommand
    ));
}

/// The one argument of `command` with this long flag.
fn flag<'c>(command: &'c Command, long: &str) -> &'c clap::Arg {
    command
        .get_arguments()
        .find(|arg| arg.get_long() == Some(long))
        .unwrap_or_else(|| panic!("no --{long} on `{}`", command.get_name()))
}

fn command_for(doc: &Document, id: &str) -> Command {
    tree::command(doc.get(id).unwrap_or_else(|| panic!("{id} is documented")))
}

#[test]
fn an_enum_in_the_document_reaches_the_command_line_as_choices() {
    let doc = document();
    let choices: Vec<String> = flag(&command_for(&doc, "createVoucher"), "status")
        .get_possible_values()
        .iter()
        .map(|value| value.get_name().to_owned())
        .collect();

    // This is what a shell offers for `--status <TAB>`, and what the parser
    // refuses anything else against: one list, from the document.
    assert_eq!(choices, ["draft", "open", "paid"]);
}

#[test]
fn only_an_operation_that_writes_carries_the_commit_flag() {
    let doc = document();
    let has_commit = |id: &str| {
        command_for(&doc, id)
            .get_arguments()
            .any(|arg| arg.get_long() == Some("commit"))
    };

    assert!(!has_commit("listVouchers"), "a read runs on sight");
    assert!(!has_commit("getVoucher"), "a read runs on sight");
    assert!(has_commit("createVoucher"), "a POST is gated");
    // The gate does not come from the method: this one is a GET the document
    // marks `x-cli-writes`, and the flag has to be there for a user to pass.
    assert!(
        has_commit("renderVoucher"),
        "a documented writing GET is gated"
    );
}
