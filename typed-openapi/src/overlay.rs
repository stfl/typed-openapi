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

use thiserror::Error;

/// The Overlay, or the document it is applied to, is not usable.
#[derive(Debug, Error)]
pub enum OverlayError {
    #[error("the OpenAPI document is not valid YAML or JSON: {0}")]
    Document(#[source] serde_yaml_ng::Error),
    #[error("the Overlay document is not valid YAML or JSON: {0}")]
    Syntax(#[source] serde_yaml_ng::Error),
    #[error("the Overlay is not a valid Overlay 1.1 document: {0}")]
    Invalid(#[source] roas_overlay::validation::Error),
    #[error("the Overlay does not apply: {0}")]
    Apply(#[source] roas_overlay::apply::ApplyError),
}

/// Read a document — YAML or JSON, as the vendor ships it — into the value a
/// correction is applied to.
pub fn parse(document: &str) -> Result<serde_json::Value, OverlayError> {
    serde_yaml_ng::from_str(document).map_err(OverlayError::Document)
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
    let overlay: roas_overlay::v1_1::Overlay =
        serde_yaml_ng::from_str(overlay).map_err(OverlayError::Syntax)?;
    #[expect(
        clippy::default_trait_access,
        reason = "the option set is an `enumset::EnumSet`, and naming it here \
                  would mean taking `enumset` as a direct dependency for one word"
    )]
    overlay
        .validate(Default::default())
        .map_err(OverlayError::Invalid)?;
    overlay
        .apply(
            &mut doc,
            roas_overlay::apply::ApplyOptions::ErrorOnZeroMatch.into(),
        )
        .map_err(OverlayError::Apply)?;
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
