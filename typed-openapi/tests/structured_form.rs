//! Structured form contracts at document, generation, blob and request seams.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "fixtures fail loudly"
)]

use serde_json::{Value, json};
use typed_openapi::{Body, Document};

fn source() -> Value {
    json!({
        "openapi": "3.0.3", "info": {"title": "Toy", "version": "1"},
        "servers": [{"url": "http://localhost:9999"}],
        "paths": {"/forms": {"post": {
            "operationId": "submitForm", "x-cli-form-json": true,
            "requestBody": {"required": true, "content": {
                "application/x-www-form-urlencoded": {"schema": {"$ref": "#/components/schemas/Form"}}
            }}, "responses": {"204": {"description": "OK"}}
        }}},
        "components": {"schemas": {"Form": {
            "type": "object", "additionalProperties": false,
            "required": ["choice", "digits", "kind", "ref[id]", "text"],
            "properties": {
                "choice": {"type": "string", "enum": ["0", "1"]},
                "digits": {"type": "string", "pattern": "^[0-9]+$"},
                "kind": {"type": "string", "enum": ["Toy"], "default": "Toy"},
                "ref[id]": {"type": "string"}, "text": {"type": "string"}
            }
        }}}
    })
}

fn load(value: &Value) -> Result<Document, typed_openapi::LoadError> {
    Document::load(&value.to_string(), &[])
}

#[test]
fn structured_form_marker_contract() {
    let doc = load(&source()).expect("true on an operation is supported");
    generated(&source(), "structured-form-marker-true").expect("the marked operation generates");
    assert_eq!(
        doc.get("submitForm")
            .expect("operation")
            .body()
            .media_type(),
        Some("application/x-www-form-urlencoded")
    );
    for marker in [
        json!(false),
        json!("true"),
        json!(1),
        json!(null),
        json!([]),
    ] {
        let mut value = source();
        value["paths"]["/forms"]["post"]["x-cli-form-json"] = marker;
        let error = load(&value).expect_err("only boolean true").to_string();
        assert!(generated(&value, "structured-form-marker-invalid").is_err());
        assert!(
            error.contains("submitForm") && error.contains("x-cli-form-json"),
            "{error}"
        );
    }
    let mut value = source();
    value["paths"]["/forms"]["post"]
        .as_object_mut()
        .expect("object")
        .remove("x-cli-form-json");
    assert!(matches!(
        load(&value)
            .expect("unmarked")
            .get("submitForm")
            .expect("operation")
            .body(),
        Body::Opaque { .. }
    ));
    check_marker_locations();
}

fn check_marker_locations() {
    for (pointer, marker) in [
        ("", "x-cli-form-json"),
        ("/components/schemas/Form", "x-cli-form-json"),
        ("/paths/~1forms", "x-cli-form-json"),
        ("/paths/~1forms/post/requestBody", "x-cli-form-json"),
        (
            "/components/schemas/Form/properties/text",
            "x-cli-form-json",
        ),
        ("/paths/~1forms/post", "x-cli-form-jsno"),
    ] {
        let mut value = source();
        value.pointer_mut(pointer).expect("location")[marker] = json!(true);
        let error = load(&value).expect_err("marker refused").to_string();
        assert!(error.contains(marker), "{error}");
    }
}

fn valid() -> Value {
    json!({"choice":"1", "digits":"42", "kind":"Toy", "ref[id]":"A+B %&=ä", "text":" ~*._-\n"})
}

const GOLDEN: &[u8] =
    b"choice=1&digits=42&kind=Toy&ref%5Bid%5D=A%2BB+%25%26%3D%C3%A4&text=+%7E*._-%0A";

fn generated(
    value: &Value,
    name: &str,
) -> Result<std::path::PathBuf, typed_openapi::generate::GenerateError> {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("remove own output");
    }
    std::fs::create_dir_all(&dir).expect("create output");
    let input = dir.join("form.json");
    std::fs::write(&input, value.to_string()).expect("input");
    typed_openapi::generate::Settings::new(input).write_to(dir.join("generated"))?;
    Ok(dir.join("generated"))
}

