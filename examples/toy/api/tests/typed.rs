//! What a Rust caller gets, and the one check that keeps the generated
//! wrappers honest about the document they were emitted from.

#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::similar_names,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use api::{Api, Contact, Currency, Money, Posting, Voucher, VoucherStatus};
use http::StatusCode;
use typed_openapi::{Part, Recorder, render};

fn api() -> Api {
    Api::new().expect("the embedded document loads")
}

fn voucher(status: VoucherStatus) -> Voucher {
    Voucher {
        id: Some(5),
        total: "12.50".parse().expect("a valid amount"),
        currency: "EUR".parse().expect("a currency code"),
        status,
        internal_ref: Some("AB-7".to_owned()),
    }
}

/// The committed document is the reviewable artefact; the committed model is
/// the one a binary reads. This is where the two are held to each other.
///
/// Nothing on the CLI's path parses the document any more — `Api::new` loads
/// the reduction the bless step wrote — so reducing it again here is the only
/// check that the reduction shipped is the reduction the document describes. A
/// bless step run on a stale document, a blob edited by hand, a generated file
/// committed without the blob beside it: all of them are this assertion.
#[test]
fn the_embedded_model_is_the_committed_documents_reduction() {
    let reduced = typed_openapi::Document::load(api::DOCUMENT, &[])
        .expect("the committed document is a document");
    let api = api();
    let embedded = api.document();

    // Three assertions rather than one over the whole value: a `Document`
    // prints as several kilobytes of `Debug`, and a failure here should name
    // what moved rather than hand the reader both copies of everything.
    assert_eq!(embedded.base(), reduced.base(), "the server moved");
    assert_eq!(
        ids(embedded),
        ids(&reduced),
        "the operation list moved — re-run the bless step"
    );
    for (shipped, fresh) in embedded.iter().zip(reduced.iter()) {
        assert_eq!(
            shipped,
            fresh,
            "`{}` is not what the document says it is",
            fresh.id()
        );
    }
}

fn ids(document: &typed_openapi::Document) -> Vec<&str> {
    document.iter().map(typed_openapi::Operation::id).collect()
}

/// The check the bless step cannot make for itself: the embedded model and the
/// generated inventory are the same list, row for row. `Api::new` makes it
/// before it hands out an `Api` — every other test in this file rests on that
/// — and this is the proof that it bites.
#[test]
fn a_document_that_has_drifted_from_the_inventory_is_refused_at_startup() {
    assert!(Api::new().is_ok(), "the committed pair agrees");

    let renamed =
        api::DOCUMENT.replace("operationId: renderVoucher", "operationId: reRenderVoucher");
    let drifted = typed_openapi::Document::load(&renamed, &[]).expect("still a document");
    let error = drifted
        .matches(api::OPERATIONS)
        .expect_err("the inventory still says renderVoucher");
    assert!(error.to_string().contains("renderVoucher"), "{error}");
}

/// Every `<group> <command>` pair the CLI can read off a command line is one
/// of the operations the document describes, and nothing else is.
#[test]
fn a_subcommand_name_maps_to_exactly_one_operation() {
    let api = api();
    for (op, (id, _, _)) in api::OperationId::ALL.iter().zip(api::OPERATIONS) {
        let described = api.operation(*op);
        let (group, command) = (described.group().as_str(), described.command().as_str());
        assert_eq!(
            api::OperationId::from_command(group, command),
            Some(*op),
            "`{group} {command}` (`{id}`)"
        );
    }
    assert_eq!(
        api::OperationId::from_command("vouchers", "no-such-thing"),
        None
    );
    assert_eq!(api::OperationId::from_command("no-such-group", "get"), None);
}

