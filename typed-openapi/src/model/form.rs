//! Reduce marked forms before the OpenAPI object model can discard a rule.
//! Ordinary references use the schema resolver; this mode accepts only its
//! explicit bounded vocabulary, including on schemas reached through a ref.

use std::collections::HashMap;

use openapiv3::ReferenceOr;
use serde_json::Value;

use super::{Body, FormField, LoadError};
use crate::markers::FORM_JSON;
use crate::scalar::{Scalar, Text};
use crate::schema::resolve;

const MEDIA: &str = "application/x-www-form-urlencoded";
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];
const ANNOTATIONS: [&str; 11] = [
    "title",
    "description",
    "format",
    "default",
    "example",
    "externalDocs",
    "xml",
    "deprecated",
    "readOnly",
    "writeOnly",
    "nullable",
];

fn refused(op: &str, reason: impl Into<String>) -> LoadError {
    LoadError::StructuredForm {
        op: op.to_owned(),
        reason: reason.into(),
    }
}

/// Preserve the resolver's missing/inside/cycle and reference-sibling rules
/// while reading the JSON the object model would otherwise normalise away.
fn followed<'a>(
    doc: &'a Value,
    value: &'a Value,
    section: &str,
    op: &str,
) -> Result<&'a Value, LoadError> {
    let wrap = |value: &'a Value| match value.get("$ref").and_then(Value::as_str) {
        Some(reference) => ReferenceOr::Reference {
            reference: reference.to_owned(),
        },
        None => ReferenceOr::Item(value),
    };
    let entries: HashMap<_, _> = doc
        .get("components")
        .and_then(|c| c.get(section))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(key, value)| (key.as_str(), wrap(value)))
        .collect();
    resolve(&wrap(value), |key| entries.get(key), section)
        .copied()
        .map_err(|source| LoadError::Unresolvable {
            op: op.to_owned(),
            name: "requestBody".to_owned(),
            source,
        })
}

pub(super) fn reduce(doc: &Value) -> Result<HashMap<String, Body>, LoadError> {
    let mut forms = HashMap::new();
    for (path, item) in doc
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if !path.starts_with('/') {
            continue;
        }
        for method in METHODS {
            let Some(operation) = item.get(method) else {
                continue;
            };
            let Some(marker) = operation.get(FORM_JSON) else {
                continue;
            };
            // The ordinary load owns a missing operationId.
            let Some(op) = operation.get("operationId").and_then(Value::as_str) else {
                continue;
            };
            if marker != &Value::Bool(true) {
                return Err(refused(op, "the marker must be the boolean true"));
            }
            forms.insert(op.to_owned(), body(doc, operation, op)?);
        }
    }
    Ok(forms)
}

fn body(doc: &Value, operation: &Value, op: &str) -> Result<Body, LoadError> {
    let body = operation
        .get("requestBody")
        .ok_or_else(|| refused(op, "requestBody is absent"))?;
    let body = followed(doc, body, "requestBodies", op)?;
    if body.get("required") != Some(&Value::Bool(true)) {
        return Err(refused(op, "requestBody must be required"));
    }
    let content = body
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            refused(
                op,
                "content must contain only application/x-www-form-urlencoded",
            )
        })?;
    let media = content
        .get(MEDIA)
        .filter(|_| content.len() == 1)
        .ok_or_else(|| {
            refused(
                op,
                "content must contain exactly application/x-www-form-urlencoded",
            )
        })?;
    if media.get("encoding").is_some() {
        return Err(refused(op, "form encoding directives are unsupported"));
    }
    let schema = media
        .get("schema")
        .filter(|s| s.get("$ref").and_then(Value::as_str).is_some())
        .ok_or_else(|| refused(op, "schema must reference a named component object"))?;
    let schema = followed(doc, schema, "schemas", op)?;
    object(doc, schema, op)
}

fn object(doc: &Value, schema: &Value, op: &str) -> Result<Body, LoadError> {
    vocabulary(
        schema,
        &["type", "properties", "required", "additionalProperties"],
        op,
        "object",
    )?;
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(refused(
            op,
            "schema must resolve to an object without composition",
        ));
    }
    if schema.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Err(refused(op, "object must state additionalProperties: false"));
    }
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| refused(op, "object must declare properties"))?;
    let required = schema.get("required").and_then(Value::as_array);
    let mut fields = Vec::new();
    for (name, property) in properties {
        if !required.is_some_and(|names| names.iter().any(|n| n.as_str() == Some(name))) {
            return Err(refused(op, format!("property `{name}` must be required")));
        }
        fields.push(field(doc, property, name, op)?);
    }
    Ok(Body::Form { fields })
}

fn vocabulary(schema: &Value, keys: &[&str], op: &str, at: &str) -> Result<(), LoadError> {
    let object = schema
        .as_object()
        .ok_or_else(|| refused(op, format!("{at} schema must be an object")))?;
    for key in object.keys() {
        if !keys.contains(&key.as_str())
            && !ANNOTATIONS.contains(&key.as_str())
            && !key.starts_with("x-")
        {
            return Err(refused(
                op,
                format!("unsupported {at} schema keyword `{key}`"),
            ));
        }
    }
    for key in ["nullable", "readOnly"] {
        if schema
            .get(key)
            .is_some_and(|value| value != &Value::Bool(false))
        {
            return Err(refused(op, format!("{at} cannot be {key}")));
        }
    }
    Ok(())
}

fn field(doc: &Value, property: &Value, name: &str, op: &str) -> Result<FormField, LoadError> {
    let schema = followed(doc, property, "schemas", op)?;
    vocabulary(
        schema,
        &["type", "enum", "pattern", "minLength", "maxLength"],
        op,
        name,
    )?;
    if schema.get("type").and_then(Value::as_str) != Some("string") {
        return Err(refused(
            op,
            format!("property `{name}` must be a non-nullable string"),
        ));
    }
    let choices = schema
        .get("enum")
        .map(|value| {
            let values = value
                .as_array()
                .filter(|values| !values.is_empty())
                .ok_or_else(|| {
                    refused(op, format!("{name}: enum must be a non-empty string list"))
                })?;
            values
                .iter()
                .map(|value| {
                    value.as_str().map(str::to_owned).ok_or_else(|| {
                        refused(op, format!("{name}: enum must contain only strings"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let pattern = schema
        .get("pattern")
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| refused(op, format!("{name}: pattern must be a string")))
        })
        .transpose()?;
    let text = Text {
        pattern,
        min_length: length(schema, "minLength", op, name)?,
        max_length: length(schema, "maxLength", op, name)?,
    };
    Scalar::Text(text.clone())
        .runnable()
        .map_err(|error| refused(op, format!("{name}: {error}")))?;
    Ok(FormField {
        name: name.to_owned(),
        choices,
        text,
    })
}

fn length(schema: &Value, key: &str, op: &str, name: &str) -> Result<Option<usize>, LoadError> {
    schema
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| refused(op, format!("{name}: {key} must be a non-negative length")))
        })
        .transpose()
}
