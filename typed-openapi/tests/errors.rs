//! An error says its own layer, once.
//!
//! A bless step, the example's `main`, sevdesk-cli's `error::chain` and
//! anyhow's `{:#}` all print an error and then walk `source()` to print each
//! cause under it. An `#[error]` string that also prints its source makes every
//! such walk say the cause twice, so the rule is that a string describes the
//! layer that owns it and the chain prints the rest.
//!
//! Each case here is one chain, built from the input that produces it where one
//! is cheap and from the variant itself where the cause is a foreign error. A
//! case built from an input is also held to the variant it is named for, so an
//! input that starts to give another variant fails under its name.
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

use clap::{Arg, Command, value_parser};
use typed_openapi::client::{BodyError, Error as ClientError};
use typed_openapi::generate::GenerateError;
use typed_openapi::overlay::{self, OverlayError};
use typed_openapi::plan::PlanError;
use typed_openapi::request::{RequestError, ValueError};
use typed_openapi::scalar::ScalarError;
use typed_openapi::tree::{self, ArgError, DispatchError};
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

/// An OpenAPI document with `marker` on its one operation, which is how a case
/// puts a word the command line cannot spell where the reduction reads it.
fn marked(marker: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: Errors, version: \"1.0\" }}\n\
         servers: [{{ url: \"{URL}\" }}]\n\
         paths:\n\
         \x20 /things:\n\
         \x20   post:\n\
         \x20     operationId: createThing\n\
         \x20     {marker}\n\
         \x20     responses: {{ \"200\": {{ description: OK }} }}\n"
    )
}

const URL: &str = "http://localhost";
const SOUND: &str = "{ name: n, in: query, schema: { type: integer } }";
const UNRESOLVABLE: &str = "{ name: n, in: query, schema: { $ref: '#/components/schemas/Nope' } }";
const UNRUNNABLE: &str = "{ name: n, in: query, schema: { type: string, pattern: '(?P<n>' } }";
/// A schema that no flag is made of, which holds an escape of the Unicode flag.
const UNICODE_SCHEMA: &str =
    "{ name: n, in: cookie, schema: { type: string, pattern: '^\\p{L}+$' } }";

/// Text that is not YAML.
const NOT_YAML: &str = "actions: [\n";
/// A valid YAML mapping that is not an Overlay 1.1 document.
const NOT_AN_OVERLAY: &str = "overlay: 1.0.0\n";
/// An Overlay 1.1 document whose list of actions is empty, which the Overlay
/// specification refuses.
const NO_ACTIONS: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions: []\n";
/// An Overlay 1.1 document with an action whose target matches nothing.
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

/// A blob of this build's layout that ends before its encoding does, which is
/// the `postcard` failure behind `DocumentError::Blob`.
fn blob_error() -> DocumentError {
    let doc = Document::load(&document(URL, SOUND), &[]).expect("a document");
    let mut blob = doc.to_blob().expect("the reduction encodes");
    blob.pop();
    Document::from_blob(&blob).expect_err("a blob with a byte missing is damaged")
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

/// A list parameter that takes each value once, given a value twice.
fn bad_list() -> ValueError {
    let list = "{ name: n, in: query, schema: { type: array, uniqueItems: true, items: { type: integer } } }";
    let doc = Document::load(&document(URL, list), &[]).expect("a document");
    let op = doc.get("listThings").expect("the one operation");
    Invocation::new(op, Values::new().each("n", [7, 7])).expect_err("`7` is given twice")
}

/// A verb of the adopter's own that declares `--n` with a parser that stores an
/// `i64`, where `values` reads a `String` whatever type the document gives `n`.
fn flag_type() -> ArgError {
    let doc = Document::load(&document(URL, SOUND), &[]).expect("a document");
    let op = doc.get("listThings").expect("the one operation");
    let matches = Command::new("list")
        .arg(Arg::new("n").long("n").value_parser(value_parser!(i64)))
        .get_matches_from(["list", "--n", "5"]);
    tree::values(op, &matches).expect_err("`--n` holds an integer")
}

/// A verb of the adopter's own that reads a field's flag with no rule of the
/// document's.
fn field_value() -> ArgError {
    let paths = "\x20 /things:\n\
                 \x20   post:\n\
                 \x20     operationId: createThing\n\
                 \x20     requestBody:\n\
                 \x20       content:\n\
                 \x20         application/json:\n\
                 \x20           schema:\n\
                 \x20             type: object\n\
                 \x20             properties:\n\
                 \x20               code: { type: string, maxLength: 2 }\n\
                 \x20     responses: { \"200\": { description: OK } }\n";
    let text = format!(
        "openapi: 3.0.3\ninfo: {{ title: Errors, version: \"1.0\" }}\nservers: [{{ url: \"{URL}\" }}]\npaths:\n{paths}"
    );
    let doc = Document::load(&text, &[]).expect("a document");
    let op = doc.get("createThing").expect("the one operation");
    let matches = Command::new("create")
        .arg(Arg::new("code").long("code"))
        .get_matches_from(["create", "--code", "abc"]);
    tree::values(op, &matches).expect_err("`abc` is longer than two characters")
}

/// One chain to check, under the name of the variant at its head.
struct Case {
    name: &'static str,
    error: Box<dyn Error>,
    /// Whether `error` is the variant `name` says it is.
    is_named: bool,
}

/// A case whose variant is written out, so it cannot be another one.
fn case(name: &'static str, error: impl Error + 'static) -> Case {
    Case {
        name,
        error: Box::new(error),
        is_named: true,
    }
}

/// A case that running the library on an input produced. `is` holds the result
/// to the variant `name` says, so an input that starts to give another variant
/// fails under this name instead of passing under it.
fn from_input<E: Error + 'static>(
    name: &'static str,
    error: E,
    is: impl FnOnce(&E) -> bool,
) -> Case {
    Case {
        name,
        is_named: is(&error),
        error: Box::new(error),
    }
}

