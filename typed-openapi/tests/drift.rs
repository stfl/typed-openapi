//! The half of the drift story that happens before anything is compiled.
//!
//! Three of the Overlay's actions are written as assertions about what the
//! vendor currently says. Applied with `ErrorOnZeroMatch`, a vendor revision
//! that changes the thing being corrected fails here — at bless time — instead
//! of being silently overwritten with a correction that no longer fits.
//!
//! The other mutations pass this stage and are caught by the compiler instead;
//! `docs/drift.md` has the whole table.
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

use typed_openapi::Document;

const TOY: &str = include_str!("fixtures/toy.yaml");
const CORRECTIONS: &str = include_str!("fixtures/corrections.yaml");
const CLI: &str = include_str!("fixtures/cli.yaml");

/// The layers, in the order a bless step applies them.
const OVERLAYS: &[&str] = &[CORRECTIONS, CLI];

/// The vendor's document with one line replaced.
fn mutated(from: &str, to: &str) -> String {
    assert!(
        TOY.contains(from),
        "the fixture no longer contains `{from}`"
    );
    TOY.replacen(from, to, 1)
}

/// Every layer over `document`, in the order a bless step applies them.
fn bless(document: &str) -> Result<serde_json::Value, typed_openapi::overlay::OverlayError> {
    let mut doc = typed_openapi::overlay::parse(document)?;
    for layer in OVERLAYS {
        doc = typed_openapi::overlay::apply(doc, layer)?;
    }
    Ok(doc)
}

#[test]
fn the_unmutated_document_blesses() {
    assert!(bless(TOY).is_ok());
}

/// Mutation 3: `total` renamed. The filter action's JSONPath stops matching.
#[test]
fn renaming_the_corrected_field_fails_the_bless() {
    let error = bless(&mutated("        total:\n", "        amount:\n"))
        .expect_err("the Overlay corrects a field that is no longer there");
    assert!(error.to_string().contains("does not apply"), "{error}");
}

/// Mutation 4: `total` retyped as a number. Same action, same reason — the
/// correction asserts `format: money` and the vendor no longer says it.
#[test]
fn retyping_the_corrected_field_fails_the_bless() {
    let error = bless(&mutated(
        "          type: string\n          format: money\n",
        "          type: number\n",
    ))
    .expect_err("the Overlay corrects a shape that is no longer there");
    assert!(error.to_string().contains("does not apply"), "{error}");
}

/// Mutation 5: the vendor factors the currency code into a schema of their own.
/// The correction states a rule for a bare string and there is no bare string
/// left, so the filter stops matching — which is the right outcome, because
/// merging this Overlay's `$ref` over the vendor's would quietly replace the
/// schema the vendor now says is the right one.
#[test]
fn retyping_the_corrected_currency_fails_the_bless() {
    let error = bless(&mutated(
        "        currency:\n          type: string\n          description: ISO 4217 code\n",
        "        currency:\n          $ref: '#/components/schemas/CurrencyCode'\n",
    ))
    .expect_err("the Overlay states a rule for a shape that is no longer there");
    assert!(error.to_string().contains("does not apply"), "{error}");
}

/// The CLI layer's own tripwire, and the reason it is aimed at the method: a
/// gate names a hazard, and the hazard is the operation rather than the path.
/// A vendor who moves the finalize to another method makes the action match
/// nothing, and somebody has to decide whether `enshrine` still names what
/// happens there.
#[test]
fn moving_a_gated_operation_to_another_method_fails_the_bless() {
    let error = bless(&mutated(
        "  /vouchers/{id}/enshrine:\n    post:\n",
        "  /vouchers/{id}/enshrine:\n    put:\n",
    ))
    .expect_err("the Overlay names a gate on an operation that is no longer a POST");
    assert!(error.to_string().contains("does not apply"), "{error}");
}

/// Mutation 6: `enshrineVoucher` withdrawn. The CLI layer names it — a gate is
/// an action like any other — so the withdrawal of the one irreversible
/// operation stops the bless, before the `documented(..)` assertion in
/// `cli/src/finalize.rs` is ever compiled.
#[test]
fn removing_a_gated_operation_fails_the_bless() {
    let error = bless(&without(
        "  /vouchers/{id}/enshrine:",
        "  /vouchers/{id}/render:",
    ))
    .expect_err("the Overlay names a gate on an operation that is gone");
    assert!(error.to_string().contains("does not apply"), "{error}");
}

/// The other half: an operation no action names goes quietly. It stops having
/// a subcommand and nothing else changes, which is why anything that depends on
/// one asserts on it — that assertion is the only thing between a withdrawal
/// and a CLI that is one verb short.
#[test]
fn removing_an_operation_the_overlay_does_not_mention_passes_the_bless() {
    let doc = Document::load(&without("  /contacts:", "  /documents:"), OVERLAYS)
        .expect("the document still loads");
    assert!(doc.get("createContact").is_none());
    assert!(doc.get("createVoucher").is_some());
}

/// The vendor's document with everything from one path key up to the next
/// dropped.
fn without(path: &str, next: &str) -> String {
    let before = TOY
        .split(path)
        .next()
        .unwrap_or_else(|| panic!("the fixture declares `{path}`"));
    let after = TOY
        .split(next)
        .nth(1)
        .unwrap_or_else(|| panic!("the fixture declares `{next}`"));
    format!("{before}{next}{after}")
}

/// Mutations 1 and 2: a field added or removed elsewhere. Neither touches an
/// Overlay target, so both bless cleanly and reach the generated types, where
/// `Posting::of`'s exhaustive destructure turns them into compile errors.
#[test]
fn adding_or_removing_an_unrelated_field_passes_the_bless() {
    let added = mutated(
        "        currency:\n",
        "        note:\n          type: string\n        currency:\n",
    );
    let doc = Document::load(&added, OVERLAYS).expect("the document still loads");
    let typed_openapi::model::Body::JsonFields(fields) =
        doc.get("createVoucher").expect("createVoucher").body()
    else {
        panic!("createVoucher takes a flat JSON body");
    };
    assert!(fields.iter().any(|f| f.name() == "note"));

    let removed = mutated(
        "        id:\n          type: integer\n          format: int64\n          \
         description: Server-assigned id\n",
        "",
    );
    assert!(Document::load(&removed, OVERLAYS).is_ok());
}
