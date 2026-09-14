//! What the runtime crate makes of the vendor's document plus the adopter's
//! Overlay — with no bless step anywhere, because a CLI-only adopter needs
//! none.
//!
//! The fixtures under `tests/fixtures/` are this crate's own, not the example
//! adoption's. `examples/toy/spec/` holds a document with the same content
//! today and a different owner: there it is the vendor's, and the example is
//! free to evolve it. Pointing these tests at that copy would let a change to
//! the example break the library.

#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use typed_openapi::model::Body;
use typed_openapi::{Document, Effect, Invocation, Values, render, tree};

const TOY: &str = include_str!("fixtures/toy.yaml");
const OVERLAY: &str = include_str!("fixtures/overlay.yaml");

fn document() -> Document {
    Document::load(TOY, OVERLAY).expect("the vendor's document plus the adopter's Overlay")
}

#[test]
fn the_overlay_adds_what_the_vendor_left_out() {
    let doc = document();
    assert!(doc.get("archiveVoucher").is_some(), "the added operation");
    let Body::JsonFields(fields) = doc.get("createVoucher").unwrap().body() else {
        panic!("createVoucher takes a flat JSON body");
    };
    assert!(
        fields.iter().any(|f| f.name() == "internal_ref"),
        "the added field"
    );
}

#[test]
fn the_gate_is_default_closed_and_the_marker_can_only_add_writes() {
    let doc = document();
    let effect = |id: &str| doc.get(id).unwrap_or_else(|| panic!("{id}")).effect();
    assert_eq!(effect("listVouchers"), Effect::Read);
    assert_eq!(effect("getVoucher"), Effect::Read);
    assert_eq!(effect("createVoucher"), Effect::Write);
    assert_eq!(effect("updateVoucher"), Effect::Write);
    assert_eq!(effect("enshrineVoucher"), Effect::Write);
    assert_eq!(effect("archiveVoucher"), Effect::Write);
    // The one fact HTTP cannot carry, and the only thing `x-cli-writes` is for.
    assert_eq!(effect("renderVoucher"), Effect::Write);
}

#[test]
fn every_body_is_exactly_one_flag_set() {
    let doc = document();
    let body = |id: &str| doc.get(id).unwrap_or_else(|| panic!("{id}")).body().clone();
    assert!(matches!(body("getVoucher"), Body::None));
    assert!(matches!(body("createVoucher"), Body::JsonFields(_)));
    // `Contact.address` is nested, so there are no per-field flags at all —
    // rather than dead ones beside a required `--json-body`.
    assert!(matches!(
        body("createContact"),
        Body::JsonWhole { required: true }
    ));
    assert!(matches!(
        body("uploadDocument"),
        Body::Opaque { ref media_type, .. } if media_type == "form-data"
    ));
    assert!(matches!(
        body("uploadDocumentMultipart"),
        Body::Multipart { .. }
    ));
}

#[test]
fn a_body_field_moves_aside_for_a_path_parameter_of_the_same_name() {
    let doc = document();
    let update = doc.get("updateVoucher").unwrap();
    assert_eq!(update.param("id").unwrap().flag(), "id");
    let Body::JsonFields(fields) = update.body() else {
        panic!("updateVoucher takes a flat JSON body");
    };
    let id = fields.iter().find(|f| f.name() == "id").unwrap();
    assert_eq!(id.flag(), "body-id");
    assert!(id.renamed(), "and it says so in its help line");
}

/// The closest thing a runtime-built tree has to a compile-time check, and the
/// one that catches the next flag collision before a user does.
#[test]
fn the_whole_mounted_tree_is_a_valid_clap_command() {
    let doc = document();
    clap::Command::new("toy")
        .subcommand(clap::Command::new("raw").subcommands(tree::commands(&doc)))
        .debug_assert();
}

#[test]
fn a_dry_run_prints_the_request_that_commit_would_send() {
    let doc = document();
    let op = doc.get("updateVoucher").unwrap();
    let values = Values::new()
        .param("id", 5)
        .json(serde_json::json!({"total": "12.50"}));
    let request = Invocation::new(op, values)
        .expect("the values satisfy the operation")
        .request(doc.base())
        .expect("the base URL is a URL");
    assert_eq!(
        render(&request),
        "PUT /vouchers/5 HTTP/1.1\n\
         host: localhost:9999\n\
         content-type: application/json\n\
         \n\
         {\"total\":\"12.50\"}\n"
    );
}

#[test]
fn the_document_rejects_values_it_does_not_describe() {
    let doc = document();
    let op = doc.get("getVoucher").unwrap();
    assert!(
        Invocation::new(op, Values::new()).is_err(),
        "`id` is required"
    );
    assert!(
        Invocation::new(op, Values::new().param("nope", 1)).is_err(),
        "there is no `nope` parameter"
    );
    assert!(
        Invocation::new(op, Values::new().param("id", "five")).is_err(),
        "`id` is an integer"
    );
    assert!(
        Invocation::new(op, Values::new().param("id", 5).json(serde_json::json!({}))).is_err(),
        "getVoucher takes no body"
    );
}

/// Two defences, in this order: the document's own type rejects the value, and
/// anything that does get through is percent-encoded rather than interpolated.
#[test]
fn a_path_value_cannot_smuggle_a_segment_into_the_url() {
    let doc = document();
    let op = doc.get("archiveVoucher").unwrap();
    let refused = Invocation::new(op, Values::new().param("id", "1/../../etc"))
        .expect_err("`id` is `type: integer` in the document");
    assert!(
        refused.to_string().contains("is not an integer"),
        "{refused}"
    );

    // The same value under a parameter the document types as a string.
    let doc = Document::load(
        &TOY.replace(
            "        schema:\n          type: integer\n          format: int64",
            "        schema:\n          type: string",
        ),
        OVERLAY,
    )
    .expect("a document whose ids are strings");
    let op = doc.get("getVoucher").unwrap();
    let request = Invocation::new(op, Values::new().param("id", "1/../../etc"))
        .unwrap()
        .request(doc.base())
        .unwrap();
    assert_eq!(request.uri().path(), "/vouchers/1%2F..%2F..%2Fetc");
}

/// The two doors onto one reduction. A bless step writes the blob, a binary
/// reads it, and nothing between them may change what the document said —
/// including the flag renames, which are decided while reducing and would be a
/// different command line if they were decided again on the way back.
#[test]
fn a_reduction_survives_the_round_trip_the_bless_step_makes() {
    let doc = document();
    let blob = doc.to_blob().expect("the reduction encodes");
    assert_eq!(Document::from_blob(&blob).expect("and decodes"), doc);
}

/// A blob that is not one is a named error, not a panic and not a CLI that
/// starts with half an API.
#[test]
fn a_blob_that_is_not_a_reduction_is_refused_by_name() {
    let error = Document::from_blob(b"not a reduction").expect_err("not a reduction");
    assert!(error.to_string().contains("reduced model"), "{error}");
}
