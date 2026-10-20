//! An error says its own layer, once.
//!
//! A bless step, the example's `main`, sevdesk-cli's `error::chain` and
//! anyhow's `{:#}` all print an error and then walk `source()` to print each
//! cause under it. An `#[error]` string that also prints its source makes every
//! such walk say the cause twice, so the rule is that a string describes the
//! layer that owns it and the chain prints the rest.
//!
//! Each case here is one chain, built from the input that produces it where one
//! is cheap and from the variant itself where the cause is a foreign error.
//! Every variant that carries a source has a case, so a string that starts to
//! print its source again is named by the variant that did it.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::PathBuf;

use typed_openapi::client::{BodyError, Error as ClientError};
use typed_openapi::generate::GenerateError;
use typed_openapi::overlay;
use typed_openapi::plan::PlanError;
use typed_openapi::request::ValueError;
use typed_openapi::scalar::ScalarError;
use typed_openapi::tree::{ArgError, DispatchError};
use typed_openapi::{Document, DocumentError, Invocation, LoadError, Values};

/// An OpenAPI document with `parameters` on its one operation and the server
/// `url`, so a case names only the part that is wrong.
fn document(url: &str, parameter: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: Errors, version: \"1.0\" }}\n\
         servers: [{{ url: \"{url}\" }}]\n\
         paths:\n\
         \x20 /things:\n\
         \x20   get:\n\
         \x20     operationId: listThings\n\
         \x20     parameters:\n\
         \x20       - {parameter}\n\
         \x20     responses: {{ \"200\": {{ description: OK }} }}\n"
    )
}

const URL: &str = "http://localhost";
const SOUND: &str = "{ name: n, in: query, schema: { type: integer } }";
const UNRESOLVABLE: &str = "{ name: n, in: query, schema: { $ref: '#/components/schemas/Nope' } }";
const UNRUNNABLE: &str = "{ name: n, in: query, schema: { type: string, pattern: '(?P<n>' } }";

/// An Overlay that is a valid YAML mapping and not an Overlay 1.1 document, and
/// one that is, with an action that matches nothing.
const NOT_AN_OVERLAY: &str = "overlay: 1.0.0\n";
const NO_ACTIONS: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions: []\n";
const MATCHES_NOTHING: &str = "overlay: 1.1.0\n\
     info: { title: t, version: \"1\" }\n\
     actions:\n\
     \x20 - { target: \"$.components.nothing\", update: {} }\n";

fn http_error() -> http::Error {
    http::Request::builder()
        .uri("not a uri")
        .body(())
        .expect_err("a space is not part of a URI")
}

fn json_error() -> serde_json::Error {
    serde_json::from_str::<u8>("x").expect_err("`x` is not a number")
}

fn io_error() -> io::Error {
    io::Error::other("read timed out")
}

fn blob_error() -> DocumentError {
    Document::from_blob(&[0xFF]).expect_err("a stray byte is not a reduction")
}

fn load(document: &str, overlays: &[&str]) -> LoadError {
    Document::load(document, overlays).expect_err("the document is refused")
}

fn bad_value() -> ValueError {
    let doc = Document::load(&document(URL, SOUND), &[]).expect("a document");
    let op = doc.get("listThings").expect("the one operation");
    Invocation::new(op, Values::new().param("n", "s3cr3t-value"))
        .expect_err("`s3cr3t-value` is not an integer")
}

