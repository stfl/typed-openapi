//! The guard of `Document::MODEL_LAYOUT`.
//!
//! A model blob opens with the number of the layout it was written in, and a
//! build refuses a blob of another number. That protects an adopter only while
//! the number moves with the layout, and a number moved by hand is a number
//! somebody forgets. So this file traces the serde layout of `Document` and of
//! every type inside it, and compares it with `tests/layout.json`, which holds
//! the description of the layout the number names. A layout that changed under
//! a number that did not fails here, and the failure says what to do.
//!
//! The trace is of types, not values: nothing here reads a fixture, and a change
//! to a document never moves it.
//!
//! To write the description again after a bump:
//! `TYPED_OPENAPI_WRITE_LAYOUT=1 cargo nextest run -p typed-openapi --test layout`.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot trace the model should fail loudly and say why"
)]

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_reflection::{ContainerFormat, Registry, Samples, Tracer, TracerConfig};
use typed_openapi::scalar::{Bounds, Limit};
use typed_openapi::{Body, Document, Effect, Join, Location, Scalar, Shape, Unsupported};

/// The description the layout is held to, written by this test.
const COMMITTED: &str = include_str!("layout.json");

/// The environment variable that makes this test write the description instead
/// of reading it.
const WRITE: &str = "TYPED_OPENAPI_WRITE_LAYOUT";

/// What the tracer hands to every field that reads a string.
///
/// Some fields of the model check what they read, and the tracer would give each
/// of them an empty string and be refused: the base `Uri` and the `Method` have
/// their own deserialisers (`serde(with)`), and the command names and gates
/// convert from a string (`try_from`). Their serialisers write a plain string,
/// not a named container, so the tracer cannot record a sample of one and has
/// one default string for them all. `get` is the shortest word that each of them
/// accepts: a method, a host for a `Uri`, and lowercase letters for a name.
/// A field that checks more than they do is refused when it is traced, and the
/// failure names this constant.
const STRING: &str = "get";

/// The description of the layout `MODEL_LAYOUT` names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Description {
    /// The layout number the traces were written under.
    model_layout: u32,
    /// The registry of containers under each root, by the root's name.
    ///
    /// `serde-reflection` names a container without its type arguments, so a
    /// generic type that is used with two arguments has two layouts and one
    /// name. `Bounds<i64>` is traced under `Document`, and `Bounds<f64>` has a
    /// trace of its own.
    traces: BTreeMap<String, Registry>,
}

/// Trace `T`, and every variant of it where it is an enum.
fn trace<'de, T: Deserialize<'de>>(tracer: &mut Tracer, samples: &'de Samples) {
    let name = std::any::type_name::<T>();
    if let Err(error) = tracer.trace_type::<T>(samples) {
        panic!(
            "cannot trace `{name}`: {error:?}\n{}\n\
             A field that checks what it reads may refuse the string `{STRING:?}` in \
             tests/layout.rs.",
            error.explanation()
        );
    }
}

/// The registry the tracer built, or the reason it could not.
fn registry(tracer: Tracer) -> Registry {
    tracer.registry().unwrap_or_else(|error| {
        panic!(
            "cannot describe the layout: {error:?}\n{}\n\
             An enum of the model that is not traced on its own is listed in \
             tests/layout.rs next to `Body`.",
            error.explanation()
        )
    })
}

fn tracer() -> Tracer {
    Tracer::new(TracerConfig::default().default_string_value(STRING.to_owned()))
}

/// The layout of `Document` and of everything it holds.
///
/// A tracer reads one variant of an enum for each type it traces, so every enum
/// of the model is traced on its own as well. One that is missing here makes the
/// registry refuse to describe the layout, which fails this test by name and
/// does not leave a variant out of the description.
fn document_trace() -> Registry {
    let mut tracer = tracer();
    let samples = Samples::new();
    trace::<Document>(&mut tracer, &samples);
    trace::<Body>(&mut tracer, &samples);
    trace::<Effect>(&mut tracer, &samples);
    trace::<Join>(&mut tracer, &samples);
    trace::<Limit<i64>>(&mut tracer, &samples);
    trace::<Location>(&mut tracer, &samples);
    trace::<Scalar>(&mut tracer, &samples);
    trace::<Shape>(&mut tracer, &samples);
    trace::<Unsupported>(&mut tracer, &samples);
    registry(tracer)
}

/// The second instance of the generic bounds of a number.
fn number_trace() -> Registry {
    let mut tracer = tracer();
    let samples = Samples::new();
    trace::<Bounds<f64>>(&mut tracer, &samples);
    trace::<Limit<f64>>(&mut tracer, &samples);
    registry(tracer)
}

/// The type arguments `Scalar` is written with.
///
/// The trace names `Bounds` and `Limit` and not the type they hold, so a change
/// of `Bounds<f64>` to another number type would not move it. This function
/// does not compile then. That is a change to the layout: bump
/// `Document::MODEL_LAYOUT`, change the type here, and write the description
/// again.
fn arguments(scalar: &Scalar) {
    match scalar {
        Scalar::Integer(bounds) => {
            let _: &Bounds<i64> = bounds;
        }
        Scalar::Number(bounds) => {
            let _: &Bounds<f64> = bounds;
        }
        Scalar::Text(_) | Scalar::Boolean | Scalar::Choice(_) => {}
    }
}

