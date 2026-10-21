//! Applying the adopter's corrections to the vendor's document.
//!
//! *Requires the `document` feature.*
//!
//! [`parse`] reads a document into the value a correction edits, and [`apply`]
//! lays one Overlay over it. A chain of the two is how a document with several
//! layers of correction is assembled, and everything downstream sees a single
//! corrected document and never learns that an Overlay existed.
//!
//! Layers are ordinary Overlay documents in an order the caller chose. This
//! module knows nothing about what any of them is *for*: whether a layer
//! repairs the vendor's mistakes, sharpens a type, or marks an operation for a
//! command line is a convention an adoption keeps, not a thing the library
//! can see.
//!
//! # How YAML is read
//!
//! Every document and every Overlay is read by one function into a
//! [`serde_json::Value`]; the Overlay is then built from that value. Reading to
//! a value first is what separates a text that cannot be read as YAML or JSON
//! ([`OverlayError::Syntax`]) from one that can and has the wrong shape
//! ([`OverlayError::Shape`]).
//!
//! The read runs on a thread of its own, with a stack this crate sizes. The
//! reader is recursive and spends stack on each level of nesting, so in a debug
//! build a text at the depth limit needs more stack than a test thread or a
//! Windows main thread has, and a read on the caller's thread would stop the
//! process with a stack overflow where it should return an error.
//!
//! The reader follows YAML 1.2 where its defaults follow 1.1, because a
//! document's strings are the vendor's: `yes`, `no`, `on`, `off`, `y` and `n`
//! stay strings, so that an enum value `N` is not `false` and `x-cli-writes:
//! yes` is not `true`. `<<` is an ordinary key, not a merge. A mapping with the
//! same key twice is refused with its line and column. A nesting past 128
//! levels is refused, and so is a value of more than 2 500 000 nodes or 64 MiB
//! of scalar text, counted with every alias expanded. An alias bomb is far past
//! them. Comments are not read. Two YAML 1.1 readings remain: `1_000` is the
//! number 1000, and `017` is a float.

use thiserror::Error;

