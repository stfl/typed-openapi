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

use std::collections::{BTreeMap, BTreeSet};

use api::{CORRECTIONS, Correction, Money, OPERATIONS, Voucher};
use typed_openapi::{Document, Effect, Gate};

/// The vendor's document, unpatched. `api` never reads this — that is the
/// point: it is the thing the corrections are corrections *to*.
const VENDOR: &str = include_str!("../../spec/toy.yaml");

fn vendor() -> Document {
    Document::load(VENDOR, &[]).expect("the vendor's document")
}

fn corrected() -> Document {
    Document::load(api::DOCUMENT, &[]).expect("the embedded, corrected document")
}

/// A document as plain JSON, for the comparisons the model does not carry —
/// what a schema property is declared as, rather than what the CLI makes of it.
fn json(document: &str) -> serde_json::Value {
    typed_openapi::overlay::parse(document).expect("a document")
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

/// One `Correction::Retyped` row, unpacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Retype {
    schema: &'static str,
    property: &'static str,
    named: &'static str,
}

/// Every schema name a document declares.
fn schema_names(document: &serde_json::Value) -> BTreeSet<String> {
    document
        .pointer("/components/schemas")
        .and_then(serde_json::Value::as_object)
        .map(|schemas| schemas.keys().cloned().collect())
        .unwrap_or_default()
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
    guarded: Vec<(&'static str, &'static [&'static str])>,
    retyped: Vec<Retype>,
    undeclared: Vec<(&'static str, &'static str)>,
}

fn rows() -> Rows {
    let mut rows = Rows::default();
    for correction in CORRECTIONS {
        match *correction {
            Correction::Undocumented(id) => rows.undocumented.push(id),
            Correction::Skipped(id) => rows.skipped.push(id),
            Correction::Gated(id) => rows.gated.push(id),
            Correction::Guarded { op, gates } => rows.guarded.push((op, gates)),
            Correction::Retyped {
                schema,
                property,
                named,
            } => rows.retyped.push(Retype {
                schema,
                property,
                named,
            }),
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

/// A named gate is the adopter's reading of what an operation costs to get
/// wrong, so it is a correction only while the vendor's document does not carry
/// it — and only while the operation really does stand behind those words.
#[test]
fn every_named_gate_is_the_overlays_own_and_is_carried() {
    for (id, gates) in rows().guarded {
        let vendor = vendor();
        let before = vendor.get(id).unwrap_or_else(|| {
            panic!("`{id}` is listed as standing behind a gate but the vendor does not ship it")
        });
        assert!(
            before.gates().is_empty(),
            "`{id}` names its own gates in the vendor's document: drop the action and the row"
        );

        let corrected = corrected();
        let after = corrected.get(id).expect("the corrected document keeps it");
        let carried: Vec<&str> = after.gates().iter().map(Gate::as_str).collect();
        assert_eq!(
            carried, *gates,
            "`{id}` does not stand behind the words the row names"
        );
    }
}

/// A retype is a correction only while the vendor leaves the rule unsaid and
/// the schema that says it is the Overlay's own — and only while the generated
/// field really is that newtype.
#[test]
fn every_retype_still_corrects_something() {
    let (before, after) = (json(VENDOR), json(api::DOCUMENT));
    let (vendor, corrected) = (properties(&before), properties(&after));

    for Retype {
        schema,
        property,
        named,
    } in rows().retyped
    {
        let key = (schema.to_owned(), property.to_owned());
        let was = vendor.get(&key).unwrap_or_else(|| {
            panic!("`{schema}.{property}` is listed as retyped but the vendor does not declare it")
        });
        assert!(
            was.get("pattern").is_none(),
            "`{schema}.{property}` is listed as retyped but the vendor now states its own rule"
        );
        assert!(
            !schema_names(&before).contains(named),
            "`{named}` is listed as a schema the Overlay adds, and the vendor now declares it"
        );
        assert!(
            after
                .pointer(&format!("/components/schemas/{named}/pattern"))
                .is_some(),
            "the corrected document's `{named}` states no rule, so the newtype enforces nothing"
        );
        assert_eq!(
            corrected
                .get(&key)
                .and_then(|declaration| declaration.get("$ref"))
                .and_then(serde_json::Value::as_str),
            Some(format!("#/components/schemas/{named}").as_str()),
            "`{schema}.{property}` does not point at `{named}`"
        );
    }

    // And the name landed in Rust: this line does not compile if `total` is
    // typify's own `String` rather than the generated newtype.
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
/// row explains fails here. Editing an Overlay without editing the list does not
/// compile away, it turns red.
///
/// This half is what a command line makes of an operation — whether it writes,
/// and which words it stands behind.
#[test]
fn every_operation_the_command_line_treats_differently_has_a_row() {
    let Rows { gated, guarded, .. } = rows();

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
        assert!(
            before.gates() == op.gates() || guarded.iter().any(|(id, _)| *id == op.id()),
            "`{}` stands behind words the vendor's document does not name, and no row says so",
            op.id()
        );
    }
    // And no gate exists that no row named: `Document::gates` is the whole
    // list, so a word added to an Overlay has nowhere to hide.
    for gate in corrected.gates() {
        assert!(
            guarded
                .iter()
                .any(|(_, gates)| gates.contains(&gate.as_str())),
            "`{gate}` is a gate the Overlay names and no row says so"
        );
    }
}

/// The same direction over what the two documents *declare*: a property that
/// arrived, changed, or left, and a schema the Overlay wrote down under a name.
#[test]
fn every_declaration_that_differs_between_the_documents_has_a_row() {
    let Rows {
        retyped,
        undeclared,
        ..
    } = rows();

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
            Some(was) if was != declaration => assert!(
                retyped.iter().any(|row| (row.schema, row.property) == pair),
                "`{schema}.{property}` is declared differently from the vendor's \
                 document and no row says so"
            ),
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

    // A whole schema the corrected document has and the vendor's does not is
    // the Overlay writing a rule down under a name, and a row has to name it.
    let added = schema_names(&json(api::DOCUMENT));
    for schema in added.difference(&schema_names(&json(VENDOR))) {
        assert!(
            retyped.iter().any(|row| row.named == schema),
            "`{schema}` is a schema the Overlay adds and no row says so"
        );
    }
}

/// The corrections are split into layers by purpose, and this is the half of
/// that split worth enforcing rather than merely intending.
///
/// `corrections.yaml` is about the vendor's API: applying it alone yields the
/// document the vendor should have shipped, which is worth having on its own
/// — it can go back to the vendor, or into a generator for another language.
/// An `x-cli-` marker in it would make it a document about this CLI instead.
/// `cli.yaml` is where those live, and it is applied after.
#[test]
fn the_vendor_layer_says_nothing_about_a_command_line() {
    const CORRECTIONS: &str = include_str!("../../spec/corrections.yaml");
    const CLI: &str = include_str!("../../spec/cli.yaml");

    assert!(
        !CORRECTIONS.contains("x-cli-"),
        "`spec/corrections.yaml` carries a marker only this CLI reads, so applying \
         it alone no longer yields a document about the vendor's API"
    );
    // The other half: a check that cannot say yes has not said no above.
    assert!(
        CLI.contains("x-cli-writes"),
        "`spec/cli.yaml` no longer carries the marker the gate reads, so either \
         the gate is uncorrected or this check can no longer see it"
    );
}
