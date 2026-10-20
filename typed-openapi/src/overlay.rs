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
//! a value first is what separates a text that is not YAML or JSON
//! ([`OverlayError::Syntax`]) from one that is and has the wrong shape
//! ([`OverlayError::Shape`]).
//!
//! The reader follows YAML 1.2 where its defaults follow 1.1, because a
//! document's strings are the vendor's: `yes`, `no`, `on`, `off`, `y` and `n`
//! stay strings, so that an enum value `N` is not `false` and `x-cli-writes:
//! yes` is not `true`. `<<` is an ordinary key, not a merge. A mapping with the
//! same key twice is refused with its line and column. A nesting past 128
//! levels and an alias bomb are refused, and the budget is wide enough for a
//! 10 MB document. Two YAML 1.1 readings remain: `1_000` is the number 1000, and
//! `017` is a float.

use std::error::Error;

use thiserror::Error;

/// The Overlay, or the document it is applied to, is not usable.
///
/// Each variant carries a [`Cause`]: a message, and the place in the text it
/// points at where there is one. No type of the YAML reader or of the Overlay
/// engine is part of this crate's API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OverlayError {
    /// The OpenAPI document is not YAML or JSON.
    #[error("the OpenAPI document is not valid YAML or JSON")]
    Document(#[source] Cause),
    /// The Overlay is not YAML or JSON.
    #[error("the Overlay document is not valid YAML or JSON")]
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

/// What the YAML reader or the Overlay engine said, in this crate's words.
///
/// The message is a whole sentence and already says where it stands. The place
/// is also here as numbers, for a caller that points at it: the line and column
/// of a text the reader refused, or the index of the action that failed. Each
/// is `None` where the thing that refused did not say.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct Cause {
    message: String,
    line: Option<u64>,
    column: Option<u64>,
    action: Option<usize>,
}

impl Cause {
    /// A cause with a message and no place.
    fn new(message: String) -> Self {
        Self {
            message,
            line: None,
            column: None,
            action: None,
        }
    }

    /// The sentence a person reads, with the place in it where there is one.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The line of the text where the reader stopped, counted from 1.
    #[must_use]
    pub fn line(&self) -> Option<u64> {
        self.line
    }

    /// The column of the text where the reader stopped, counted from 1.
    #[must_use]
    pub fn column(&self) -> Option<u64> {
        self.column
    }

    /// The index in `actions` of the action the Overlay engine refused,
    /// counted from 0.
    #[must_use]
    pub fn action(&self) -> Option<usize> {
        self.action
    }
}

/// The options of every YAML read in this crate, and the one place they are
/// set.
///
/// The reader's defaults read YAML 1.1 booleans, merge `<<` keys and refuse a
/// document of a few hundred thousand nodes, and none of the three is right for
/// a vendor's OpenAPI document. The snippet stays off, because a message that
/// draws the file across several lines does not fit under `caused by:`.
fn reader() -> serde_saphyr::Options {
    serde_saphyr::options! {
        strict_booleans: true,
        merge_keys: serde_saphyr::MergeKeyPolicy::AsOrdinary,
        budget: serde_saphyr::budget! {
            max_depth: 128,
            max_nodes: 2_500_000,
            max_events: 10_000_000,
        },
        with_snippet: false,
    }
}

/// Read YAML or JSON text into a value. Every read of a document or an Overlay
/// goes through here.
///
/// The message is the reader's own wording for a person rather than a
/// programmer: the other one advises a caller to change an option this crate
/// sets and its caller cannot.
fn read(text: &str) -> Result<serde_json::Value, Cause> {
    serde_saphyr::from_str_with_options(text, reader()).map_err(|error| Cause {
        line: error.location().map(|at| at.line()),
        column: error.location().map(|at| at.column()),
        ..Cause::new(error.render_with_formatter(&serde_saphyr::UserMessageFormatter))
    })
}

/// Write the corrected document as YAML, with the emitter's defaults.
///
/// The defaults quote every string a YAML 1.1 reader would misread (`"N"`,
/// `"yes"`, `"1.0"`), so a reader that follows YAML 1.1 reads back the strings
/// that were written.
pub(crate) fn emit(document: &serde_json::Value) -> Result<String, Box<dyn Error + Send + Sync>> {
    Ok(serde_saphyr::to_string(document)?)
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
        Cause {
            action,
            ..Cause::new(error.to_string())
        }
    })
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
        OverlayError::Invalid(Cause::new(findings.join("; ")))
    })?;
    overlay
        .apply(
            &mut doc,
            roas_overlay::apply::ApplyOptions::ErrorOnZeroMatch.into(),
        )
        .map_err(|refused| {
            OverlayError::Apply(Cause {
                action: Some(refused.action_index),
                ..Cause::new(refused.to_string())
            })
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
