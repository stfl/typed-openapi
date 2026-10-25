//! A refusal names the operation and the place it stopped.
//!
//! An adopter meets a refusal in a document of tens of thousands of lines, and
//! a message that says what is wrong without saying where sends them to read the
//! whole file. Each case here is the input of a probe of the review that found
//! it, and each asserts the variant beside the message: the variant is what a
//! caller matches and the message is what a person reads.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::error::Error;

use typed_openapi::{Document, LoadError, RefError};

/// A document of this test's own: one server, and whatever the case puts under
/// it.
fn document(rest: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         {rest}"
    )
}

/// What loading `rest` under the fixed header refuses with.
fn refused(rest: &str) -> LoadError {
    Document::load(&document(rest), &[]).expect_err("the document is refused")
}

/// An operation that the OpenAPI model reads, with the path and the id given.
fn sound(path: &str, id: &str) -> String {
    format!(
        "  {path}:\n    get: {{ operationId: {id}, responses: {{ '200': {{ description: OK }} }} }}\n"
    )
}

/// The cause under `error`, as a person reads it.
fn cause(error: &dyn Error) -> String {
    error.source().expect("the layer has a cause").to_string()
}

/// The OpenAPI model refuses a document and says nothing of where. The reduction
/// reads it again a path item and an operation at a time, so the refusal points
/// at the node and not at `paths`.
#[test]
fn the_openapi_model_refusing_an_operation_names_its_path_and_its_node() {
    let error = refused(&format!(
        "paths:\n{}  /b:\n    get:\n      operationId: b\n      deprecated: 'yes'\n      \
         responses: {{ '200': {{ description: OK }} }}\n",
        sound("/a", "a")
    ));

    assert!(
        matches!(&error, LoadError::Shape { at, .. } if at == "paths[/b].get.deprecated"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "the overlaid document is not an OpenAPI 3 document: refused at `paths[/b].get.deprecated`"
    );
    assert_eq!(
        cause(&error),
        "invalid type: string \"yes\", expected a boolean"
    );
}

/// The first node that fails is the one named, in the order the document lists
/// them, and a later one that fails too is left for the next run.
#[test]
fn the_first_of_two_refused_operations_is_the_one_named() {
    let error = refused(
        "paths:\n  /a:\n    get: { operationId: a, deprecated: 1, responses: {} }\n  \
         /b:\n    get: { operationId: b, deprecated: 2, responses: {} }\n",
    );

    assert!(
        matches!(&error, LoadError::Shape { at, .. } if at == "paths[/a].get.deprecated"),
        "{error:?}"
    );
}

/// A path item holds up to eight operations, and the one that fails is the one
/// named, not the first one the item has.
#[test]
fn the_operation_of_a_path_item_that_fails_is_the_one_named() {
    let error = refused(
        "paths:\n  /a:\n    get: { operationId: a, responses: {} }\n    \
         post: { operationId: b, parameters: [{ name: n, in: bogus }], responses: {} }\n",
    );

    assert!(
        matches!(&error, LoadError::Shape { at, .. } if at == "paths[/a].post.parameters[0]"),
        "{error:?}"
    );
}

/// A refusal in what the path item holds beside its operations is the item's.
#[test]
fn a_path_item_refused_beside_its_operations_names_the_path() {
    let error = refused(
        "paths:\n  /a:\n    parameters: [{ name: n, in: bogus }]\n    \
         get: { operationId: a, responses: {} }\n",
    );

    assert!(
        matches!(&error, LoadError::Shape { at, .. } if at == "paths[/a]"),
        "{error:?}"
    );
}

/// Outside `paths` the model already says where, and the refusal keeps it.
#[test]
fn a_refusal_outside_the_paths_keeps_the_place_the_model_gave() {
    let error = Document::load(
        "openapi: 3.0.3\ninfo: { title: 7, version: \"1\" }\npaths: {}\n",
        &[],
    )
    .expect_err("a title is text");

    assert!(
        matches!(&error, LoadError::Shape { at, .. } if at == "info.title"),
        "{error:?}"
    );
}

/// A document that is no OpenAPI object at all is the document that is refused.
#[test]
fn a_document_that_is_not_an_openapi_object_is_refused_at_its_root() {
    let error = Document::load("swagger: '2.0'\n", &[]).expect_err("not OpenAPI 3");

    assert!(
        matches!(&error, LoadError::Shape { at, .. } if at == "."),
        "{error:?}"
    );
    assert_eq!(cause(&error), "missing field `openapi`");
}

/// The probe's root path has no segment to name a group after, so the group
/// is the path itself, which no command is called. The refusal names the
/// operation and the path, and the two markers that repair it.
#[test]
fn the_root_path_is_named_and_so_is_the_repair() {
    let error = refused(&format!(
        "paths:\n{}{}",
        sound("/", "index"),
        sound("/vouchers", "listVouchers")
    ));

    assert!(
        matches!(&error, LoadError::Name { op, source } if op == "index" && source.raw == "/"),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.starts_with("index: "), "{message}");
    assert!(
        message.contains("`x-cli-group`") && message.contains("`x-cli-command`"),
        "{message}"
    );
    assert_eq!(
        cause(&error),
        "the path `/` does not kebab-case into [a-z0-9-]"
    );
}

/// A path segment that no command can be called names the operation too.
#[test]
fn a_path_segment_that_cannot_be_a_command_names_the_operation() {
    let error = refused(&format!(
        "paths:\n{}{}",
        sound("/vouchers", "listVouchers"),
        sound("/___", "underscores")
    ));

    assert!(
        matches!(&error, LoadError::Name { op, source }
            if op == "underscores" && source.origin == "path segment" && source.raw == "___"),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("`x-cli-group`"),
        "{}",
        error.to_string()
    );
}

/// An `x-cli-command` that is itself unspellable is the probe's `___`.
#[test]
fn an_override_that_cannot_be_a_command_names_the_operation() {
    let error = refused(
        "paths:\n  /vouchers:\n    get:\n      operationId: listVouchers\n      \
         x-cli-command: '___'\n      responses: { '200': { description: OK } }\n",
    );

    assert!(
        matches!(&error, LoadError::Name { op, source }
            if op == "listVouchers" && source.origin == "x-cli-command"),
        "{error:?}"
    );
    assert_eq!(
        cause(&error),
        "the x-cli-command `___` does not kebab-case into [a-z0-9-]"
    );
}

/// A gate that is no flag is a name error with another repair: the gate is
/// spelled in `x-cli-gates`, and `x-cli-group` would not mend it.
#[test]
fn a_gate_that_cannot_be_a_flag_names_the_operation_and_its_own_repair() {
    let error = refused(
        "paths:\n  /vouchers:\n    post:\n      operationId: createVoucher\n      \
         x-cli-gates: ['???']\n      responses: { '200': { description: OK } }\n",
    );

    assert!(
        matches!(&error, LoadError::GateName { op, source }
            if op == "createVoucher" && source.raw == "???"),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(
        message.starts_with("createVoucher: ") && message.contains("`x-cli-gates`"),
        "{message}"
    );
    assert!(!message.contains("x-cli-group"), "{message}");
}

/// A parameter that is a `$ref` to nothing is refused with the operation that
/// holds it, and with the reference.
#[test]
fn a_parameter_reference_that_does_not_resolve_names_the_operation() {
    let error = refused(
        "paths:\n  /a:\n    get:\n      operationId: a\n      \
         parameters: [{ $ref: '#/components/parameters/Missing' }]\n      \
         responses: { '200': { description: OK } }\n",
    );

    assert!(
        matches!(&error, LoadError::ParameterRef { op, source: RefError::Missing { reference } }
            if op == "a" && reference == "#/components/parameters/Missing"),
        "{error:?}"
    );
    assert!(error.to_string().starts_with("a: "), "{error}");
    assert_eq!(
        cause(&error),
        "`#/components/parameters/Missing` does not resolve"
    );
}

/// A path item that is a `$ref` is refused with its path and the reference it
/// holds, both of which the document spells and an adopter can search for.
#[test]
fn a_path_item_reference_names_the_path_and_the_reference() {
    let error = refused(&format!(
        "paths:\n  /a:\n    $ref: 'other.yaml#/paths/~1a'\n{}",
        sound("/b", "b")
    ));

    assert!(
        matches!(&error, LoadError::PathItem { path, source: RefError::Missing { reference } }
            if path == "/a" && reference == "other.yaml#/paths/~1a"),
        "{error:?}"
    );
    assert!(error.to_string().starts_with("/a: "), "{error}");
    assert_eq!(cause(&error), "`other.yaml#/paths/~1a` does not resolve");
}

/// A multipart body whose schema is a `$ref` to nothing loaded with no part
/// names. It is the same refusal as any other body schema that cannot be
/// followed, and names the operation and `requestBody`.
#[test]
fn a_multipart_schema_that_does_not_resolve_is_refused_not_swallowed() {
    let error = refused(
        "paths:\n  /uploads:\n    post:\n      operationId: upload\n      requestBody:\n        \
         required: true\n        content:\n          multipart/form-data:\n            \
         schema: { $ref: '#/components/schemas/Missing' }\n      \
         responses: { '201': { description: Created } }\n",
    );

    assert!(
        matches!(&error, LoadError::Unresolvable { op, name, source: RefError::Missing { reference } }
            if op == "upload" && name == "requestBody"
                && reference == "#/components/schemas/Missing"),
        "{error:?}"
    );
}

/// An Overlay that does not apply is refused with the layer that failed, by its
/// place in the list the caller gave.
#[test]
fn an_overlay_layer_that_fails_is_named_by_its_place_in_the_list() {
    const FIRST: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  \
                         - { target: \"$.info\", update: { description: ok } }\n";
    const SECOND: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  \
                          - { target: \"$.components.nothing\", update: {} }\n";
    let document = document("paths: {}\n");

    let error = Document::load(&document, &[FIRST, SECOND]).expect_err("the second does not apply");

    assert!(
        matches!(&error, LoadError::Layer { layer: 1, .. }),
        "{error:?}"
    );
    assert_eq!(error.to_string(), "cannot use `overlays[1]`");
    assert_eq!(cause(&error), "the Overlay does not apply");
}

/// The layer that cannot be read as YAML is named the same way.
#[test]
fn an_overlay_layer_that_is_not_yaml_is_named() {
    let document = document("paths: {}\n");

    let error = Document::load(&document, &["actions: [\n"]).expect_err("not YAML");

    assert!(
        matches!(&error, LoadError::Layer { layer: 0, .. }),
        "{error:?}"
    );
}

/// The document itself is not a layer, and keeps the variant that names it.
#[test]
fn a_document_that_is_not_yaml_is_not_a_layer() {
    let error = Document::load(": [", &[]).expect_err("not YAML");

    assert!(matches!(&error, LoadError::Overlay(_)), "{error:?}");
}
