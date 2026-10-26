//! An optional body can be left out, and a required one cannot.
//!
//! Whether a body is required travels in the model, and both roads read it. A
//! required body that is missing is refused by `Invocation::new`, and an optional
//! body that nobody filled in is not in the request at all. The documents are the
//! input of the probes of SEV-249 behind F-18 (`A7/docs/optbody.yaml`, and
//! `A2/wire` `nobody` over the toy fixtures), in `tests/fixtures/optional.yaml`.
//! A case that refuses a body asserts the variant beside the words.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use clap::error::ErrorKind;
use serde_json::json;
use typed_openapi::model::{Body, Operation};
use typed_openapi::tree::{self, Asked, Outcome};
use typed_openapi::{
    Answers, Document, Invocation, JSON_BODY, Plan, PlanError, Recorder, ValueError, Values,
};

const OPTIONAL: &str = include_str!("fixtures/optional.yaml");
const TOY: &str = include_str!("fixtures/toy.yaml");
const CORRECTIONS: &str = include_str!("fixtures/corrections.yaml");
const CLI: &str = include_str!("fixtures/cli.yaml");

fn optional() -> Document {
    Document::load(OPTIONAL, &[]).expect("the fixture loads")
}

fn toy() -> Document {
    Document::load(TOY, &[CORRECTIONS, CLI]).expect("the toy fixtures load")
}

fn operation<'d>(doc: &'d Document, id: &str) -> &'d Operation {
    doc.get(id).expect("the operation")
}

/// The command line `args` makes of `id`, parsed. A refusal of clap is the
/// error, not an exit of this process.
fn parsed(doc: &Document, id: &str, args: &[&str]) -> Result<clap::ArgMatches, clap::Error> {
    let op = operation(doc, id);
    let mut line = vec!["optional", op.group().as_str(), op.command().as_str()];
    line.extend_from_slice(args);
    clap::Command::new("optional")
        .subcommand_required(true)
        .subcommands(tree::commands(doc))
        .try_get_matches_from(line)
}

/// The one request that the command line `args` sends for `id`, confirmed.
fn sent(doc: &Document, id: &str, args: &[&str]) -> http::Request<Vec<u8>> {
    let mut args = args.to_vec();
    args.push("--commit");
    let matches = parsed(doc, id, &args).expect("clap accepts the command line");
    let Asked::Run(selection) =
        tree::select(doc, &matches).expect("the command names an operation")
    else {
        panic!("this command line runs the operation");
    };
    let client = Recorder::new();
    let outcome = selection
        .send(&client, doc.base())
        .expect("the request is sent");
    assert!(matches!(outcome, Outcome::Sent(_)), "the commit sends it");
    let mut sent = client.take();
    assert_eq!(sent.len(), 1, "exactly one request went out");
    sent.pop().expect("the length is one")
}

/// The request that the typed road makes of `id` from `values`.
fn built(doc: &Document, id: &str, values: Values) -> http::Request<Vec<u8>> {
    Invocation::new(operation(doc, id), values)
        .expect("the values satisfy the operation")
        .request(doc.base())
        .expect("a request")
}

/// A request that carries a body has a `content-type`, and one that carries none
/// has neither.
fn carries_no_body(request: &http::Request<Vec<u8>>) -> bool {
    request.body().is_empty() && !request.headers().contains_key("content-type")
}

fn json_of(request: &http::Request<Vec<u8>>) -> serde_json::Value {
    serde_json::from_slice(request.body()).expect("the body is JSON")
}

/// A file under `CARGO_TARGET_TMPDIR`, named for this test binary's process so
/// that two runs do not meet in one file.
fn scratch(name: &str, text: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "typed-openapi-optional-{}-{name}",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("writing the scratch file");
    path
}

/// What a body says about itself: `Some` of whether the document requires it,
/// and `None` for the operation that has no body.
fn required(body: &Body) -> Option<bool> {
    match body {
        Body::None => None,
        Body::JsonFields { required, .. }
        | Body::JsonWhole { required, .. }
        | Body::Multipart { required, .. }
        | Body::Opaque { required, .. } => Some(*required),
    }
}

// ----------------------------------------------------------------- the model