/// The Overlay, or the document it is applied to, is not usable.
///
/// Each variant carries a [`Cause`]: a message, and the [`Place`] it points at
/// where there is one. No type of the YAML reader or of the Overlay
/// engine is part of this crate's API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OverlayError {
    /// The OpenAPI document cannot be read as YAML or JSON: the text is not
    /// valid, or it is past a limit of the reader.
    #[error("the OpenAPI document cannot be read as YAML or JSON")]
    Document(#[source] Cause),
    /// The Overlay cannot be read as YAML or JSON: the text is not valid, or it
    /// is past a limit of the reader.
    #[error("the Overlay document cannot be read as YAML or JSON")]
    Syntax(#[source] Cause),
    /// The Overlay is YAML or JSON and does not have the shape of an Overlay
    /// 1.1 document: a field is missing, a field has the wrong type, or the
    /// version is not 1.1.
    #[error("the Overlay document is valid YAML or JSON but not an Overlay 1.1 document")]
    Shape(#[source] Cause),
    /// The Overlay has the shape of an Overlay 1.1 document and breaks a rule
    /// of the specification.
    #[error("the Overlay is not a valid Overlay 1.1 document")]
    Invalid(#[source] Cause),
    /// An action of the Overlay could not be laid over the document.
    #[error("the Overlay does not apply")]
    Apply(#[source] Cause),
}

/// What the YAML reader or the Overlay engine said, as a message and a place.
///
/// It is a type of this crate, so no type of the reader or of the engine is
/// part of the API. The message is a whole sentence and already says where it
/// stands. The place is also here as data, for a caller that points at it, and
/// is `None` where the thing that refused did not say.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct Cause {
    message: String,
    place: Option<Place>,
}

impl Cause {
    fn new(message: String, place: Option<Place>) -> Self {
        Self { message, place }
    }

    /// The sentence a person reads, with the place in it where there is one.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Where the thing that refused stopped, where it said.
    #[must_use]
    pub fn place(&self) -> Option<Place> {
        self.place
    }
}

/// Where in the input a refusal points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Place {
    /// A position in the text the reader refused, with the line and the column
    /// counted from 1.
    Text {
        /// The line of the text where the reader stopped.
        line: u64,
        /// The column of that line where the reader stopped.
        column: u64,
    },
    /// An action of the Overlay, by its index in `actions` counted from 0.
    Action(usize),
}

/// The options of every YAML read in this crate, and the one place they are
/// set.
///
/// The reader's defaults read YAML 1.1 booleans, merge `<<` keys, keep
/// comments, and hold a budget that suits a configuration file. None of the
/// four suits a vendor's OpenAPI document.
///
/// The budget limits the value that a read builds: 2 500 000 nodes and 64 MiB
/// of scalar text, counted with every alias expanded, at a depth of 128. Every
/// other cap is set where a document inside those limits cannot reach it, so
/// that the budget refuses no other size of document. A response defined once
/// and used by a hundred operations is the document working as intended, and a
/// value that occurs a million times is a large document and not a hostile one.
/// The reader counts what an alias replays in the node limit, so that limit
/// refuses an alias bomb as it refuses a text with too many nodes. Only anchors
/// nested inside each other reach the caps on the events and the bytes that
/// anchors keep, and those caps limit the memory of the copies.
///
/// A value holds no comment, and the reader refuses a run of more than 32 of
/// them between a key and its nested value, so comments are not read. The
/// snippet stays off, because a message that draws the file across several
/// lines does not fit under `caused by:`.
fn reader() -> serde_saphyr::Options {
    serde_saphyr::options! {
        strict_booleans: true,
        merge_keys: serde_saphyr::MergeKeyPolicy::AsOrdinary,
        budget: serde_saphyr::budget! {
            max_depth: 128,
            max_nodes: 2_500_000,
            max_events: 10_000_000,
            max_aliases: 2_500_000,
            max_anchors: 2_500_000,
            max_recorded_anchor_events: 10_000_000,
            enforce_alias_anchor_ratio: false,
        },
        alias_limits: serde_saphyr::alias_limits! {
            max_total_replayed_events: 10_000_000,
        },
        emit_comments: false,
        with_snippet: false,
    }
}

/// The stack of the thread that reads, in bytes.
///
/// The reader descends 128 levels before it refuses the 129th, and a debug build
/// spends about 3 MiB of stack to do it. The constant is more than twice that,
/// so the descent does not use the caller's stack. A thread's stack is reserved
/// address space, and the system commits a page of it only when the reader
/// first uses it, so a text of a real document's depth costs what it uses.
const READER_STACK: usize = 8 * 1024 * 1024;

/// Read YAML or JSON text into a value. Every read of a document or an Overlay
/// goes through here.
///
/// The read runs on a thread whose stack is [`READER_STACK`], so a nesting at
/// the limit is read, and one past it is refused, without the descent using the
/// caller's stack. The caller's thread still runs the scope and the join, and
/// drops the value. A panic in the reader comes back on the caller's thread,
/// where the caller would see it without the thread. A read does not fail
/// because of a thread: where the system refuses one, as on a target with no
/// threads, the read happens on the caller's.
fn read(text: &str) -> Result<serde_json::Value, Cause> {
    std::thread::scope(|scope| {
        let spawned = std::thread::Builder::new()
            .name("typed-openapi-yaml".to_owned())
            .stack_size(READER_STACK)
            .spawn_scoped(scope, || read_here(text));
        match spawned {
            Ok(reading) => reading
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            Err(_) => read_here(text),
        }
    })
}

/// The read itself, on whatever thread calls it. The [`Cause`] is built here,
/// so no type of the reader leaves the thread.
///
/// The message is the reader's own wording for a person rather than a
/// programmer: the other one advises a caller to change an option this crate
/// sets and its caller cannot.
fn read_here(text: &str) -> Result<serde_json::Value, Cause> {
    serde_saphyr::from_str_with_options(text, reader()).map_err(|error| {
        let place = error.location().map(|at| Place::Text {
            line: at.line(),
            column: at.column(),
        });
        Cause::new(
            error.render_with_formatter(&serde_saphyr::UserMessageFormatter),
            place,
        )
    })
}

/// Read a document — YAML or JSON, as the vendor ships it — into the value a
/// correction is applied to.
pub fn parse(document: &str) -> Result<serde_json::Value, OverlayError> {
    read(document).map_err(OverlayError::Document)
}

/// Build the Overlay from the value its text read into, and say which action
/// it was building when a field was wrong.
fn overlay_of(value: serde_json::Value) -> Result<roas_overlay::v1_1::Overlay, Cause> {
    use serde_path_to_error::Segment;

    serde_path_to_error::deserialize(value).map_err(|error| {
        let mut path = error.path().iter();
        let action = match (path.next(), path.next()) {
            (Some(Segment::Map { key }), Some(Segment::Seq { index })) if key == "actions" => {
                Some(*index)
            }
            _ => None,
        };
        Cause::new(error.to_string(), action.map(Place::Action))
    })
}

/// The index in `actions` that every finding of a validation names, if they
/// name one action and the same one. The engine writes a finding's path as
/// `#.actions[3].target`.
fn action_of_all(findings: &[roas_overlay::validation::ValidationError]) -> Option<usize> {
    let action = |path: &str| {
        path.strip_prefix("#.actions[")?
            .split_once(']')?
            .0
            .parse()
            .ok()
    };
    let first = action(&findings.first()?.path)?;
    findings
        .iter()
        .all(|finding| action(&finding.path) == Some(first))
        .then_some(first)
}

/// Lay one Overlay over the document as it stands, and hand back the result.
///
/// `overlay` is a file's contents, YAML or JSON. An empty one leaves the
/// document alone, so a layer an adoption has not written yet costs nothing.
///
/// Taking and returning the document by value is what lets layers chain: the
/// second Overlay corrects what the first produced, which is what makes the
/// order of a list of layers meaningful.
///
/// The Overlay is applied with `ErrorOnZeroMatch`, which is what makes a
/// correction a check as well as an edit: an action whose JSONPath no longer
/// matches — because the vendor renamed or retyped the thing it corrects — is
/// an error here rather than a silent no-op.
pub fn apply(mut doc: serde_json::Value, overlay: &str) -> Result<serde_json::Value, OverlayError> {
    use roas_overlay::apply::Apply as _;
    use roas_overlay::validation::Validate as _;

    if overlay.trim().is_empty() {
        return Ok(doc);
    }
    let overlay = read(overlay).map_err(OverlayError::Syntax)?;
    let overlay = overlay_of(overlay).map_err(OverlayError::Shape)?;
    #[expect(
        clippy::default_trait_access,
        reason = "the option set is an `enumset::EnumSet`, and naming it here \
                  would mean taking `enumset` as a direct dependency for one word"
    )]
    overlay.validate(Default::default()).map_err(|invalid| {
        let findings: Vec<String> = invalid.errors.iter().map(ToString::to_string).collect();
        let place = action_of_all(&invalid.errors).map(Place::Action);
        OverlayError::Invalid(Cause::new(findings.join("; "), place))
    })?;
    overlay
        .apply(
            &mut doc,
            roas_overlay::apply::ApplyOptions::ErrorOnZeroMatch.into(),
        )
        .map_err(|refused| {
            OverlayError::Apply(Cause::new(
                refused.to_string(),
                Some(Place::Action(refused.action_index)),
            ))
        })?;
    Ok(doc)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    const DOC: &str = r#"{"openapi":"3.0.3","info":{"title":"t","version":"1"},"paths":{}}"#;

    /// The document as it stands, before any layer.
    fn document() -> serde_json::Value {
        parse(DOC).expect("the document parses")
    }

    #[test]
    fn an_empty_overlay_leaves_the_document_alone() {
        let out = apply(document(), "   \n").expect("an empty layer is no layer");
        assert_eq!(out["openapi"], "3.0.3");
    }

    #[test]
    fn an_action_that_matches_nothing_is_an_error_not_a_no_op() {
        let overlay = r#"
overlay: 1.1.0
info: { title: t, version: "1" }
actions:
  - target: $.components.schemas.Nothing
    remove: true
"#;
        let error = apply(document(), overlay).expect_err("zero matches must fail");
        assert!(matches!(error, OverlayError::Apply(_)), "{error}");
    }

    #[test]
    fn an_action_that_matches_is_applied() {
        let overlay = r#"
overlay: 1.1.0
info: { title: t, version: "1" }
actions:
  - target: $.info
    update: { title: patched }
"#;
        let out = apply(document(), overlay).expect("the action matches");
        assert_eq!(out["info"]["title"], "patched");
    }

    /// Layers are applied in the order they are handed over, so a later one
    /// corrects the document an earlier one produced. Nothing else about an
    /// ordered list of Overlays is worth saying: this is what the order means.
    #[test]
    fn layers_apply_in_the_order_they_are_given() {
        let layer = |title: &str| {
            format!(
                "overlay: 1.1.0\n\
                 info: {{ title: t, version: \"1\" }}\n\
                 actions:\n\
                 \x20 - target: $.info\n\
                 \x20   update: {{ title: {title} }}\n"
            )
        };
        let layered = |first: &str, second: &str| {
            let doc = apply(document(), &layer(first)).expect("the first layer applies");
            apply(doc, &layer(second)).expect("the second layer applies")["info"]["title"]
                .as_str()
                .expect("a title")
                .to_owned()
        };
        assert_eq!(layered("first", "second"), "second");
        assert_eq!(layered("second", "first"), "first");
    }
}
