//! The accounting every regeneration owes the adopter.
//!
//! Two documents are in play: the vendor's, exactly as it ships, and the
//! corrected one this crate embeds. Every difference between them is supposed
//! to be one row of [`api::CORRECTIONS`]. These tests are what make "supposed
//! to be" into "is" — in both directions, so neither a correction the vendor
//! has caught up with nor an Overlay edit nobody wrote down survives a run.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::collections::BTreeMap;

use api::{CORRECTIONS, Correction, Money, OPERATIONS, Voucher};
use typed_openapi::{Document, Effect};

/// The vendor's document, unpatched. `api` never reads this — that is the
/// point: it is the thing the corrections are corrections *to*.
const VENDOR: &str = include_str!("../../spec/toy.yaml");

fn vendor() -> Document {
    Document::load(VENDOR, "").expect("the vendor's document")
}

fn corrected() -> Document {
    Document::load(api::DOCUMENT, "").expect("the embedded, corrected document")
}

/// A document as plain JSON, for the comparisons the model does not carry —
/// what a schema property is declared as, rather than what the CLI makes of it.
fn json(document: &str) -> serde_json::Value {
    typed_openapi::overlay::apply(document, "").expect("a document")
}

/// Every schema property of a document, keyed by `(schema, property)`.
fn properties(document: &serde_json::Value) -> BTreeMap<(String, String), serde_json::Value> {
    let mut out = BTreeMap::new();
    let Some(schemas) = document
        .pointer("/components/schemas")
        .and_then(serde_json::Value::as_object)
    else {
        return out;
    };
    for (schema, body) in schemas {
        let Some(declared) = body
            .get("properties")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (property, declaration) in declared {
            out.insert((schema.clone(), property.clone()), declaration.clone());
        }
    }
    out
}

fn mounted(id: &str) -> bool {
    OPERATIONS.iter().any(|(mounted, _, _)| *mounted == id)
}

/// The rows, sorted into the question each kind of correction answers.
///
/// One exhaustive match, with no wildcard arm, so a `Correction` variant added
/// later is a compile error here — where someone has to decide what checking it
/// means — rather than a row that quietly goes unchecked.
#[derive(Debug, Default)]
struct Rows {
    undocumented: Vec<&'static str>,
    skipped: Vec<&'static str>,
    gated: Vec<&'static str>,
    retyped: Vec<&'static str>,
    undeclared: Vec<(&'static str, &'static str)>,
}

fn rows() -> Rows {
    let mut rows = Rows::default();
    for correction in CORRECTIONS {
        match *correction {
            Correction::Undocumented(id) => rows.undocumented.push(id),
            Correction::Skipped(id) => rows.skipped.push(id),
            Correction::Gated(id) => rows.gated.push(id),
            Correction::Retyped { format, .. } => rows.retyped.push(format),
            Correction::Undeclared { schema, property } => {
                rows.undeclared.push((schema, property));
            }
        }
    }
    rows
}

/// The survey: an operation that appears in the vendor's document, or leaves
/// it, is named here or the build stops.
#[test]
fn operations_are_accounted_for() {
    let Rows {
        skipped,
        undocumented,
        ..
    } = rows();

    for op in &vendor() {
        assert!(
            mounted(op.id()) || skipped.contains(&op.id()),
            "`{}` is in the vendor's document but is neither mounted nor listed as skipped",
            op.id()
        );
    }
    for id in skipped {
        assert!(
            vendor().get(id).is_some(),
            "`{id}` is listed as skipped but the vendor no longer describes it"
        );
        assert!(!mounted(id), "`{id}` is listed as skipped but is mounted");
    }
    for (id, _, _) in OPERATIONS {
        assert!(
            vendor().get(id).is_some() || undocumented.contains(id),
            "`{id}` is mounted but the vendor's document does not describe it, \
             and it is not listed as undocumented"
        );
    }
}