fn schema_refusals() -> Vec<(String, Value)> {
    let mut rows = Vec::new();
    for body in [
        json!(null),
        json!({"required":false}),
        json!({"required":true,"content":{}}),
        json!({"required":true,"content":{"application/json":{}}}),
    ] {
        let mut value = source();
        if body.is_null() {
            value["paths"]["/forms"]["post"]
                .as_object_mut()
                .expect("operation")
                .remove("requestBody");
        } else {
            value["paths"]["/forms"]["post"]["requestBody"] = body;
        }
        rows.push(("body".into(), value));
    }
    for (key, entry) in [
        ("required", json!(false)),
        (
            "content",
            json!({"application/json":{}, "application/x-www-form-urlencoded":{}}),
        ),
    ] {
        let mut value = source();
        value["paths"]["/forms"]["post"]["requestBody"][key] = entry;
        rows.push((key.into(), value));
    }
    for schema in [
        json!({"type":"object"}),
        json!({"allOf":[{"$ref":"#/components/schemas/Form"}]}),
        json!({}),
    ] {
        let mut value = source();
        value["paths"]["/forms"]["post"]["requestBody"]["content"]["application/x-www-form-urlencoded"]
            ["schema"] = schema;
        rows.push(("unnamed".into(), value));
    }
    rows.extend(object_refusals());
    let mut open = source();
    open["components"]["schemas"]["Form"]
        .as_object_mut()
        .expect("schema")
        .remove("additionalProperties");
    rows.push(("implicit open".into(), open));
    rows.extend(property_refusals());
    for directive in [json!({}), json!({"text":{"style":"form"}})] {
        let mut value = source();
        value["paths"]["/forms"]["post"]["requestBody"]["content"]["application/x-www-form-urlencoded"]
            ["encoding"] = directive;
        rows.push(("encoding".into(), value));
    }
    rows
}

fn object_refusals() -> Vec<(String, Value)> {
    let mut rows = Vec::new();
    for (key, entry) in [
        ("additionalProperties", json!(true)),
        ("additionalProperties", json!({})),
        ("required", json!(["choice", "digits", "kind", "text"])),
        ("nullable", json!(true)),
        ("minProperties", json!(1)),
        ("maxProperties", json!(9)),
        ("enum", json!([{}])),
        ("allOf", json!([{"type":"object"}])),
        ("anyOf", json!([{"type":"object"}])),
        ("oneOf", json!([{"type":"object"}])),
        ("not", json!({})),
    ] {
        let mut value = source();
        value["components"]["schemas"]["Form"][key] = entry;
        rows.push((key.into(), value));
    }
    rows
}

fn property_refusals() -> Vec<(String, Value)> {
    let mut rows = Vec::new();
    for property in [
        json!({"type":"number"}),
        json!({"type":"integer"}),
        json!({"type":"boolean"}),
        json!({"type":"array", "items":{"type":"string"}}),
        json!({"type":"object"}),
        json!({"type":"string","nullable":true}),
        json!({"type":"string","readOnly":true}),
        json!({"allOf":[{"type":"string"}]}),
        json!({"type":"string","not":{"enum":["x"]}}),
        json!({"type":"string","const":"x"}),
        json!({"type":"string","minLength":-1}),
        json!({"type":"string","minLength":4294967296_u64}),
        json!({"type":"string","maxLength":4294967296_u64}),
        json!({"type":"string","enum":[null]}),
        json!({"type":"string","enum":[]}),
        json!({"type":"string","minimum":1}),
        json!({"type":"string","pattern":"["}),
    ] {
        let mut value = source();
        value["components"]["schemas"]["Form"]["properties"]["text"] = property;
        rows.push(("property".into(), value));
    }
    rows
}

#[test]
fn structured_form_schema_contract() {
    let mut value = source();
    let property = value["components"]["schemas"]["Form"]["properties"]["digits"].take();
    value["components"]["schemas"]["Digits"] = property;
    value["components"]["schemas"]["Form"]["properties"]["digits"] =
        json!({"$ref":"#/components/schemas/Digits"});
    value["components"]["schemas"]["Alias"] = json!({"$ref":"#/components/schemas/Form"});
    value["paths"]["/forms"]["post"]["requestBody"]["content"]["application/x-www-form-urlencoded"]
        ["schema"] = json!({"$ref":"#/components/schemas/Alias"});
    let body = value["paths"]["/forms"]["post"]["requestBody"].take();
    value["components"]["requestBodies"]["Body"] = body;
    value["paths"]["/forms"]["post"]["requestBody"] =
        json!({"$ref":"#/components/requestBodies/Body"});
    assert!(load(&value).is_ok(), "ordinary reference chains");
    let output = generated(&value, "structured-form-schema")
        .expect("generation follows the same references");
    let ops = std::fs::read_to_string(output.join("src/ops.rs")).expect("generated ops");
    assert!(ops.contains("body: &crate::types::SubmitFormBody"), "{ops}");
    let mut boundary = source();
    boundary["components"]["schemas"]["Form"]["properties"]["text"]["maxLength"] = json!(u32::MAX);
    load(&boundary).expect("the generator's largest length survives reduction");
    generated(&boundary, "structured-form-length-boundary")
        .expect("the largest supported length generates");
    let mut formatted = source();
    formatted["components"]["schemas"]["Form"]["properties"]["text"]["format"] = json!("date");
    let output = generated(&formatted, "structured-form-descriptive-format")
        .expect("descriptive format generates");
    let types = std::fs::read_to_string(output.join("src/types.rs")).expect("types");
    let request = types
        .split("pub struct SubmitFormBody {")
        .nth(1)
        .expect("form-specific request type")
        .split('}')
        .next()
        .expect("fields");
    assert!(
        request.contains("pub text: ::std::string::String"),
        "a format must not add a validation rule: {request}"
    );
    assert!(
        types.contains("::chrono"),
        "the ordinary component retains its JSON meaning"
    );
    for (name, value) in schema_refusals() {
        let error = load(&value).expect_err(&name).to_string();
        assert!(
            error.contains("submitForm") && error.contains("x-cli-form-json"),
            "{name}: {error}"
        );
    }
}