/// And the reverse: the inventory `documented()` reads describes exactly the
/// document, so a `const _: () = assert!(documented(..))` elsewhere means what
/// it says.
#[test]
fn the_inventory_is_the_document() {
    let api = api();
    let from_document: Vec<(String, String, String)> = api
        .document()
        .iter()
        .map(|op| {
            (
                op.id().to_owned(),
                op.method().to_string(),
                op.path().to_owned(),
            )
        })
        .collect();
    let from_inventory: Vec<(String, String, String)> = api::OPERATIONS
        .iter()
        .map(|(id, method, path)| ((*id).to_owned(), (*method).to_owned(), (*path).to_owned()))
        .collect();
    assert_eq!(from_document, from_inventory);
    assert_eq!(api::OPERATION_COUNT, from_document.len());
    assert_eq!(api::OperationId::ALL.len(), from_document.len());
    assert!(api::documented(
        "archiveVoucher",
        "POST",
        "/vouchers/{id}/archive"
    ));
    assert!(!api::documented("archiveVoucher", "GET", "/vouchers"));
}

#[test]
fn a_typed_wrapper_builds_the_request_the_document_describes() {
    let api = api();
    let request = api
        .update_voucher(5, &voucher(VoucherStatus::Draft))
        .expect("the operation exists and the values satisfy it")
        .request()
        .expect("the base URL is a URL");
    assert_eq!(request.method(), http::Method::PUT);
    assert_eq!(request.uri().path(), "/vouchers/5");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(request.body()).unwrap(),
        serde_json::json!({
            "currency": "EUR",
            "id": 5,
            "internal_ref": "AB-7",
            "status": "draft",
            "total": "12.50",
        })
    );
}

/// The undocumented operation costs the adopter no Rust at all: it is in the
/// Overlay, so it is in the document, so it has a wrapper.
#[test]
fn the_undocumented_operation_is_a_wrapper_like_any_other() {
    let api = api();
    let request = api
        .archive_voucher(9)
        .expect("the Overlay added it")
        .request()
        .expect("the base URL is a URL");
    assert_eq!(request.method(), http::Method::POST);
    assert_eq!(request.uri().path(), "/vouchers/9/archive");
}

#[test]
fn a_query_parameter_is_omitted_when_it_is_none() {
    let api = api();
    let all = api.list_vouchers(None, None).unwrap().request().unwrap();
    assert_eq!(all.uri().path_and_query().unwrap(), "/vouchers");
    let some = api
        .list_vouchers(Some(VoucherStatus::Paid), Some(10))
        .unwrap()
        .request()
        .unwrap();
    assert_eq!(
        some.uri().path_and_query().unwrap(),
        "/vouchers?status=paid&limit=10"
    );
}

#[test]
fn a_nested_body_is_one_typed_argument() {
    let api = api();
    let contact = Contact {
        id: None,
        name: "Ada".to_owned(),
        address: api::Address {
            street: "1 Main".to_owned(),
            city: "Vienna".to_owned(),
        },
    };
    let request = api.create_contact(&contact).unwrap().request().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(request.body()).unwrap(),
        serde_json::json!({
            "address": {"city": "Vienna", "street": "1 Main"},
            "name": "Ada",
        })
    );
}

#[test]
fn the_misspelled_upload_sends_the_vendors_own_media_type() {
    let api = api();
    let request = api
        .upload_document(b"PDF-BYTES".to_vec())
        .unwrap()
        .request()
        .unwrap();
    assert_eq!(
        request.headers().get(http::header::CONTENT_TYPE).unwrap(),
        "form-data"
    );
    assert_eq!(request.body(), b"PDF-BYTES");
}

#[test]
fn the_correctly_spelled_upload_is_assembled() {
    let api = api();
    let request = api
        .upload_document_multipart(vec![Part::file("file", "doc.bin", b"PDF".to_vec())])
        .unwrap()
        .request()
        .unwrap();
    let content_type = request
        .headers()
        .get(http::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.starts_with("multipart/form-data; boundary="));
    let body = String::from_utf8_lossy(request.body()).into_owned();
    assert!(body.contains("filename=\"doc.bin\""), "{body}");
}

