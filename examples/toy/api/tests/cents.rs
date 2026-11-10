//! A bounded owned type stays away from flags, and accepts its response limit.

#![expect(clippy::expect_used, reason = "test fixtures must load")]

use api::{Cents, Contact, DOCUMENT, money};
use typed_openapi::scalar::Text;
use typed_openapi::{Body, Document, Scalar};

#[test]
fn no_operation_carries_cents_as_a_flag() {
    let document = Document::load(DOCUMENT, &[]).expect("the embedded document");
    for operation in &document {
        assert!(
            operation.carrying("cents").next().is_none(),
            "`{}` carries cents as a flag; the owned type is narrower than the document",
            operation.id()
        );
    }
    assert!(matches!(
        document.get("createContact").expect("createContact").body(),
        Body::JsonWhole { .. }
    ));
}

#[test]
fn the_wrapper_reads_and_serializes_the_owned_count() {
    for raw in ["0", "0012", "9223372036854775807"] {
        let owned = raw.parse::<money::Cents>().expect("an i64 count of cents");
        let wrapped = raw
            .parse::<Cents>()
            .expect("the same count through the schema");
        assert_eq!(wrapped.0, owned);
        assert_eq!(
            serde_json::to_string(&wrapped).expect("serialize"),
            serde_json::to_string(&owned).expect("serialize")
        );
        assert_eq!(
            serde_json::from_str::<Cents>(&serde_json::to_string(&wrapped).expect("serialize"))
                .expect("deserialize"),
            wrapped
        );
    }
    for raw in ["", "+1", "-1", "1.0", " 1", "1\n", "9223372036854775808"] {
        assert!(raw.parse::<Cents>().is_err(), "{raw}");
    }
    let count = "0012".parse::<Cents>().expect("leading zeroes are digits");
    assert_eq!(count.to_string(), "12");
    assert_eq!(serde_json::to_string(&count).expect("serialize"), "\"12\"");
    assert!(serde_json::from_str::<Cents>("12").is_err());
}

#[test]
fn a_response_allowed_by_the_document_can_exceed_the_owned_count() {
    let document = typed_openapi::overlay::parse(DOCUMENT).expect("the embedded document");
    assert_eq!(
        document
            .pointer("/components/schemas/Cents/pattern")
            .and_then(serde_json::Value::as_str),
        Some("^[0-9]+$")
    );
    let rule = Scalar::Text(Text {
        pattern: document
            .pointer("/components/schemas/Cents/pattern")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        ..Text::default()
    });
    assert!(rule.parse("9223372036854775808").is_ok());
    let body = serde_json::json!({"name":"Ada", "address":{"street":"Main", "city":"Vienna"}, "creditLimit":"9223372036854775808"});
    assert!(serde_json::from_value::<Contact>(body).is_err());
    let contact: Contact = serde_json::from_value(serde_json::json!({"name":"Ada", "address":{"street":"Main", "city":"Vienna"}, "creditLimit":"12"})).expect("bounded response");
    assert_eq!(contact.credit_limit, Some("12".parse().expect("cents")));
    let contact: Contact = serde_json::from_value(
        serde_json::json!({"name":"Ada", "address":{"street":"Main", "city":"Vienna"}}),
    )
    .expect("creditLimit is optional");
    assert_eq!(contact.credit_limit, None);
}