/// The layout of the model, as this build has it.
fn traced() -> Description {
    arguments(&Scalar::Boolean);
    Description {
        model_layout: Document::MODEL_LAYOUT,
        traces: BTreeMap::from([
            ("Bounds<f64>".to_owned(), number_trace()),
            ("Document".to_owned(), document_trace()),
        ]),
    }
}

/// The description in `tests/layout.json`.
fn committed() -> Description {
    serde_json::from_str(COMMITTED).expect("tests/layout.json is a description of the layout")
}

/// The name of each container whose layout differs between two traces.
fn differing(traced: &Description, committed: &Description) -> Vec<String> {
    let mut names = Vec::new();
    for (root, new) in &traced.traces {
        let old = committed.traces.get(root);
        let containers = new.keys().chain(old.into_iter().flat_map(BTreeMap::keys));
        for name in containers {
            if old.and_then(|old| old.get(name)) != new.get(name) {
                names.push(format!("{root}/{name}"));
            }
        }
    }
    names.extend(
        committed
            .traces
            .keys()
            .filter(|root| !traced.traces.contains_key(*root))
            .cloned(),
    );
    names.sort();
    names.dedup();
    names
}

/// What the traced layout says about the committed one: nothing, or what is
/// wrong and what to do about it.
fn verdict(traced: &Description, committed: &Description) -> Result<(), String> {
    let again = format!("{WRITE}=1 cargo nextest run -p typed-openapi --test layout");
    let (current, described) = (traced.model_layout, committed.model_layout);
    match (traced.traces == committed.traces, current == described) {
        (true, true) => Ok(()),
        (true, false) => Err(format!(
            "`Document::MODEL_LAYOUT` is {current} and tests/layout.json describes layout {described}. \
             Write the description again: `{again}`."
        )),
        (false, true) => Err(format!(
            "The serde layout of the model changed ({}), and `Document::MODEL_LAYOUT` did not. \
             A blob written before the change would decode into a wrong model or fail as a \
             codec error. Bump `MODEL_LAYOUT` in typed-openapi/src/model.rs and write the \
             description again: `{again}`.",
            differing(traced, committed).join(", ")
        )),
        (false, false) => Err(format!(
            "`Document::MODEL_LAYOUT` is {current}, and the layout it names differs from the \
             description of layout {described} in tests/layout.json ({}). Write the description \
             again: `{again}`.",
            differing(traced, committed).join(", ")
        )),
    }
}

#[test]
fn the_layout_of_the_model_is_the_one_the_committed_description_names() {
    let traced = traced();
    if std::env::var_os(WRITE).is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/layout.json");
        let text = serde_json::to_string_pretty(&traced).expect("the description is JSON");
        std::fs::write(&path, text + "\n").expect("tests/layout.json is writable");
        return;
    }
    if let Err(message) = verdict(&traced, &committed()) {
        panic!("{message}");
    }
}

/// The description is of the layout and nothing else, so a trace is the same
/// each time it is made and a change shows as a change.
#[test]
fn a_trace_is_the_same_each_time() {
    assert_eq!(traced(), traced());
}

/// Two adjacent fields of different types in one struct swap places. The model
/// still compiles, and every blob written before decodes into a wrong one.
fn swapped(description: &Description) -> Description {
    let mut changed = description.clone();
    let operation = changed
        .traces
        .get_mut("Document")
        .and_then(|registry| registry.get_mut("Operation"))
        .expect("the trace holds `Operation`");
    let ContainerFormat::Struct(fields) = operation else {
        panic!("`Operation` is a struct");
    };
    let at = fields
        .windows(2)
        .position(|pair| pair.first().map(|f| &f.value) != pair.get(1).map(|f| &f.value))
        .expect("two adjacent fields of different types");
    fields.swap(at, at + 1);
    changed
}

#[test]
fn a_layout_that_changed_under_the_same_number_fails_and_names_what_to_do() {
    let committed = traced();
    let message = verdict(&swapped(&committed), &committed).expect_err("the layout moved");
    assert!(message.contains("MODEL_LAYOUT"), "{message}");
    assert!(message.contains("Bump"), "{message}");
    assert!(message.contains("write the description again"), "{message}");
    assert!(message.contains("Document/Operation"), "{message}");
}

#[test]
fn a_bumped_number_without_the_description_fails_and_says_to_write_it() {
    let committed = traced();
    let mut bumped = swapped(&committed);
    bumped.model_layout += 1;
    let message = verdict(&bumped, &committed).expect_err("the description is stale");
    assert!(message.contains("MODEL_LAYOUT"), "{message}");
    assert!(message.contains("description again"), "{message}");

    let mut only_bumped = committed.clone();
    only_bumped.model_layout += 1;
    let message = verdict(&only_bumped, &committed).expect_err("the description is stale");
    assert!(message.contains("describes layout"), "{message}");
}
