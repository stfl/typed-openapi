//! What the YAML reader makes of a document and of an Overlay.
//!
//! A vendor's document is the vendor's: its strings are not ours to reinterpret,
//! and its size and shape are not ours to trust. So the reader is held to what
//! it must keep, in the terms a document uses — a string that looks like a
//! boolean stays a string, a mapping that repeats a key is refused where it
//! repeats, and an input built to exhaust the machine is refused quickly rather
//! than answered.
//!
//! The two protections state their numbers. A depth of 128 is the nesting this
//! crate has always read, and an alias bomb of a billion leaves is the input
//! that took the old reader 14 ms to refuse. Each test also states how long the
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
use typed_openapi::overlay::{self, OverlayError};

/// How long a refusal of a hostile input may take. A debug build refuses the
/// alias bomb in about three seconds; expanding it is minutes and gigabytes.
const REFUSAL_WITHIN: Duration = Duration::from_secs(30);

/// The stack the depth test gives the reader. A debug build spends about 16 KiB
/// of stack on each level it descends, four times what the reader spends in a
/// release build, so the 2 MiB of a test thread does not hold 128 levels. The
/// refusal of the 129th needs the same room, because the reader descends 128
/// levels before it stops: a document nested past the limit takes the process
/// down, not just the read, on a thread whose stack is smaller.
const STACK_FOR_128_LEVELS: usize = 16 * 1024 * 1024;

/// The deepest nesting the reader accepts.
const MAX_DEPTH: usize = 128;

/// Leaves in the alias bomb: nine levels of ten aliases each.
const BOMB_LEAVES: u64 = 1_000_000_000;

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

/// A document whose last anchor stands for `BOMB_LEAVES` leaves: ten scalars,
/// then eight anchors of ten aliases of the one before.
fn alias_bomb() -> String {
    let mut text = String::from("l0: &l0 [x, x, x, x, x, x, x, x, x, x]\n");
    for level in 1..9 {
        let aliases = vec![format!("*l{}", level - 1); 10].join(", ");
        writeln!(text, "l{level}: &l{level} [{aliases}]").expect("a string takes a write");
    }
    text
}

fn document_error(text: &str) -> overlay::Cause {
    match overlay::parse(text) {
        Err(OverlayError::Document(cause)) => cause,
        other => panic!("expected the reader to refuse the document, got {other:?}"),
    }
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

    assert_eq!((cause.line(), cause.column()), (Some(3), Some(1)));
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
        matches!(&error, OverlayError::Syntax(cause) if cause.line() == Some(2)),
        "{error:?}"
    );
}

/// The container at depth 128 is read and the one at 129 is not, so the limit
/// is the one this crate has always had and not the reader's own default of 64.
#[test]
fn a_nesting_of_128_levels_is_read_and_one_of_129_is_refused_within_seconds() {
    let (read, cause, took) = std::thread::Builder::new()
        .stack_size(STACK_FOR_128_LEVELS)
        .spawn(|| {
            let read = overlay::parse(&nested(MAX_DEPTH));
            let started = Instant::now();
            let cause = document_error(&nested(MAX_DEPTH + 1));
            (read, cause, started.elapsed())
        })
        .expect("a thread with room for the descent")
        .join()
        .expect("the reader refuses a deep document and does not panic on it");

    assert!(
        read.is_ok(),
        "128 levels is the deepest nesting that is read: {read:?}"
    );
    assert_eq!(cause.line(), Some(129), "{cause}");
    assert!(
        took < REFUSAL_WITHIN,
        "129 levels took {took:?} to refuse, limit {REFUSAL_WITHIN:?}"
    );
}

#[test]
fn an_alias_bomb_of_a_billion_leaves_is_refused_within_seconds() {
    let text = alias_bomb();
    assert_eq!(10_u64.pow(9), BOMB_LEAVES, "nine levels of ten");

    let started = Instant::now();
    let cause = document_error(&text);
    let took = started.elapsed();

    assert!(cause.line().is_some(), "{cause}");
    assert!(
        took < REFUSAL_WITHIN,
        "{BOMB_LEAVES} leaves took {took:?} to refuse, limit {REFUSAL_WITHIN:?}"
    );
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

/// Two readings of YAML 1.1 that the reader cannot switch off. They are pinned
/// as they are, not endorsed: the day one is repaired, the test that names it
/// goes red and whoever repaired it reads what the old behaviour was. The
/// CHANGELOG names both.
#[test]
fn two_yaml_1_1_numbers_are_still_read_as_numbers() {
    let read = overlay::parse("separated: 1_000\noctal: 017\n").expect("a document");

    assert_eq!(read["separated"], json!(1000));
    assert_eq!(read["octal"], json!(17.0));
}

/// A document that held `.inf` or `.nan` loaded before, as `null`. It is
/// refused now, with the place, rather than read as a value it was not.
#[test]
fn a_non_finite_number_is_refused_where_it_stands() {
    let cause = document_error("fine: 1\nworse: .inf\n");

    assert_eq!(cause.line(), Some(2));
}

#[test]
fn text_that_is_not_yaml_is_a_syntax_error_with_its_place() {
    let error = overlay::apply(json!({}), "actions: [\n").expect_err("an unclosed sequence");

    assert!(
        matches!(&error, OverlayError::Syntax(cause)
            if cause.line().is_some() && cause.column().is_some()),
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
            if cause.action() == Some(2) && cause.message().contains("target")),
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
        matches!(&error, OverlayError::Apply(cause) if cause.action() == Some(1)),
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
