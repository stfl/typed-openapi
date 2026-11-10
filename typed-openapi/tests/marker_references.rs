//! Marker placement agrees with the model's reading of parameter references,
//! through loading and the generated model blob.

#![expect(clippy::expect_used, reason = "a broken fixture must fail the test")]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use typed_openapi::generate::{GenerateError, Settings};
use typed_openapi::{Document, LoadError, Shape, Unsupported};

#[derive(Debug, Clone, Copy)]
enum Position {
    Path,
    Operation,
    Component,
}

const POSITIONS: [Position; 3] = [Position::Path, Position::Operation, Position::Component];

impl Position {
    fn at(self) -> &'static str {
        match self {
            Self::Path => "$.paths['/items'].parameters[0]",
            Self::Operation => "$.paths['/items'].get.parameters[1]",
            Self::Component => "$.components.parameters.A",
        }
    }
}

fn direct_references() -> [Option<Value>; 6] {
    [
        None,
        Some(json!(42)),
        Some(Value::Null),
        Some(json!(true)),
        Some(json!({})),
        Some(json!([])),
    ]
}

fn out(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("the test's directory");
    dir
}

fn source(position: Position, reference: Option<Value>, marker: Option<Value>) -> String {
    let mut parameter = json!({
        "name": "a", "in": "query", "schema": { "type": "string" }
    });
    let fields = parameter.as_object_mut().expect("a parameter");
    if let Some(reference) = reference {
        fields.insert("$ref".into(), reference);
    }
    if let Some(marker) = marker {
        fields.insert("x-cli-requires".into(), marker);
    }
    let mut path_parameters = Vec::new();
    let mut operation_parameters = vec![json!({
        "name": "b", "in": "query", "schema": { "type": "string" }
    })];
    let mut components = json!({
        "Target": { "name": "c", "in": "query", "schema": { "type": "string" } }
    });
    match position {
        Position::Path => path_parameters.push(parameter),
        Position::Operation => operation_parameters.push(parameter),
        Position::Component => {
            components
                .as_object_mut()
                .expect("parameter map")
                .insert("A".into(), parameter);
            operation_parameters.push(json!({ "$ref": "#/components/parameters/A" }));
        }
    }
    json!({
        "openapi": "3.0.3", "info": { "title": "Markers", "version": "1" },
        "servers": [{ "url": "http://localhost:9999" }],
        "paths": { "/items": {
            "parameters": path_parameters,
            "get": {
                "operationId": "listItems",
                "parameters": operation_parameters,
                "responses": { "200": { "description": "OK" } }
            }
        } },
        "components": { "parameters": components }
    })
    .to_string()
}

fn generate(source: &str, dir: &Path) -> Result<Vec<PathBuf>, GenerateError> {
    let input = dir.join("input.json");
    std::fs::write(&input, source).expect("the input");
    Settings::new(&input).write_to(dir.join("generated"))
}

fn read_both(source: &str, dir: &Path) -> [Document; 2] {
    let loaded = Document::load(source, &[]).expect("a direct parameter");
    generate(source, dir).expect("the document generates");
    let blob = std::fs::read(dir.join("generated/src/model.postcard")).expect("the generated blob");
    [
        loaded,
        Document::from_blob(&blob).expect("the generated model"),
    ]
}

fn refused_both(source: &str, dir: &Path) -> [LoadError; 2] {
    let loaded = Document::load(source, &[]).expect_err("the load must refuse");
    let error = generate(source, dir).expect_err("generation must refuse");
    let GenerateError::Unusable(generated) = error else {
        panic!("the generator must report the load refusal: {error:?}");
    };
    [loaded, generated]
}

fn assert_parameter(doc: &Document, name: &str, requires: &[&str]) {
    let op = doc.get("listItems").expect("the operation");
    assert_eq!(op.params().len(), 2);
    let parameter = op.param(name).expect("the parameter");
    let Shape::Flag { flag, .. } = parameter.shape() else {
        panic!("the parameter must have a flag");
    };
    assert_eq!(flag.as_str(), name);
    assert_eq!(parameter.requires(), requires);
}

