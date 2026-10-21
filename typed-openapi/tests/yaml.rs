//! What the YAML reader makes of a document and of an Overlay.
//!
//! A vendor's document is the vendor's: its strings are not ours to reinterpret,
//! and its size and shape are not ours to trust. So the reader is held to what
//! it must keep, in the terms a document uses — a string that looks like a
//! boolean stays a string, a mapping that repeats a key is refused where it
//! repeats, and an input built to exhaust the machine is refused quickly rather
//! than answered.
//!
//! The two protections state their numbers: the depth limit is 128 levels, and
//! the alias bomb has a billion leaves. Each test also states how long the
//! refusal may take, which is generous (a refusal takes milliseconds in a
//! release build) and still far below what expanding the bomb would cost: a
//! reader that lost its limit would not finish at all.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use typed_openapi::overlay::{self, Cause, OverlayError, Place};

/// How long a refusal of a hostile input may take. A debug build refuses the
/// alias bomb in about three seconds; expanding it is minutes and gigabytes.
const REFUSAL_WITHIN: Duration = Duration::from_secs(30);

/// The stack of the thread that calls the reader in the depth test. Descending
/// 128 levels takes about 450 KiB in a release build and about 3 MiB in a debug
/// build, and the reader descends all 128 before it refuses the 129th. The
/// caller's stack is smaller than both, so the test passes only when the read
/// does not run on the caller's stack, and a read that does takes the whole
/// process down.
const CALLER_STACK: usize = 256 * 1024;

/// The deepest nesting the reader accepts.
const MAX_DEPTH: usize = 128;

/// Leaves in the alias bomb.
const BOMB_LEAVES: u64 = 1_000_000_000;

/// Leaves in the alias document that is read: the bomb's shape at a size the
/// reader's budget holds.
const READABLE_LEAVES: u64 = 100_000;

/// A minimal Overlay, with `actions` as given, so that a case names only the
/// part that is wrong.
fn overlay_with(actions: &str) -> String {
    format!("overlay: 1.1.0\ninfo: {{ title: t, version: \"1\" }}\n{actions}")
}

/// A mapping `containers` levels deep, one space of indent per level, whose
/// innermost mapping holds one leaf.
fn nested(containers: usize) -> String {
    let mut text = String::new();
    for level in 0..containers - 1 {
        writeln!(text, "{}k:", " ".repeat(level)).expect("a string takes a write");
    }
    writeln!(text, "{}leaf: 1", " ".repeat(containers - 1)).expect("a string takes a write");
    text
}

/// A document whose last anchor stands for `leaves` leaves, a power of ten: ten
/// scalars, then anchors of ten aliases of the one before.
fn alias_bomb(leaves: u64) -> String {
    let mut text = String::from("l0: &l0 [x, x, x, x, x, x, x, x, x, x]\n");
    for level in 1..leaves.ilog10() {
        let aliases = vec![format!("*l{}", level - 1); 10].join(", ");
        writeln!(text, "l{level}: &l{level} [{aliases}]").expect("a string takes a write");
    }
    text
}

fn document_error(text: &str) -> Cause {
    match overlay::parse(text) {
        Err(OverlayError::Document(cause)) => cause,
        other => panic!("expected the reader to refuse the document, got {other:?}"),
    }
}

/// Whether the cause points at `line` of the text the reader refused.
fn is_at_line(cause: &Cause, line: u64) -> bool {
    matches!(cause.place(), Some(Place::Text { line: at, .. }) if at == line)
}

#[test]
fn a_word_that_yaml_1_1_reads_as_a_boolean_stays_a_string() {
    let read = overlay::parse(
        "x-cli-writes: yes\n\
         enum: [N, n, y, Y, yes, Yes, no, NO, on, On, off, Off]\n\
         real: [true, false, True, FALSE]\n",
    )
    .expect("a document");

    assert_eq!(read["x-cli-writes"], json!("yes"));
    assert_eq!(
        read["enum"],
        json!([
            "N", "n", "y", "Y", "yes", "Yes", "no", "NO", "on", "On", "off", "Off"
        ])
    );
    assert_eq!(read["real"], json!([true, false, true, false]));
}