/// Whether a body is required is a fact of the document, and it is in the model
/// for every kind of body, so that neither road has to ask the document again.
#[test]
fn whether_a_body_is_required_travels_in_the_model_for_every_kind() {
    let doc = optional();
    let toy = toy();
    let rows = [
        (&doc, "createReminder", Some(false)),
        (&doc, "createNote", Some(false)),
        (&doc, "createExport", Some(false)),
        (&doc, "uploadAttachment", Some(false)),
        (&doc, "uploadScan", Some(false)),
        (&doc, "createLabel", Some(true)),
        (&doc, "createTag", Some(true)),
        (&toy, "createVoucher", Some(true)),
        (&toy, "createContact", Some(true)),
        (&toy, "uploadDocument", Some(true)),
        (&toy, "uploadDocumentMultipart", Some(true)),
        (&toy, "listVouchers", None),
    ];
    for (doc, id, wanted) in rows {
        assert_eq!(required(operation(doc, id).body()), wanted, "{id}");
    }
}

/// A property says whether its own schema requires it, whatever the body says,
/// so a property of an optional body is still the one that a body must hold once
/// one is sent, and the body that is required does not make a property so.
#[test]
fn a_property_keeps_its_own_required_apart_from_the_body() {
    let doc = optional();
    let flags = |id: &str| -> Vec<(String, bool)> {
        operation(&doc, id)
            .body()
            .fields()
            .iter()
            .map(|field| (field.flag().to_owned(), field.required()))
            .collect()
    };

    // An optional body whose property is required: A7 `optbody.yaml`.
    assert_eq!(flags("createReminder"), [("note".to_owned(), true)]);
    // A required body in which no property is required.
    assert_eq!(
        flags("createLabel"),
        [("name".to_owned(), false), ("color".to_owned(), false)]
    );
    // A required body in which one is.
    assert_eq!(
        flags("createTag"),
        [("name".to_owned(), true), ("color".to_owned(), false)]
    );
    // An optional body in which none is.
    assert_eq!(
        flags("createNote"),
        [("text".to_owned(), false), ("pinned".to_owned(), false)]
    );
}

// ------------------------------------------------------------ the command line

/// `A7/docs/optbody.yaml`: `reminders create --invoice 7 --commit` sent `{}`,
/// which the generated type refuses for want of `note`. An operator who gives no
/// field and no file has given no body, and the request carries none.
#[test]
fn an_optional_flat_body_with_no_flag_sends_no_body() {
    let doc = optional();
    let request = sent(&doc, "createReminder", &["--invoice", "7"]);

    assert_eq!(request.method(), http::Method::POST);
    assert_eq!(
        request
            .uri()
            .path_and_query()
            .map(http::uri::PathAndQuery::as_str),
        Some("/reminders?invoice=7")
    );
    assert!(carries_no_body(&request), "{request:?}");
}

/// The same holds for an optional flat body in which every property is optional:
/// the body is left out and not sent as the empty object.
#[test]
fn an_optional_flat_body_of_optional_properties_sends_no_body_with_no_flag() {
    let doc = optional();

    assert!(carries_no_body(&sent(&doc, "createNote", &[])));
}