#[test]
fn a_nonstring_ref_keeps_the_direct_parameters_requires() {
    let dir = out("marker-reference-direct");
    for position in POSITIONS {
        let baseline = Document::load(&source(position, None, Some(json!(["b"]))), &[])
            .expect("the control")
            .to_blob()
            .expect("the control blob");
        for reference in direct_references() {
            for doc in read_both(&source(position, reference, Some(json!(["b"]))), &dir) {
                assert_parameter(&doc, "a", &["b"]);
                assert_parameter(&doc, "b", &[]);
                assert_eq!(
                    doc.to_blob().expect("the model blob"),
                    baseline,
                    "{position:?}"
                );
            }
        }
    }
}

#[test]
fn nonstring_refs_do_not_hide_malformed_requires_values() {
    let dir = out("marker-reference-malformed");
    for position in POSITIONS {
        for reference in direct_references() {
            for marker in [
                json!(null),
                json!(42),
                json!(true),
                json!("b"),
                json!({}),
                json!([42]),
                json!([null]),
                json!([true]),
                json!([{}]),
                json!([[]]),
            ] {
                let errors = refused_both(
                    &source(position, reference.clone(), Some(marker.clone())),
                    &dir,
                );
                assert!(
                    errors.iter().all(|error| matches!(error,
                    LoadError::RequiresList { op, name } if op == "listItems" && name == "a")),
                    "{position:?}, {reference:?}, {marker}: {errors:?}"
                );
            }
        }
    }
}

#[test]
fn nonstring_refs_do_not_hide_invalid_requires_relations() {
    let dir = out("marker-reference-relations");
    for position in POSITIONS {
        for reference in direct_references() {
            for error in refused_both(
                &source(position, reference.clone(), Some(json!(["a"]))),
                &dir,
            ) {
                assert!(
                    matches!(&error, LoadError::RequiresItself { op, name } if op == "listItems" && name == "a"),
                    "{position:?}, {reference:?}: {error:?}"
                );
            }
            for error in refused_both(
                &source(position, reference.clone(), Some(json!(["absent"]))),
                &dir,
            ) {
                assert!(
                    matches!(&error, LoadError::RequiresUnknown { op, name, requires } if op == "listItems" && name == "a" && requires == "absent"),
                    "{position:?}, {reference:?}: {error:?}"
                );
            }
        }
    }
}

#[test]
fn nonstring_refs_do_not_hide_an_unreachable_requirement() {
    let dir = out("marker-reference-unreachable");
    for position in POSITIONS {
        for reference in direct_references() {
            let mut document: Value =
                serde_json::from_str(&source(position, reference.clone(), Some(json!(["b"]))))
                    .expect("the fixture is JSON");
            *document
                .pointer_mut("/paths/~1items/get/parameters/0/schema")
                .expect("b's schema") = json!({ "type": "object" });
            for error in refused_both(&document.to_string(), &dir) {
                assert!(
                    matches!(&error, LoadError::RequiresUnreachable { op, name, requires, why: Unsupported::Structured }
                    if op == "listItems" && name == "a" && requires == "b"),
                    "{position:?}, {reference:?}: {error:?}"
                );
            }
        }
    }
}

#[test]
fn string_refs_drop_sibling_parameter_fields_and_refuse_sibling_markers() {
    let dir = out("marker-reference-string");
    for position in POSITIONS {
        let reference = Some(json!("#/components/parameters/Target"));
        for doc in read_both(&source(position, reference.clone(), None), &dir) {
            assert_parameter(&doc, "c", &[]);
            assert_parameter(&doc, "b", &[]);
            assert!(
                doc.get("listItems")
                    .expect("the operation")
                    .param("a")
                    .is_none(),
                "{position:?}: sibling name must be ignored"
            );
        }
        for marker in [json!(["b"]), json!(null)] {
            for error in refused_both(&source(position, reference.clone(), Some(marker)), &dir) {
                assert!(
                    matches!(&error, LoadError::MisplacedMarker { key, at } if key == "x-cli-requires" && at == position.at()),
                    "{position:?}: {error:?}"
                );
            }
        }
    }
}
