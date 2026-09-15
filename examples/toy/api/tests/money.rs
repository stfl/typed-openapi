//! The document says what an amount looks like; [`Money`] says what one is.
//!
//! Those are two statements of one rule in two places, and a hand-owned type
//! borrows nothing: `money` does not depend on `typed-openapi`, and the `Money`
//! schema's `pattern` reaches `--total` without passing anywhere near
//! `Money::from_str`. So they can disagree — the vendor loosens the pattern, or
//! someone edits the parser — and nothing would say so.
//!
//! This is what says so. It reads the rule off the document this crate embeds,
//! the same one a command line runs, and holds the type to it value for value
//! over every edge the pattern has. A vendor revision that widens the rule
//! fails here instead of quietly admitting values `Money` refuses.
//!
//! The `format: money` tag beside that rule is the other half of the pair, and
//! it is what the two sides of the adoption agree on: the bless step reads it
//! to give `Voucher.total` this type, and the reduced model carries it so the
//! command line can name the same field as an amount.
//!
//! [`Money`]: api::Money

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use api::{DOCUMENT, Money, MoneyError};
use typed_openapi::{Body, Carrier, Document, Scalar};

/// Every edge the `Money` schema's `pattern` has, and whether it admits it.
///
/// Both columns are checked: the left against the document, so a vendor who
/// moves the rule is named here, and then the type against the document, so
/// the two cannot drift apart in either direction.
const VALUES: &[(&str, bool)] = &[
    // What the pattern is for.
    ("12.50", true),  // two decimals — the document's own spelling
    ("12.5", true),   // one decimal
    ("12", true),     // none at all
    ("0", true),      // and zero of them
    ("-3.07", true),  // a sign
    ("-0.5", true),   // a sign and one decimal
    ("007.10", true), // leading zeros are digits like any other
    ("99999999999999999999999999.99", true), // more digits than any fixed-width integer holds
    // What it is not for.
    ("12.505", false), // three decimals
    ("12.", false),    // a point with nothing after it
    (".5", false),     // nothing before it
    ("", false),       // nothing at all
    ("12,50", false),  // a comma — how a person writes it, not the wire
    (" 12.50", false), // a leading space
    ("12.50 ", false), // a trailing one
    ("+12.50", false), // an explicit plus
    ("-", false),      // a sign and no number
    ("1e3", false),    // an exponent
    ("12.5x", false),  // a suffix
    ("١٢.٥٠", false),  // digits, and not the ones ASCII means
];

/// The rule the command line runs on `--total`, read out of the document this
/// crate embeds.
///
/// Off the reduced model rather than off the YAML, because the model is what a
/// shipped binary carries: this is the very `Scalar` that refuses a `--total`,
/// with the `$ref` to `Money` already followed.
fn rule() -> Scalar {
    let document = Document::load(DOCUMENT, &[]).expect("the embedded document");
    let operation = document.get("updateVoucher").expect("updateVoucher");
    let Body::JsonFields(fields) = operation.body() else {
        panic!("updateVoucher takes a flat JSON body");
    };
    fields
        .iter()
        .find(|field| field.name() == "total")
        .expect("`total` is a field of the body")
        .scalar()
        .clone()
}

fn verdict(accepted: bool) -> &'static str {
    if accepted { "accepts" } else { "refuses" }
}

#[test]
fn money_reads_exactly_what_the_documents_pattern_admits() {
    let rule = rule();
    // A rule that says nothing would make every row below pass on both sides.
    assert!(
        rule.note().is_some_and(|note| note.contains("matches")),
        "`--total` carries no `pattern`, so this test can no longer tell the \
         two apart: the document has stopped stating what an amount is"
    );

    for (raw, admitted) in VALUES {
        let by_document = rule.parse(raw).is_ok();
        assert_eq!(
            by_document,
            *admitted,
            "the document's rule has moved: it now {} `{raw}`",
            verdict(by_document)
        );
        assert_eq!(
            raw.parse::<Money>().is_ok(),
            by_document,
            "`{raw}`: the document {} it and `Money` {} it",
            verdict(by_document),
            verdict(raw.parse::<Money>().is_ok())
        );
    }

    // A refusal names the value and is the only failure an amount has, which is
    // what an adopter handling one writes against.
    assert_eq!(
        "12,50".parse::<Money>(),
        Err(MoneyError {
            raw: "12,50".to_owned()
        })
    );
}

/// The tag reaches the command line's side of the adoption too.
///
/// `format: money` is the half of the pair the document *can* state, and a
/// bless step reads it to give `Voucher.total` the type this adoption owns —
/// which `tests/typed.rs` compiles against and is therefore held to. The
/// reduced model carries the same tag, so the other side can be asked which of
/// an operation's values are amounts and answers `total`: the very field the
/// generated struct gave the type to, named without a list kept by hand and
/// without reading the document again.
///
/// What the tag means is still this adoption's business. The document says
/// which values are amounts; nothing here says what may be done with one.
#[test]
fn the_reduced_model_names_the_amounts_the_generated_type_stands_for() {
    let document = Document::load(DOCUMENT, &[]).expect("the embedded document");
    let amounts = |id: &str| -> Vec<String> {
        document
            .get(id)
            .unwrap_or_else(|| panic!("{id}"))
            .carrying("money")
            .map(|carrier| carrier.name().to_owned())
            .collect()
    };

    assert_eq!(amounts("updateVoucher"), ["total"]);
    assert_eq!(amounts("createVoucher"), ["total"]);
    // `currency` is the field next door and the other route entirely: a named
    // schema and the newtype the generator writes, with no tag for anything to
    // key on. A route that names no kind is named by nothing.
    assert!(!amounts("updateVoucher").contains(&"currency".to_owned()));
    // An operation that sends no amount says so, rather than answering with
    // everything it does send.
    assert!(
        amounts("getVoucher").is_empty(),
        "fetching a voucher sends no amount"
    );

    let op = document.get("updateVoucher").expect("updateVoucher");
    assert!(
        op.carrying("money")
            .all(|carrier| matches!(carrier, Carrier::Field(_))),
        "an amount reaches this operation as a property of its body"
    );
}

/// Why the count of cents is arbitrary precision rather than an `i64`.
///
/// The document's `pattern` admits an unbounded run of digits. Anything
/// narrower would refuse amounts the document allows, and this file would have
/// to record the gap instead of denying it — so there would be a value the
/// command line accepts and the type does not, on a type whose whole job is to
/// agree with the command line.
#[test]
fn no_amount_the_document_admits_is_too_large_for_the_type() {
    let rule = rule();
    // Past `i64`, past `u64`, and well past anything a fixed-width integer
    // reaches.
    for digits in [19_usize, 20, 40, 100] {
        let raw = format!("{}.99", "9".repeat(digits));
        assert!(
            rule.parse(&raw).is_ok(),
            "the document stopped admitting {digits} digits"
        );
        assert_eq!(
            raw.parse::<Money>().map(|amount| amount.wire()),
            Ok(raw.clone()),
            "`Money` refuses an amount of {digits} digits that the document admits"
        );
    }
}