fn overlay_error(overlay: &str) -> OverlayError {
    overlay::apply(serde_json::json!({}), overlay).expect_err("the Overlay is refused")
}

/// Reading a document and its Overlays, and decoding the blob.
fn reading() -> Vec<Case> {
    vec![
        from_input(
            "OverlayError::Document",
            overlay::parse(": [").expect_err("not YAML"),
            |e| matches!(e, OverlayError::Document(_)),
        ),
        from_input("OverlayError::Syntax", overlay_error(NOT_YAML), |e| {
            matches!(e, OverlayError::Syntax(_))
        }),
        from_input("OverlayError::Shape", overlay_error(NOT_AN_OVERLAY), |e| {
            matches!(e, OverlayError::Shape(_))
        }),
        from_input("OverlayError::Invalid", overlay_error(NO_ACTIONS), |e| {
            matches!(e, OverlayError::Invalid(_))
        }),
        from_input("OverlayError::Apply", overlay_error(MATCHES_NOTHING), |e| {
            matches!(e, OverlayError::Apply(_))
        }),
        from_input("DocumentError::Blob", blob_error(), |e| {
            matches!(e, DocumentError::Blob(_))
        }),
        // No reduction a load writes fails to encode, so this cause is built
        // by hand: the variant that holds it is what the case is about.
        case(
            "DocumentError::Encode",
            DocumentError::Encode(postcard::Error::SerializeBufferFull),
        ),
    ]
}

/// Reducing a document to operations.
fn loading() -> Vec<Case> {
    vec![
        from_input("LoadError::Overlay", load(": [", &[]), |e| {
            matches!(e, LoadError::Overlay(_))
        }),
        from_input(
            "LoadError::Layer",
            load("openapi: 3.0.3", &[MATCHES_NOTHING]),
            |e| matches!(e, LoadError::Layer { .. }),
        ),
        from_input("LoadError::Shape", load("{}", &[]), |e| {
            matches!(e, LoadError::Shape { .. })
        }),
        from_input(
            "LoadError::Name",
            load(&marked("x-cli-command: '___'"), &[]),
            |e| matches!(e, LoadError::Name { .. }),
        ),
        from_input(
            "LoadError::GateName",
            load(&marked("x-cli-gates: ['???']"), &[]),
            |e| matches!(e, LoadError::GateName { .. }),
        ),
        from_input(
            "LoadError::ParameterRef",
            load(
                &document(URL, "{ $ref: '#/components/parameters/Nope' }"),
                &[],
            ),
            |e| matches!(e, LoadError::ParameterRef { .. }),
        ),
        from_input(
            "LoadError::ServerUrl",
            load(&document("not a url", SOUND), &[]),
            |e| matches!(e, LoadError::ServerUrl { .. }),
        ),
        from_input(
            "LoadError::ServerBase",
            load(&document("http://u:p@localhost", SOUND), &[]),
            |e| matches!(e, LoadError::ServerBase { .. }),
        ),
        from_input(
            "LoadError::Unresolvable",
            load(&document(URL, UNRESOLVABLE), &[]),
            |e| matches!(e, LoadError::Unresolvable { .. }),
        ),
        from_input(
            "LoadError::Unrunnable",
            load(&document(URL, UNRUNNABLE), &[]),
            |e| matches!(e, LoadError::Unrunnable { .. }),
        ),
        from_input(
            "LoadError::UnrunnablePattern",
            load(&document(URL, UNICODE_SCHEMA), &[]),
            |e| matches!(e, LoadError::UnrunnablePattern { .. }),
        ),
    ]
}