type Case = (&'static str, Box<dyn Error>);

fn case(name: &'static str, error: impl Error + 'static) -> Case {
    (name, Box::new(error))
}

fn overlay_error(overlay: &str) -> overlay::OverlayError {
    overlay::apply(serde_json::json!({}), overlay).expect_err("the Overlay is refused")
}

/// Reading a document and its Overlays.
fn reading() -> Vec<Case> {
    vec![
        case(
            "OverlayError::Document",
            overlay::parse(": [").expect_err("not YAML"),
        ),
        case("OverlayError::Syntax", overlay_error(NOT_AN_OVERLAY)),
        case("OverlayError::Invalid", overlay_error(NO_ACTIONS)),
        case("OverlayError::Apply", overlay_error(MATCHES_NOTHING)),
        case("DocumentError::Blob", blob_error()),
        case(
            "LoadError::Overlay",
            load("openapi: 3.0.3", &[MATCHES_NOTHING]),
        ),
        case("LoadError::Shape", load("{}", &[])),
        case(
            "LoadError::ServerUrl",
            load(&document("not a url", SOUND), &[]),
        ),
        case(
            "LoadError::Unresolvable",
            load(&document(URL, UNRESOLVABLE), &[]),
        ),
        case(
            "LoadError::Unrunnable",
            load(&document(URL, UNRUNNABLE), &[]),
        ),
    ]
}

/// Turning values and arguments into a request.
fn requesting() -> Vec<Case> {
    vec![
        case("ValueError::BadValue", bad_value()),
        case("PlanError::Request", PlanError::Request(http_error())),
        case("PlanError::Value", PlanError::Value(bad_value())),
        case(
            "client::BodyError",
            BodyError {
                op: "createThing".to_owned(),
                path: ".".to_owned(),
                source: json_error(),
            },
        ),
        case(
            "ArgError::ReadBody",
            ArgError::ReadBody {
                path: PathBuf::from("body.json"),
                source: io::Error::from(io::ErrorKind::NotFound),
            },
        ),
        case(
            "ArgError::ParseBody",
            ArgError::ParseBody {
                path: PathBuf::from("body.json"),
                source: json_error(),
            },
        ),
    ]
}

/// Sending one, from a command line and from a typed call.
fn calling() -> Vec<Case> {
    vec![
        case(
            "DispatchError::Transport",
            DispatchError::Transport(Box::new(io_error())),
        ),
        case(
            "DispatchError::Arg",
            DispatchError::Arg(ArgError::ParseBody {
                path: PathBuf::from("body.json"),
                source: json_error(),
            }),
        ),
        case(
            "DispatchError::Plan",
            DispatchError::Plan(PlanError::Request(http_error())),
        ),
        case(
            "client::Error::Document",
            ClientError::Document(blob_error()),
        ),
        case("client::Error::Request", ClientError::Request(http_error())),
        case(
            "client::Error::Encode",
            ClientError::Encode(
                serde_json::to_vec(&BTreeMap::from([((1, 2), 3)]))
                    .expect_err("a JSON object key is a string"),
            ),
        ),
        case(
            "client::Error::Transport",
            ClientError::Transport(Box::new(io_error())),
        ),
        case(
            "client::Error::Decode",
            ClientError::Decode {
                source: json_error(),
                raw: "x".to_owned(),
            },
        ),
        case("client::Error::Values", ClientError::Values(bad_value())),
    ]
}

/// A bless step reading its inputs and writing its outputs.
fn blessing() -> Vec<Case> {
    let path = || PathBuf::from("spec/cli.yaml");
    vec![
        case(
            "GenerateError::Read",
            GenerateError::Read {
                path: path(),
                source: io_error(),
            },
        ),
        case(
            "GenerateError::Write",
            GenerateError::Write {
                path: path(),
                source: io_error(),
            },
        ),
        case(
            "GenerateError::Overlay",
            GenerateError::Overlay {
                path: path(),
                source: overlay_error(MATCHES_NOTHING),
            },
        ),
        case(
            "GenerateError::Yaml",
            GenerateError::Yaml(
                serde_yaml_ng::from_str::<u8>("[").expect_err("an unclosed sequence"),
            ),
        ),
        case(
            "GenerateError::Unusable",
            GenerateError::Unusable(load("{}", &[])),
        ),
        case(
            "GenerateError::NotOpenApi",
            GenerateError::NotOpenApi(json_error()),
        ),
        case(
            "GenerateError::RustfmtSpawn",
            GenerateError::RustfmtSpawn {
                path: path(),
                source: io_error(),
            },
        ),
    ]
}

/// A bless step turning the reduction into Rust.
fn emitting() -> Vec<Case> {
    vec![
        case(
            "GenerateError::Schema",
            GenerateError::Schema {
                name: "Voucher".to_owned(),
                source: json_error(),
            },
        ),
        case(
            "GenerateError::Typify",
            GenerateError::Typify(typify::Error::InvalidTypeId),
        ),
        case(
            "GenerateError::Operation",
            GenerateError::Operation {
                op: "listThings".to_owned(),
                source: Box::new(GenerateError::NoType {
                    schema: "Nope".to_owned(),
                }),
            },
        ),
        case(
            "GenerateError::NotRust",
            GenerateError::NotRust {
                file: "ops.rs",
                source: syn::parse_str::<syn::File>("fn")
                    .map(drop)
                    .expect_err("a stump of a function"),
            },
        ),
        case("GenerateError::Blob", GenerateError::Blob(blob_error())),
    ]
}

/// The message of `error` and of every cause under it, outermost first.
fn messages(error: &(dyn Error + 'static)) -> Vec<String> {
    let mut messages = vec![error.to_string()];
    let mut cause = error.source();
    while let Some(next) = cause {
        messages.push(next.to_string());
        cause = next.source();
    }
    messages
}

/// What is wrong with `error`'s chain, one sentence per layer that prints the
/// layer under it.
fn repeated(name: &str, error: &(dyn Error + 'static)) -> Vec<String> {
    let messages = messages(error);
    if messages.len() < 2 {
        return vec![format!(
            "{name}: the chain is {messages:?}, so there is no cause for the layer to leave out"
        )];
    }
    messages
        .iter()
        .zip(messages.iter().skip(1))
        .filter(|(outer, cause)| cause.is_empty() || outer.contains(cause.as_str()))
        .map(|(outer, cause)| format!("{name}: {outer:?} prints the cause {cause:?}"))
        .collect()
}

/// Every case at once, so a failing run names each variant that repeats its
/// source and not only the first.
fn assert_say_their_own_layer_once(cases: impl IntoIterator<Item = Case>) {
    let repeated: Vec<String> = cases
        .into_iter()
        .flat_map(|(name, error)| repeated(name, error.as_ref()))
        .collect();
    assert!(repeated.is_empty(), "{repeated:#?}");
}

#[test]
fn every_error_enum_of_the_library_prints_a_cause_once_down_one_chain() {
    assert_say_their_own_layer_once(reading().into_iter().chain(requesting()).chain(calling()));
}

#[test]
fn every_failure_of_a_bless_step_prints_a_cause_once_down_one_chain() {
    assert_say_their_own_layer_once(blessing().into_iter().chain(emitting()));
}

/// The variant is what a caller matches and the message is what a person reads,
/// so the one case a chain test cannot tell apart from a lucky message is held
/// to both: a refused value names the operation and the parameter in its own
/// layer and leaves the value, which the user typed, to the cause.
#[test]
fn a_refused_value_names_the_parameter_in_its_layer_and_the_value_in_its_cause() {
    let error = bad_value();

    assert!(
        matches!(&error, ValueError::BadValue { op, name, source: ScalarError::Kind { .. } }
            if op == "listThings" && name == "n"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "listThings: `n` was given a value the document refuses"
    );
    assert_eq!(
        error.source().map(ToString::to_string).as_deref(),
        Some("`s3cr3t-value` is not an integer")
    );
}