#[test]
fn a_merge_key_is_an_ordinary_key() {
    let read = overlay::parse("base: &b { x: 1 }\nuse:\n  <<: *b\n  y: 2\n").expect("a document");

    assert_eq!(read["use"], json!({ "<<": { "x": 1 }, "y": 2 }));
}

#[test]
fn a_mapping_that_repeats_a_key_is_refused_where_it_repeats() {
    let cause = document_error("a: 1\nb: 2\na: 3\n");

    assert_eq!(cause.place(), Some(Place::Text { line: 3, column: 1 }));
    assert!(
        cause.message().contains("duplicate") && cause.message().contains("line 3"),
        "{cause}"
    );
    // The reader's own wording advises a change to a setting that this crate
    // makes and its caller cannot, and the message leaves that advice out.
    assert!(
        !cause.message().contains("Options") && !cause.message().contains("Policy"),
        "{cause}"
    );
}

#[test]
fn an_overlay_that_repeats_a_key_is_refused_as_not_yaml() {
    let error =
        overlay::apply(json!({}), "overlay: 1.1.0\noverlay: 1.1.0\n").expect_err("a repeated key");

    assert!(
        matches!(&error, OverlayError::Syntax(cause) if is_at_line(cause, 2)),
        "{error:?}"
    );
}

/// The container at depth 128 is read and the one at 129 is not, so the limit
/// is 128 and not the reader's own default of 64. The caller's thread is too
/// small for either descent, so the reader runs on a stack of its own.
#[test]
fn a_nesting_of_128_levels_is_read_and_one_of_129_is_refused_within_seconds() {
    let (read, cause, took) = std::thread::Builder::new()
        .stack_size(CALLER_STACK)
        .spawn(|| {
            let read = overlay::parse(&nested(MAX_DEPTH));
            let started = Instant::now();
            let cause = document_error(&nested(MAX_DEPTH + 1));
            (read, cause, started.elapsed())
        })
        .expect("a thread for the caller")
        .join()
        .expect("the reader refuses a deep document and does not stop its caller");

    assert!(
        read.is_ok(),
        "128 levels is the deepest nesting that is read: {read:?}"
    );
    assert!(is_at_line(&cause, 129), "{cause}");
    assert!(
        took < REFUSAL_WITHIN,
        "129 levels took {took:?} to refuse, limit {REFUSAL_WITHIN:?}"
    );
}

#[test]
fn an_alias_bomb_of_a_billion_leaves_is_refused_within_seconds() {
    let started = Instant::now();
    let cause = document_error(&alias_bomb(BOMB_LEAVES));
    let took = started.elapsed();

    assert!(
        took < REFUSAL_WITHIN,
        "{BOMB_LEAVES} leaves took {took:?} to refuse, limit {REFUSAL_WITHIN:?}: {cause}"
    );
}

/// The control of the test above: the same generator, at a size the budget
/// holds, is read. A fixture that is wrong everywhere would fail here too.
#[test]
fn an_alias_document_of_a_hundred_thousand_leaves_is_read() {
    let read = overlay::parse(&alias_bomb(READABLE_LEAVES)).expect("a small alias document");

    assert_eq!(read["l4"].as_array().map(Vec::len), Some(10));
}

/// The budget is the reader's own to set, and its default of 250 000 nodes
/// refuses a real document of a few megabytes. This one has 260 000.
#[test]
fn a_document_of_more_nodes_than_the_readers_default_budget_is_read() {
    let nodes = 260_000;
    let text = format!("[{}]", vec!["1"; nodes].join(","));

    let read = overlay::parse(&text).expect("a large document is not a hostile one");

    assert_eq!(read.as_array().map(Vec::len), Some(nodes));
}

/// Two readings of YAML 1.1 that the reader cannot switch off. The test pins
/// them as they are and does not endorse them: the day one changes, the test
/// goes red, and the CHANGELOG line that names it needs its edit too.
#[test]
fn two_yaml_1_1_numbers_are_still_read_as_numbers() {
    let read = overlay::parse("separated: 1_000\noctal: 017\n").expect("a document");

    assert_eq!(read["separated"], json!(1000));
    assert_eq!(read["octal"], json!(17.0));
}