/// Turning values and arguments into a request.
fn requesting() -> Vec<Case> {
    vec![
        from_input("ValueError::BadValue", bad_value(), |e| {
            matches!(e, ValueError::BadValue { .. })
        }),
        from_input("ValueError::BadList", bad_list(), |e| {
            matches!(e, ValueError::BadList { .. })
        }),
        case(
            "PlanError::Request",
            PlanError::Request(RequestError::Http(http_error())),
        ),
        case("RequestError::Http", RequestError::Http(http_error())),
        case("PlanError::Value", PlanError::Value(bad_value())),
        case(
            "client::BodyError::Mismatch",
            BodyError::Mismatch {
                op: "createThing".to_owned(),
                path: ".".to_owned(),
                source: json_error(),
            },
        ),
        case(
            "client::BodyError::Unwritable",
            BodyError::Unwritable {
                op: "createThing".to_owned(),
                source: json_error(),
            },
        ),
        from_input("ArgError::FlagType", flag_type(), |e| {
            matches!(e, ArgError::FlagType { .. })
        }),
        from_input("ArgError::FieldValue", field_value(), |e| {
            matches!(e, ArgError::FieldValue { .. })
        }),
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
            DispatchError::Plan(PlanError::Request(RequestError::Http(http_error()))),
        ),
        case(
            "client::Error::Document",
            ClientError::Document(blob_error()),
        ),
        case(
            "client::Error::Request",
            ClientError::Request(RequestError::Http(http_error())),
        ),
        case(
            "client::Error::Encode",
            ClientError::Encode {
                op: "createThing".to_owned(),
                source: serde_json::to_vec(&BTreeMap::from([((1, 2), 3)]))
                    .expect_err("a JSON object key is a string"),
            },
        ),
        case(
            "client::Error::Transport",
            ClientError::Transport {
                op: "listThings".to_owned(),
                source: Box::new(io_error()),
            },
        ),
        case(
            "client::Error::Decode",
            ClientError::Decode {
                op: "listThings".to_owned(),
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
            GenerateError::Yaml(Box::new(io_error())),
        ),
        case(
            "GenerateError::Unusable",
            GenerateError::Unusable(load("{}", &[])),
        ),
        case(
            "GenerateError::NotOpenApi",
            GenerateError::NotOpenApi {
                at: "paths[/things].get".to_owned(),
                source: json_error(),
            },
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
            GenerateError::Typify(Box::new(typify::Error::InvalidTypeId)),
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

/// What is wrong with `case`: an error that is not the variant it is named for,
/// then each layer of its chain that prints the layer under it.
fn problems(case: &Case) -> Vec<String> {
    let Case {
        name,
        error,
        is_named,
    } = case;
    let not_named =
        (!is_named).then(|| format!("{name}: the input gives {error:?}, which is another variant"));
    not_named
        .into_iter()
        .chain(repeated(name, error.as_ref()))
        .collect()
}

/// Every case at once, so a failing run names each variant that is wrong and
/// not only the first.
fn assert_say_their_own_layer_once(cases: impl IntoIterator<Item = Case>) {
    let problems: Vec<String> = cases.into_iter().flat_map(|case| problems(&case)).collect();
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn every_error_enum_of_the_library_prints_a_cause_once_down_one_chain() {
    assert_say_their_own_layer_once(
        reading()
            .into_iter()
            .chain(loading())
            .chain(requesting())
            .chain(calling()),
    );
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
