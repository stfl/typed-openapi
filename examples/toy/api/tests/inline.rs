//! A body the document describes where it uses it, held to what it describes.
//!
//! `createLedgerEntry` states its request body inline rather than under a name
//! in `components.schemas` — an ordinary thing for a document to do, and no
//! statement at all about how much the document describes. This one states a
//! `required` list and points a property at a named schema carrying a
//! `pattern`, so there is a rule to run and something to run it on.
//!
//! What makes that worth a test of its own is how easily it reads as the
//! opposite. A body check that accepts anything is indistinguishable from a
//! document with no opinion, and an adopter who meets one writes the second
//! sentence down and stops looking. So this file does not ask whether the type
//! exists. It reads the rule off the document this crate embeds — the same
//! reduction a command line runs — and holds the generated type to it value for
//! value, in both directions, on both roads to the request: the type a Rust
//! caller parses into, and [`OperationId::check_body`], which is what a
//! `--json-body` file goes through in
//! [`cli/src/raw.rs`](../../cli/src/raw.rs).
//!
//! A vendor revision that widens the rule, or a bless step that stops reaching
//! it, fails here rather than quietly admitting values the document forbids.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use api::{CreateLedgerEntryBody, DOCUMENT, LedgerAccount, Memo, OperationId, money};
use typed_openapi::{Body, Document, Scalar};

/// Every edge the `LedgerAccount` schema's `pattern` has, and whether the
/// document admits it.
///
/// One column, checked twice: against the document, so a vendor who moves the
/// rule is named here, and then against the generated type, so the two cannot
/// drift apart in either direction.
const ACCOUNTS: &[(&str, bool)] = &[
    // What the pattern is for: four digits, and the vendor means all of them.
    ("4400", true),
    ("0000", true),
    ("9999", true),
    // What it is not for.
    ("440", false),   // three
    ("44000", false), // five
    ("44a0", false),  // a letter among them
    ("", false),      // nothing at all
    (" 4400", false), // a leading space
    ("4400 ", false), // a trailing one
    ("-4400", false), // a sign
    ("4.400", false), // a separator a person would write
    ("٤٤٠٠", false),  // digits, and not the ones ASCII means
];

/// The rule the command line runs on `--account`, read off the reduced model
/// this crate embeds.
///
/// Off the model rather than off the YAML because the model is what a shipped
/// binary carries: this is the very [`Scalar`] that refuses an `--account`,
/// with the `$ref` to `LedgerAccount` already followed.
fn rule() -> Scalar {
    field("account").0
}

/// One field of the body `createLedgerEntry` states inline, as the reduction
/// carries it: the rule a flag runs, and whether the flag is demanded.
fn field(name: &str) -> (Scalar, bool) {
    let document = Document::load(DOCUMENT, &[]).expect("the embedded document");
    let operation = document
        .get("createLedgerEntry")
        .expect("createLedgerEntry is in the document this crate embeds");
    let Body::JsonFields(fields) = operation.body() else {
        panic!("createLedgerEntry takes a flat JSON body");
    };
    let field = fields
        .iter()
        .find(|field| field.name() == name)
        .unwrap_or_else(|| panic!("`{name}` is a field of the body"));
    (field.scalar().clone(), field.required())
}

/// A body carrying `account`, and the two other properties filled in well.
fn body(account: &str) -> serde_json::Value {
    serde_json::json!({ "account": account, "amount": "12.50", "memo": "PO-1234" })
}

fn verdict(accepted: bool) -> &'static str {
    if accepted { "accepts" } else { "refuses" }
}

/// The claim the whole change rests on: a value the document forbids does not
/// reach a request.
///
/// Three readings of one rule have to agree — the document's own, the generated
/// type's, and the check a `--json-body` file goes through — because an adopter
/// reaches the API by all three and a rule that holds on one road is a rule
/// that does not hold.
#[test]
fn a_value_the_documents_pattern_forbids_does_not_deserialise_into_the_body() {
    let rule = rule();
    // A rule that says nothing would make every row below pass on all three
    // sides, so the test would hold nothing.
    assert!(
        rule.note().is_some_and(|note| note.contains("matches")),
        "`--account` carries no `pattern`, so this test can no longer tell an \
         admitted value from a forbidden one: the document has stopped saying \
         what a ledger account is"
    );

    for (raw, admitted) in ACCOUNTS {
        let by_document = rule.parse(raw).is_ok();
        assert_eq!(
            by_document,
            *admitted,
            "the document's rule has moved: it now {} `{raw}`",
            verdict(by_document)
        );

        let by_type = serde_json::from_value::<CreateLedgerEntryBody>(body(raw)).is_ok();
        assert_eq!(
            by_type,
            by_document,
            "`{raw}`: the document {} it and the generated body type {} it",
            verdict(by_document),
            verdict(by_type)
        );

        let by_check = OperationId::CreateLedgerEntry
            .check_body(&body(raw))
            .is_ok();
        assert_eq!(
            by_check,
            by_document,
            "`{raw}`: the document {} it and the check a `--json-body` file goes \
             through {} it",
            verdict(by_document),
            verdict(by_check)
        );
    }
}

