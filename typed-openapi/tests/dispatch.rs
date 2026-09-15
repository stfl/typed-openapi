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

use clap::{Arg, ArgAction, ArgMatches, Command};
use http::{Method, StatusCode};
use serde_json::json;
use typed_openapi::tree::{self, DispatchError, Outcome};
use typed_openapi::{
    Answers, COMMIT, Document, HttpRequest, Plan, Recorder, RecorderError, SyncClient, Values,
    render,
};

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

/// The other half of the seam. `DispatchError::Transport` is where a client's
/// own error arrives, and a script is what puts one there.
#[test]
fn a_client_that_fails_is_reported_as_the_transport_and_the_request_still_went_out() {
    let doc = document();
    // An answer is queued against the request as it goes out, so the route is
    // the path with the id already substituted, not the template the document
    // spells it with.
    let client = Recorder::new().failing_route(Method::GET, "/vouchers/5", "nothing came back");
    let matches = parse(&doc, &["toy", "vouchers", "get", "--id", "5"]);

    let error = tree::dispatch(&doc, doc.base(), &client, &matches)
        .expect_err("the script fails this route");

    assert!(matches!(error, DispatchError::Transport(_)), "{error:?}");
    assert_eq!(error.to_string(), "transport: nothing came back");
    assert_eq!(only(&client).uri().path(), "/vouchers/5");
    assert_eq!(client.unused(), 0, "the script was used up");
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
        assert!(
            !selected.answers().committed(),
            "a read carries no --commit flag"
        );
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
                    "send-by-email".to_owned(),
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

/// A named gate is a flag of its own, and only where the document names one.
#[test]
fn only_an_operation_the_document_names_a_gate_on_carries_that_flag() {
    let doc = document();
    let has_gate = |id: &str, long: &str| {
        command_for(&doc, id)
            .get_arguments()
            .any(|arg| arg.get_long() == Some(long))
    };

    assert!(has_gate("enshrineVoucher", "enshrine"));
    assert!(has_gate("sendVoucherByEmail", "email"));
    assert!(
        !has_gate("createVoucher", "enshrine"),
        "a gate belongs to the operation the document names it on"
    );
    assert!(
        !has_gate("getVoucher", "enshrine"),
        "a read carries no gate flag, exactly as it carries no --commit"
    );
}

/// The gate flag is `required`, so the hazard is on the command line before a
/// request exists — a dry run of it is still a command somebody had to write
/// the word on. The refusal names the flag.
#[test]
fn a_write_whose_gate_is_unnamed_is_refused_before_a_request_is_built() {
    let doc = document();
    let refused = root(&doc)
        .try_get_matches_from(["toy", "vouchers", "enshrine", "--id", "5"])
        .expect_err("enshrineVoucher stands behind --enshrine")
        .to_string();

    assert!(refused.contains("--enshrine"), "{refused}");

    // And with the word typed it parses, whether or not it is also committed:
    // the gate holds the request back, it does not refuse the command line.
    let client = Recorder::new();
    let matches = parse(
        &doc,
        &["toy", "vouchers", "enshrine", "--id", "5", "--enshrine"],
    );
    let outcome = tree::dispatch(&doc, doc.base(), &client, &matches).expect("dispatches");
    assert!(matches!(outcome, Outcome::DryRun(_)), "--commit is missing");
    assert!(client.take().is_empty());
}

#[test]
fn a_gated_write_goes_out_once_both_words_are_given() {
    let doc = document();
    let client = Recorder::new().answering(http::StatusCode::OK, &serde_json::json!({}));
    let matches = parse(
        &doc,
        &[
            "toy",
            "vouchers",
            "enshrine",
            "--id",
            "5",
            "--enshrine",
            "--commit",
        ],
    );

    let outcome = tree::dispatch(&doc, doc.base(), &client, &matches).expect("dispatches");

    assert!(matches!(outcome, Outcome::Sent(_)));
    assert_eq!(only(&client).uri().path(), "/vouchers/5/enshrine");
}

/// One operation, two words. Every gate is answered or nothing is sent — a
/// gate can only hold a request back, never let one through, so answering one
/// of two is a dry run even from a caller who confirmed.
///
/// The fixture has no two-gated operation, and no command line can reach this
/// state either: both flags are `required`, so clap refuses the invocation
/// first. A hand-written verb that builds its own [`Answers`] can, which is
/// what this document is here for.
#[test]
fn a_committed_write_with_one_of_two_gates_answered_sends_nothing() {
    const TWO_GATES: &str = "openapi: 3.0.3\n\
         info: { title: t, version: \"1\" }\n\
         servers: [{ url: 'http://localhost:9999' }]\n\
         paths:\n\
         \x20 /vouchers/{id}/enshrine:\n\
         \x20   post:\n\
         \x20     operationId: enshrineVoucher\n\
         \x20     x-cli-gates: [enshrine, email]\n\
         \x20     parameters:\n\
         \x20       - { name: id, in: path, required: true, schema: { type: integer } }\n\
         \x20     responses: { \"200\": { description: OK } }\n";

    let doc = Document::load(TWO_GATES, &[]).expect("a document with a two-gated operation");
    let op = doc
        .get("enshrineVoucher")
        .expect("the document describes it");
    let decided = |answers: &Answers| {
        Plan::build(op, doc.base(), Values::new().param("id", 5), answers)
            .expect("the values satisfy the operation")
    };

    assert!(matches!(decided(&Answers::new().commit()), Plan::DryRun(_)));
    assert!(matches!(
        decided(&Answers::new().commit().gate("enshrine")),
        Plan::DryRun(_)
    ));
    assert!(
        matches!(
            decided(&Answers::new().gate("enshrine").gate("email")),
            Plan::DryRun(_)
        ),
        "both gates and no confirmation is still a dry run: a gate is answered \
         beside --commit, never instead of it"
    );
    assert!(matches!(
        decided(&Answers::new().commit().gate("enshrine").gate("email")),
        Plan::Send(_)
    ));
}

/// One definition, two callers. A command of the caller's own carries exactly
/// the gate flags the generated subcommand carries — same spelling, same help,
/// same required-ness — because both are `tree::gates`, and a hand-written verb
/// over a gated operation spells nothing itself.
#[test]
fn a_command_of_your_own_carries_the_gate_the_subcommand_carries() {
    let doc = document();
    let op = doc
        .get("enshrineVoucher")
        .expect("the document describes it");

    let mine = tree::gates(Command::new("finalize-voucher"), op);
    let generated = tree::command(op);

    for gate in op.gates() {
        let (mine, generated) = (flag(&mine, gate.as_str()), flag(&generated, gate.as_str()));
        assert_eq!(
            mine.get_help().map(ToString::to_string),
            generated.get_help().map(ToString::to_string)
        );
        assert!(mine.is_required_set() && generated.is_required_set());
    }

    // And nothing else came with them: the gates are all this door adds, so a
    // caller's own flags are theirs to choose.
    let added: Vec<&str> = mine
        .get_arguments()
        .filter_map(|arg| arg.get_long())
        .filter(|long| *long != "help")
        .collect();
    assert_eq!(added, ["enshrine"]);
}

/// An operation with no gate is handed back the command it was given, so a
/// caller adds the flags unconditionally and asks the document nothing.
#[test]
fn an_operation_that_names_no_gate_adds_no_flag() {
    let doc = document();
    let op = doc.get("createVoucher").expect("the document describes it");

    let mine = tree::gates(Command::new("create"), op);

    assert!(
        mine.get_arguments()
            .filter_map(|arg| arg.get_long())
            .all(|long| long == "help")
    );
}

/// A command this crate did not build has no answer for a gate it never
/// declared, and no answer is the closed one: the request is held back, rather
/// than the reading panicking on a flag clap has not heard of.
#[test]
fn a_gate_flag_a_command_never_declared_reads_as_unanswered() {
    let doc = document();
    let op = doc
        .get("enshrineVoucher")
        .expect("the document describes it");
    // A verb of the caller's own that offers `--commit` and no gate flag: the
    // shape `tree::gates` exists to prevent, and the one that must still answer.
    let matches = Command::new("finalize-voucher")
        .arg(Arg::new(COMMIT).long(COMMIT).action(ArgAction::SetTrue))
        .get_matches_from(["finalize-voucher", "--commit"]);

    let answered = tree::answers(op, &matches);

    assert!(answered.committed(), "the flag it does offer is read");
    let gate = op.gates().first().expect("enshrineVoucher names one gate");
    assert!(
        !answered.answered(gate),
        "and the one it does not is closed"
    );
    assert!(
        matches!(
            Plan::build(op, doc.base(), Values::new().param("id", 5), &answered)
                .expect("the values satisfy the operation"),
            Plan::DryRun(_)
        ),
        "an unanswered gate holds the request back, confirmation and all"
    );

    // A command carrying neither flag is the same answer rather than two
    // panics: nothing was asked, so nothing is answered.
    let bare = Command::new("bare").get_matches_from(["bare"]);
    assert_eq!(tree::answers(op, &bare), Answers::new());
}

/// An adopter with one production client and one [`Recorder`] holds a trait
/// object, and the crate's own `send` takes one.
///
/// Both lines below rest on the blanket impl: a reference to a client is a
/// client. The `&dyn` one rests on its `?Sized` half as well — bounded to sized
/// clients the impl covers `&Recorder` and still not the trait object, which is
/// the case that wanted a delegating newtype in the first place.
#[test]
fn a_reference_and_a_trait_object_both_reach_the_crates_own_send() {
    let doc = document();
    let client = Recorder::new()
        .answering_route(
            Method::GET,
            "/vouchers/5",
            StatusCode::OK,
            &json!({"id": 5}),
        )
        .answering_route(
            Method::GET,
            "/vouchers/6",
            StatusCode::OK,
            &json!({"id": 6}),
        );

    let through_reference: &Recorder = &client;
    let through_object: &dyn SyncClient<Error = RecorderError> = &client;

    let five = parse(&doc, &["toy", "vouchers", "get", "--id", "5"]);
    let six = parse(&doc, &["toy", "vouchers", "get", "--id", "6"]);
    let by_reference = tree::dispatch(&doc, doc.base(), &through_reference, &five)
        .expect("a reference to a client is a client");
    let by_object = tree::dispatch(&doc, doc.base(), &through_object, &six)
        .expect("a trait object over a client is a client");

    for outcome in [by_reference, by_object] {
        assert!(matches!(outcome, Outcome::Sent(_)), "{outcome:?}");
    }
    let paths: Vec<String> = client
        .take()
        .iter()
        .map(|request| request.uri().path().to_owned())
        .collect();
    assert_eq!(paths, ["/vouchers/5", "/vouchers/6"]);
    assert_eq!(client.unused(), 0, "both answers were reached");
}
