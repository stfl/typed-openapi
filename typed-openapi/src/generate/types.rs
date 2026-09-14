//! `components.schemas` as Rust types, from typify.
//!
//! The one setting that matters is [`with_conversion`]: the adopter owns a Rust
//! type for a vendor *format*, and typify emits that type wherever the document
//! declares the format. A `total` field of `format: money` becomes the
//! adopter's own newtype, named by the path
//! [`Settings::replace`](super::Settings::replace) gives it. There is no
//! hand-written mirror on top of the generated type and nothing added to the
//! vendor's `components.schemas` to hang a name on.
//!
//! Keying on the format rather than on a name is what keeps the correction
//! honest: the shape typify is told to replace is read out of the document, so
//! the rule a CLI validates against and the rule the Rust type stands for are
//! the same bytes.
//!
//! [`with_conversion`]: typify::TypeSpaceSettings::with_conversion

use serde_json::Value;
use typify::{TypeSpace, TypeSpaceImpl, TypeSpaceSettings};

use super::GenerateError;

/// One schema of the document, under its own name, in typify's dialect.
type Definition = (String, Value);

pub(super) fn emit(
    api: &openapiv3::OpenAPI,
    header: &str,
    replacements: &[(String, String)],
) -> Result<String, GenerateError> {
    let definitions = definitions(api)?;

    let mut settings = TypeSpaceSettings::default();
    settings.with_derive("PartialEq".to_owned());
    for (format, rust) in replacements {
        for shape in shapes_declaring(&definitions, format) {
            let shape: schemars::schema::SchemaObject =
                serde_json::from_value(shape).map_err(|source| {
                    GenerateError::Unsupported(format!(
                        "a `format: {format}` shape typify does not accept: {source}"
                    ))
                })?;
            settings.with_conversion(
                shape,
                rust,
                [TypeSpaceImpl::Display, TypeSpaceImpl::FromStr].into_iter(),
            );
        }
    }

    let mut space = TypeSpace::new(&settings);
    space
        .add_ref_types(schemas(definitions)?)
        .map_err(GenerateError::Typify)?;

    let file: syn::File =
        syn::parse2(space.to_stream()).map_err(|source| GenerateError::NotRust {
            file: "types.rs",
            source,
        })?;
    Ok(format!("{header}{}", prettyplease::unparse(&file)))
}

/// Every named schema the document declares, in document order.
fn definitions(api: &openapiv3::OpenAPI) -> Result<Vec<Definition>, GenerateError> {
    let components = api.components.as_ref().ok_or_else(|| {
        GenerateError::Unsupported("the overlaid document has no components".to_owned())
    })?;
    components
        .schemas
        .iter()
        .map(|(name, schema)| {
            let value = serde_json::to_value(schema).map_err(|source| GenerateError::Schema {
                name: name.clone(),
                source,
            })?;
            Ok((name.clone(), as_json_schema(value)))
        })
        .collect()
}

/// The same schemas as typify's own type, which is the last point at which a
/// shape it cannot read is still attributable to a name.
fn schemas(
    definitions: Vec<Definition>,
) -> Result<Vec<(String, schemars::schema::Schema)>, GenerateError> {
    definitions
        .into_iter()
        .map(|(name, value)| {
            let schema: schemars::schema::Schema =
                serde_json::from_value(value).map_err(|source| {
                    GenerateError::Unsupported(format!(
                        "schema `{name}` is not a JSON Schema typify accepts: {source}"
                    ))
                })?;
            Ok((name, schema))
        })
        .collect()
}

/// Every distinct schema in the document that declares `format`, in document
/// order.
///
/// typify matches a conversion on the whole shape, so a document that spells
/// one format two ways gets one conversion per spelling rather than a silent
/// miss on the second.
fn shapes_declaring(definitions: &[Definition], format: &str) -> Vec<Value> {
    let mut found = Vec::new();
    for (_, schema) in definitions {
        collect(schema, format, &mut found);
    }
    found
}

fn collect(value: &Value, format: &str, found: &mut Vec<Value>) {
    let Value::Object(fields) = value else {
        if let Value::Array(items) = value {
            for item in items {
                collect(item, format, found);
            }
        }
        return;
    };
    if fields.get("format") == Some(&Value::String(format.to_owned())) && !found.contains(value) {
        found.push(value.clone());
    }
    for nested in fields.values() {
        collect(nested, format, found);
    }
}

/// An OpenAPI schema object as the JSON Schema typify understands: only the
/// `$ref` spelling differs.
fn as_json_schema(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    let value = match (key.as_str(), &value) {
                        ("$ref", Value::String(target)) => {
                            Value::String(target.replace("#/components/schemas/", "#/definitions/"))
                        }
                        _ => as_json_schema(value),
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(as_json_schema).collect()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value,
    }
}
