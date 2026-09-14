//! Applying the adopter's corrections to the vendor's document.
//!
//! *Requires the `document` feature.*
//!
//! One function, one decision: does this Overlay apply cleanly to this
//! document? Everything downstream sees a single corrected document and never
//! learns that an Overlay existed.

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

/// Parse `document`, apply `overlay` to it, and hand back the result.
///
/// Both arguments are file contents, YAML or JSON. An empty `overlay` runs the
/// vendor's document unpatched.
///
/// The Overlay is applied with `ErrorOnZeroMatch`, which is what makes a
/// correction a check as well as an edit: an action whose JSONPath no longer
/// matches — because the vendor renamed or retyped the thing it corrects — is
/// an error here rather than a silent no-op.
pub fn apply(document: &str, overlay: &str) -> Result<serde_json::Value, OverlayError> {
    use roas_overlay::apply::Apply as _;
    use roas_overlay::validation::Validate as _;

    let mut doc: serde_json::Value =
        serde_yaml_ng::from_str(document).map_err(OverlayError::Document)?;
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

    #[test]
    fn an_empty_overlay_leaves_the_document_alone() {
        let out = apply(DOC, "   \n").expect("the document parses");
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
        let error = apply(DOC, overlay).expect_err("zero matches must fail");
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
        let out = apply(DOC, overlay).expect("the action matches");
        assert_eq!(out["info"]["title"], "patched");
    }
}