/// `.inf` and `.nan` are refused, with the place, because JSON has no value for
/// them and the document is read into a JSON value.
#[test]
fn a_non_finite_number_is_refused_where_it_stands() {
    let cause = document_error("fine: 1\nworse: .inf\n");

    assert!(is_at_line(&cause, 2), "{cause}");
}

#[test]
fn text_that_is_not_yaml_is_a_syntax_error_with_its_place() {
    let error = overlay::apply(json!({}), "actions: [\n").expect_err("an unclosed sequence");

    assert!(
        matches!(&error, OverlayError::Syntax(cause)
            if matches!(cause.place(), Some(Place::Text { .. }))),
        "{error:?}"
    );
}

/// Valid YAML that is not an Overlay is a shape error, and is not called
/// "not valid YAML or JSON".
#[test]
fn an_overlay_of_the_wrong_shape_is_a_shape_error() {
    let no_actions = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\n";
    let wrong_version = "overlay: 1.0.0\ninfo: { title: t, version: \"1\" }\nactions: []\n";
    let no_target = overlay_with("actions:\n  - update: {}\n");

    for (name, text) in [
        ("no actions", no_actions.to_owned()),
        ("a wrong version", wrong_version.to_owned()),
        ("an action with no target", no_target),
    ] {
        let error = overlay::apply(json!({}), &text).expect_err(name);
        assert!(
            matches!(&error, OverlayError::Shape(_)),
            "{name}: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "the Overlay document is valid YAML or JSON but not an Overlay 1.1 document",
            "{name}"
        );
    }
}

#[test]
fn a_shape_error_names_the_action_it_stopped_in() {
    let text = overlay_with(
        "actions:\n  - { target: $.a, update: {} }\n  - { target: $.b, update: {} }\n  - { update: {} }\n",
    );

    let error = overlay::apply(json!({}), &text).expect_err("the third action has no target");

    assert!(
        matches!(&error, OverlayError::Shape(cause)
            if cause.place() == Some(Place::Action(2)) && cause.message().contains("target")),
        "{error:?}"
    );
}

#[test]
fn an_action_that_matches_nothing_names_its_index() {
    let text = overlay_with(
        "actions:\n  - { target: $.info, update: {} }\n  - { target: $.nothing, update: {} }\n",
    );

    let error =
        overlay::apply(json!({ "info": {} }), &text).expect_err("the second matches nothing");

    assert!(
        matches!(&error, OverlayError::Apply(cause) if cause.place() == Some(Place::Action(1))),
        "{error:?}"
    );
}

#[test]
fn a_finding_names_the_action_it_stands_in() {
    let text = overlay_with(
        "actions:\n  - { target: $.a, update: {} }\n  - { target: \"\", update: {} }\n",
    );

    let error = overlay::apply(json!({}), &text).expect_err("the second target is empty");

    assert!(
        matches!(&error, OverlayError::Invalid(cause) if cause.place() == Some(Place::Action(1))),
        "{error:?}"
    );
}

#[test]
fn findings_in_two_actions_name_no_action() {
    let text = overlay_with(
        "actions:\n  - { target: \"\", update: {} }\n  - { target: \"\", update: {} }\n",
    );

    let error = overlay::apply(json!({}), &text).expect_err("both targets are empty");

    assert!(
        matches!(&error, OverlayError::Invalid(cause) if cause.place().is_none()),
        "{error:?}"
    );
}

#[test]
fn a_document_is_read_into_the_same_value_whichever_way_it_is_written() {
    let yaml = overlay::parse("a: [1, 2]\nb: { c: d }\ne: \"1.0\"\n").expect("YAML");
    let json: Value =
        overlay::parse(r#"{"a": [1, 2], "b": {"c": "d"}, "e": "1.0"}"#).expect("JSON");

    assert_eq!(yaml, json);
}