fn refused_before_plan(doc: &Document, values: typed_openapi::Values) {
    let recorder = typed_openapi::Recorder::new();
    let result = typed_openapi::Plan::build(
        doc.get("submitForm").expect("operation"),
        doc.base(),
        values,
        &typed_openapi::Answers::new().commit(),
    );
    let error = result.expect_err("invalid input cannot produce a Plan");
    assert!(error.to_string().contains("submitForm"), "{error}");
    assert!(recorder.take().is_empty());
}

#[test]
fn structured_form_direct_values_contract() {
    let mut value = source();
    value["components"]["schemas"]["Form"]["default"] = valid();
    let doc = load(&value).expect("schema defaults remain descriptive");
    refused_before_plan(&doc, typed_openapi::Values::new());
    refused_before_plan(&doc, typed_openapi::Values::new().raw(vec![1, 2, 3]));
    for body in [
        json!(null),
        json!([]),
        json!("text"),
        json!(3),
        json!(false),
    ] {
        refused_before_plan(&doc, typed_openapi::Values::new().json(body));
    }
    for key in ["choice", "digits", "kind", "ref[id]", "text"] {
        let mut body = valid();
        body.as_object_mut().expect("object").remove(key);
        refused_before_plan(&doc, typed_openapi::Values::new().json(body));
        for wrong in [json!(null), json!(42), json!(true), json!([]), json!({})] {
            let mut body = valid();
            body[key] = wrong;
            refused_before_plan(&doc, typed_openapi::Values::new().json(body));
        }
    }
    let mut extra = valid();
    extra["extra"] = json!("ignored?");
    refused_before_plan(&doc, typed_openapi::Values::new().json(extra));
    for (key, wrong) in [("choice", "2"), ("digits", "4x"), ("kind", "toy")] {
        let mut body = valid();
        body[key] = json!(wrong);
        refused_before_plan(&doc, typed_openapi::Values::new().json(body));
    }
    check_constraint_table();
}

fn check_constraint_table() {
    // Independent constraint rows and their conjunction, with character rather
    // than byte boundaries for UTF-8 and a decomposed two-character string.
    for (rules, allowed, denied) in [
        (json!({"enum":["ä","abc"]}), vec!["ä", "abc"], vec!["", "x"]),
        (json!({"pattern":"^[0-9]+$"}), vec!["42"], vec!["", "4x"]),
        (
            json!({"minLength":1,"maxLength":2}),
            vec!["ä", "😀", "ä😀"],
            vec!["", "ä😀x", "e\u{301}x"],
        ),
        (
            json!({"enum":["ä","ä😀","abc","x"],"pattern":"^ä","minLength":2,"maxLength":2}),
            vec!["ä😀"],
            vec!["ä", "abc", "x", "äx"],
        ),
    ] {
        let mut source = source();
        let mut schema = rules;
        schema["type"] = json!("string");
        source["components"]["schemas"]["Form"]["properties"]["text"] = schema;
        let doc = load(&source).expect("supported constraints");
        for text in allowed {
            let mut body = valid();
            body["text"] = json!(text);
            assert!(
                typed_openapi::Invocation::new(
                    doc.get("submitForm").expect("operation"),
                    typed_openapi::Values::new().json(body)
                )
                .is_ok()
            );
        }
        for text in denied {
            let mut body = valid();
            body["text"] = json!(text);
            refused_before_plan(&doc, typed_openapi::Values::new().json(body));
        }
    }
}

fn request(doc: &Document) -> typed_openapi::HttpRequest {
    typed_openapi::Invocation::new(
        doc.get("submitForm").expect("operation"),
        typed_openapi::Values::new().json(valid()),
    )
    .expect("valid")
    .request(doc.base())
    .expect("request")
}

#[test]
fn structured_form_blob_contract() {
    let mut value = source();
    value["components"]["schemas"]["Form"]["properties"]["digits"] = json!({"type":"string","enum":["42","123"],"pattern":"^[0-9]+$","minLength":2,"maxLength":2});
    let source = load(&value).expect("supported");
    let blob = source.to_blob().expect("encode");
    let decoded = Document::from_blob(&blob).expect("decode");
    assert_eq!(source, decoded, "mode and every constraint survive");
    for doc in [&source, &decoded] {
        assert_eq!(request(doc).body(), GOLDEN);
        let mut wrong = valid();
        wrong["digits"] = json!("123");
        refused_before_plan(doc, typed_openapi::Values::new().json(wrong));
    }
    assert!(matches!(
        Document::from_blob(include_bytes!("fixtures/model-layout-9.postcard")),
        Err(typed_openapi::DocumentError::Stale { .. })
    ));
}