/// A flag, or a file, is a body, and a body goes out as it always did.
#[test]
fn an_optional_flat_body_is_sent_once_a_flag_or_a_file_gives_one() {
    let doc = optional();

    let by_flag = sent(&doc, "createReminder", &["--invoice", "7", "--note", "pay"]);
    assert_eq!(json_of(&by_flag), json!({ "note": "pay" }));
    assert_eq!(by_flag.headers()["content-type"], "application/json");

    let file = scratch("reminder.json", r#"{ "note": "pay up" }"#);
    let path = file.to_str().expect("the path is text");
    let by_file = sent(
        &doc,
        "createReminder",
        &["--invoice", "7", &format!("--{JSON_BODY}"), path],
    );
    std::fs::remove_file(&file).expect("removing the scratch file");
    assert_eq!(json_of(&by_file), json!({ "note": "pay up" }));

    let by_option = sent(&doc, "createNote", &["--pinned", "true"]);
    assert_eq!(json_of(&by_option), json!({ "pinned": true }));
}

/// An optional body that goes whole is left out the same way, and sent when its
/// file is given.
#[test]
fn an_optional_body_that_goes_whole_is_left_out_and_sent_from_its_file() {
    let doc = optional();
    assert!(matches!(
        operation(&doc, "createExport").body(),
        Body::JsonWhole { .. }
    ));

    assert!(carries_no_body(&sent(&doc, "createExport", &[])));

    let file = scratch("export.json", r#"{ "filter": { "year": 2026 } }"#);
    let path = file.to_str().expect("the path is text");
    let request = sent(&doc, "createExport", &[&format!("--{JSON_BODY}"), path]);
    std::fs::remove_file(&file).expect("removing the scratch file");
    assert_eq!(json_of(&request), json!({ "filter": { "year": 2026 } }));
}

/// An optional body sent verbatim and an optional multipart body are left out
/// with no `--raw-body` and no part.
#[test]
fn an_optional_raw_or_multipart_body_is_left_out() {
    let doc = optional();

    assert!(carries_no_body(&sent(&doc, "uploadAttachment", &[])));
    assert!(carries_no_body(&sent(&doc, "uploadScan", &[])));
}

/// A required flat body in which no property is required is still a body, so the
/// command line sends the empty object when nobody gave a field: `{}` is a body
/// the document admits, and a request without one is not.
#[test]
fn a_required_flat_body_with_no_required_property_goes_out_as_the_empty_object() {
    let doc = optional();
    let request = sent(&doc, "createLabel", &[]);

    assert_eq!(json_of(&request), json!({}));
    assert_eq!(request.headers()["content-type"], "application/json");
}

/// A property is demanded of the command line when the body is required and the
/// property is, and a file satisfies the demand. An optional body demands none:
/// what it asks for once there is a body is the check of the adopter's type.
#[test]
fn a_property_is_demanded_only_of_a_body_that_is_required() {
    let doc = optional();

    let refused = parsed(&doc, "createTag", &["--commit"]).expect_err("`--name` is demanded");
    assert_eq!(refused.kind(), ErrorKind::MissingRequiredArgument);

    let file = scratch("tag.json", r#"{ "name": "work" }"#);
    let path = file.to_str().expect("the path is text");
    let by_file = parsed(
        &doc,
        "createTag",
        &[&format!("--{JSON_BODY}"), path, "--commit"],
    );
    std::fs::remove_file(&file).expect("removing the scratch file");
    assert!(by_file.is_ok(), "a file stands for the property");

    let tag = sent(&doc, "createTag", &["--name", "work"]);
    assert_eq!(json_of(&tag), json!({ "name": "work" }));

    assert!(
        parsed(&doc, "createReminder", &["--invoice", "7"]).is_ok(),
        "an optional body demands no property"
    );
}

// ------------------------------------------------------------------ typed road

/// `A2/wire` `nobody` over the toy fixtures: `createVoucher` takes a required
/// flat body, and `Invocation::new` accepted values with none, so `Plan::build`
/// with the commit made a `POST` with no body.
#[test]
fn a_required_flat_body_that_is_missing_is_refused() {
    let doc = toy();
    let op = operation(&doc, "createVoucher");

    let refused = Invocation::new(op, Values::new()).expect_err("the body is required");
    assert!(
        matches!(&refused, ValueError::MissingBody { op } if op == "createVoucher"),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "createVoucher: a request body is required"
    );

    let planned = Plan::build(op, doc.base(), Values::new(), &Answers::new().commit());
    assert!(
        matches!(
            planned,
            Err(PlanError::Value(ValueError::MissingBody { .. }))
        ),
        "{planned:?}"
    );
}

/// The refusal does not depend on a property being required: a required flat
/// body in which none is, is the case a model that folded the body's `required`
/// into its properties could not tell from an optional body.
#[test]
fn a_required_flat_body_with_no_required_property_is_refused_when_missing() {
    let doc = optional();

    for id in ["createLabel", "createTag"] {
        let refused =
            Invocation::new(operation(&doc, id), Values::new()).expect_err("the body is required");
        assert!(
            matches!(&refused, ValueError::MissingBody { op } if op == id),
            "{id}: {refused:?}"
        );
    }

    let sent = built(&doc, "createLabel", Values::new().json(json!({})));
    assert_eq!(json_of(&sent), json!({}));
}

/// An optional body of every kind is left out of the typed call the way it is
/// left out of the command line: no body, and no `content-type` to describe one.
#[test]
fn an_optional_body_of_every_kind_may_be_left_out_of_a_typed_call() {
    let doc = optional();
    let cases = [
        ("createReminder", Values::new().param("invoice", 7)),
        ("createNote", Values::new()),
        ("createExport", Values::new()),
        ("uploadAttachment", Values::new()),
        ("uploadScan", Values::new()),
    ];
    for (id, values) in cases {
        let request = built(&doc, id, values);
        assert!(carries_no_body(&request), "{id}: {request:?}");
    }

    let given = built(
        &doc,
        "createReminder",
        Values::new()
            .param("invoice", 7)
            .json(json!({ "note": "pay" })),
    );
    assert_eq!(json_of(&given), json!({ "note": "pay" }));
}