#[test]
fn a_sync_client_and_an_async_client_send_the_same_bytes() {
    let api = api();
    let sync = Recorder::new().answering(StatusCode::OK, &serde_json::json!(voucher_json()));
    let asynchronous =
        Recorder::new().answering(StatusCode::OK, &serde_json::json!(voucher_json()));

    let from_sync = api.get_voucher(5).unwrap().send(&sync).unwrap();
    let from_async = block_on(api.get_voucher(5).unwrap().send_async(&asynchronous)).unwrap();

    assert_eq!(from_sync, from_async);
    assert_eq!(from_sync.total, "12.50".parse::<Money>().unwrap());
    assert_eq!(
        render(&sync.take()[0]),
        render(&asynchronous.take()[0]),
        "the two flavours share one request builder"
    );
}

/// A value that came back from the API has to be printable, and for a type the
/// generator wrote what comes out is what went in: the document states the
/// rule, the emitted `FromStr` enforces it, and `Display` hands the same bytes
/// back. typify writes neither of those last two, so both are this crate's to
/// prove.
#[test]
fn a_generated_newtype_prints_what_it_was_parsed_from() {
    let currency: Currency = "EUR".parse().expect("a currency code");
    assert_eq!(format!("{currency}"), "EUR");
    assert_eq!(
        format!("{currency}").parse::<Currency>().ok(),
        Some(currency.clone())
    );

    // The rule the document states is the rule the type carries.
    assert!("eur".parse::<Currency>().is_err());
    assert!("EURO".parse::<Currency>().is_err());
}

/// The other route, on the field next door. `Money` is a type this adoption
/// owns, substituted in by `Settings::replace`, so it is free to print for a
/// person — and the two directions stop being inverses. `tests/money.rs` is
/// where its reading half is held to the document; this is what a caller sees.
#[test]
fn a_replaced_type_prints_for_a_person_and_sends_what_the_document_accepts() {
    let voucher = voucher(VoucherStatus::Open);
    assert_eq!(
        format!("{} {}", voucher.total, voucher.currency),
        "12,50 EUR"
    );
    assert_eq!(voucher.total.wire(), "12.50");
    assert_eq!(voucher.total.minor_units(), 1250);
    assert!(
        voucher.total.to_string().parse::<Money>().is_err(),
        "printing an amount and reading one are different jobs"
    );
}

#[test]
fn a_non_success_status_keeps_the_body_and_never_deserialises_the_success_type() {
    let api = api();
    let client = Recorder::new().answering(
        StatusCode::NOT_FOUND,
        &serde_json::json!({"error": "no such voucher"}),
    );
    let error = api.get_voucher(404).unwrap().send(&client).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("404"), "{message}");
    assert!(message.contains("no such voucher"), "{message}");
}

#[test]
fn a_response_the_document_does_not_promise_a_body_for_needs_no_special_case() {
    let api = api();
    let client = Recorder::new().answering(StatusCode::CREATED, &serde_json::json!(null));
    let created = api
        .upload_document(b"x".to_vec())
        .unwrap()
        .send(&client)
        .unwrap();
    assert_eq!(created, api::NoContent);
}

/// The tripwire in prose: a posting is derived from every field of a voucher,
/// and the derivation is a pure function a test reaches with literals.
#[test]
fn a_posting_is_derived_from_the_whole_voucher() {
    assert_eq!(
        Posting::of(&voucher(VoucherStatus::Open)),
        Posting {
            reference: "AB-7".to_owned(),
            amount: "12.50".parse().unwrap(),
            currency: "EUR".parse().expect("a currency code"),
            booked: true,
        }
    );
}

fn voucher_json() -> serde_json::Value {
    serde_json::json!({
        "id": 5,
        "total": "12.50",
        "currency": "EUR",
        "status": "open",
        "internal_ref": "AB-7",
    })
}

/// A runtime in six lines. The recorder never yields, so the first poll is
/// ready — enough to prove the async path builds and sends the same request,
/// without taking tokio as a dependency of this crate's tests.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}