/// A correction the vendor has caught up with would go on working in silence
/// and hide the fact that the Overlay is now fighting the document. This is the
/// only place that can happen, and it cannot.
#[test]
fn nothing_hand_written_shadows_the_document() {
    for id in rows().undocumented {
        assert!(
            vendor().get(id).is_none(),
            "`{id}` is listed as undocumented but the vendor now describes it: \
             drop the Overlay action and the row"
        );
        assert!(
            mounted(id),
            "`{id}` is listed as undocumented but not mounted"
        );
    }
}

/// A gate is a correction only while the document's own method says otherwise.
/// A vendor who moves the operation to POST makes the row redundant, and says so
/// here.
#[test]
fn every_gate_still_corrects_something() {
    for id in rows().gated {
        let vendor = vendor();
        let before = vendor
            .get(id)
            .unwrap_or_else(|| panic!("`{id}` is listed as gated but the vendor does not ship it"));
        assert_eq!(
            before.effect(),
            Effect::Read,
            "`{id}` is listed as gated but the vendor's own method already writes"
        );
        let corrected = corrected();
        let after = corrected.get(id).expect("the corrected document keeps it");
        assert_eq!(
            after.effect(),
            Effect::Write,
            "`{id}` is not gated after all"
        );
    }
}

/// A retype is a correction only while the vendor still declares the format it
/// keys on, and only while the generated field really is the adopter's type.
#[test]
fn every_retype_still_corrects_something() {
    for format in rows().retyped {
        assert!(
            VENDOR.contains(&format!("format: {format}")),
            "no schema declares `format: {format}` any more"
        );
    }
    // And the substitution landed: this line does not compile if `total` is
    // typify's own `String` rather than the adopter's newtype.
    let total: fn(&Voucher) -> &Money = |voucher| &voucher.total;
    let _ = total;
}

/// An undeclared property is a correction only while the vendor leaves it out.
#[test]
fn every_undeclared_property_is_still_undeclared() {
    for (schema, property) in rows().undeclared {
        let declared = |document: &str| {
            document
                .split(&format!("{schema}:"))
                .nth(1)
                .is_some_and(|rest| rest.contains(&format!("{property}:")))
        };
        assert!(
            !declared(VENDOR),
            "`{schema}.{property}` is listed as undeclared but the vendor now declares it"
        );
        assert!(
            declared(api::DOCUMENT),
            "`{schema}.{property}` is listed as undeclared but the Overlay no longer adds it"
        );
    }
}

/// The other direction, and the one that keeps [`api::CORRECTIONS`] from being
/// a second copy of the Overlay: a difference between the two documents that no
/// row explains fails here. Editing `spec/overlay.yaml` without editing the list
/// does not compile away, it turns red.
#[test]
fn every_difference_between_the_documents_has_a_row() {
    let Rows {
        gated,
        retyped,
        undeclared,
        ..
    } = rows();

    let (vendor, corrected) = (vendor(), corrected());
    for op in &corrected {
        let Some(before) = vendor.get(op.id()) else {
            continue;
        };
        assert!(
            before.effect() == op.effect() || gated.contains(&op.id()),
            "`{}` is gated differently from the vendor's own method and no row says so",
            op.id()
        );
    }

    let before = properties(&json(VENDOR));
    let after = properties(&json(api::DOCUMENT));
    for ((schema, property), declaration) in &after {
        let pair = (schema.as_str(), property.as_str());
        match before.get(&(schema.clone(), property.clone())) {
            None => assert!(
                undeclared.contains(&pair),
                "`{schema}.{property}` is in the corrected document and not the vendor's, \
                 and no row says so"
            ),
            Some(was) if was != declaration => {
                let format = declaration
                    .get("format")
                    .and_then(serde_json::Value::as_str);
                assert!(
                    format.is_some_and(|format| retyped.contains(&format)),
                    "`{schema}.{property}` is declared differently from the vendor's \
                     document and no row says so"
                );
            }
            Some(_) => {}
        }
    }
    for (schema, property) in before.keys() {
        assert!(
            after.contains_key(&(schema.clone(), property.clone())),
            "`{schema}.{property}` was removed from the document, which no `Correction` \
             variant describes"
        );
    }
}