/// The other direction, which the one above cannot give: a body the document
/// admits arrives as the values that were sent, and goes back out as the bytes
/// they came in as.
///
/// A type that refused everything would satisfy every refusal above. This is
/// what says it refuses the right things rather than all of them.
#[test]
fn a_body_the_document_admits_round_trips_through_the_generated_type() {
    let sent = body("4400");
    let parsed: CreateLedgerEntryBody =
        serde_json::from_value(sent.clone()).expect("the document admits this body");

    assert_eq!(
        parsed.account,
        "4400".parse::<LedgerAccount>().expect("four digits")
    );
    assert_eq!(
        parsed.amount,
        "12.50".parse::<money::Money>().expect("an amount")
    );
    assert_eq!(
        parsed.memo,
        Some("PO-1234".parse::<Memo>().expect("a memo"))
    );

    assert_eq!(
        serde_json::to_value(&parsed).expect("a body serialises"),
        sent,
        "the body does not go back out as the bytes it came in as"
    );
}

/// A `required` list is part of what the document says about a body, and it
/// reaches the type with everything else. `memo` beside it is the control: a
/// property the document does not require is one the type does not demand.
#[test]
fn the_required_list_on_a_body_stated_inline_reaches_the_generated_type() {
    let missing = serde_json::json!({ "account": "4400" });
    let refused = serde_json::from_value::<CreateLedgerEntryBody>(missing)
        .expect_err("the document requires `amount`");
    assert!(
        refused.to_string().contains("missing field `amount`"),
        "the refusal does not name the property the document requires: {refused}"
    );

    let without_memo = serde_json::json!({ "account": "4400", "amount": "12.50" });
    let parsed: CreateLedgerEntryBody =
        serde_json::from_value(without_memo).expect("`memo` is not required");
    assert_eq!(parsed.memo, None);

    assert!(
        field("account").1 && field("amount").1,
        "a property the document requires is optional on the command line"
    );
    assert!(
        !field("memo").1,
        "a property the document does not require is demanded on the command line"
    );
}

/// What the amount on this body is read by, and what it is not.
///
/// The vendor tags `amount` with `format: money` and states no rule for it —
/// the same thing they do to `Voucher.total`, which `spec/corrections.yaml`
/// repairs by pointing it at a `Money` schema that carries a `pattern`. Nothing
/// repairs this one, so the two roads to the request do not run the same rule:
/// the generated type hands the reading to [`money::Money`], which refuses a
/// malformed amount, while `--amount` carries no rule to refuse one with.
///
/// The tag still travels, so the reduction can be asked which of this
/// operation's values are amounts and answers `amount` — the very property the
/// generated type gave the type to.
///
/// This test goes red if an Overlay action ever points `amount` at `Money`,
/// which is the repair. That is the point: the gap is stated here rather than
/// left for someone to find with a bad value in a request.
#[test]
fn an_amount_stated_inline_is_read_by_the_type_and_not_by_the_command_line() {
    let malformed = serde_json::json!({ "account": "4400", "amount": "12,50" });
    assert!(
        serde_json::from_value::<CreateLedgerEntryBody>(malformed).is_err(),
        "the amount on a body stated inline is not held to anything"
    );
    assert!(
        "12,50".parse::<money::Money>().is_err(),
        "the type the amount is read by admits a comma"
    );

    let (amount, _) = field("amount");
    assert!(
        amount.note().is_none(),
        "`--amount` carries a rule, so the document has been repaired and this \
         file's account of the gap is out of date"
    );

    let document = Document::load(DOCUMENT, &[]).expect("the embedded document");
    let named: Vec<String> = document
        .get("createLedgerEntry")
        .expect("createLedgerEntry")
        .carrying("money")
        .map(|carrier| carrier.name().to_owned())
        .collect();
    assert_eq!(named, ["amount"]);
}
