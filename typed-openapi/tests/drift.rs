//! The half of the drift story that happens before anything is compiled.
//!
//! Two of the Overlay's actions are written as assertions about what the vendor
//! currently says. Applied with `ErrorOnZeroMatch`, a vendor revision that
//! changes the thing being corrected fails here — at bless time — instead of
//! being silently overwritten with a correction that no longer fits.
//!
//! The other three mutations pass this stage and are caught by the compiler
//! instead; `docs/drift.md` has the whole table.
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

/// Mutation 5: `enshrineVoucher` removed. The Overlay does not mention it, so
/// the bless succeeds — this one is the compiler's to catch, through the
/// `documented(..)` assertion in `cli/src/finalize.rs`.
#[test]
fn removing_an_operation_the_overlay_does_not_mention_passes_the_bless() {
    let without = TOY
        .split("  /vouchers/{id}/enshrine:")
        .next()
        .expect("the fixture declares the operation")
        .to_owned()
        + "  /vouchers/{id}/render:"
        + TOY
            .split("  /vouchers/{id}/render:")
            .nth(1)
            .expect("the fixture declares the render operation");
    let doc = Document::load(&without, OVERLAYS).expect("the document still loads");
    assert!(doc.get("enshrineVoucher").is_none());
    assert!(doc.get("renderVoucher").is_some());
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
        "        currency:\n          type: string\n          description: ISO 4217 code\n",
        "",
    );
    assert!(Document::load(&removed, OVERLAYS).is_ok());
}
