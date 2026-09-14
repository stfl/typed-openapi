//! `components.schemas` as Rust types, from typify.
//!
//! The one setting that matters is [`with_conversion`]: the adopter owns a Rust
//! type for a vendor *format*, and typify emits that type wherever the document
//! declares the format. `Voucher.total` is `format: money`, so the generated
//! field is `api_types::Money` — the adopter's own newtype, named by the path
//! [`REPLACED`] gives it. There is no hand-written mirror on top of the
//! generated type and nothing added to the vendor's `components.schemas` to
//! hang a name on.
//!
//! Keying on the format rather than on a name is what keeps the correction
//! honest: the shape typify is told to replace is read out of the document, so
//! the rule the CLI validates against and the rule the Rust type stands for are
//! the same bytes.
//!
//! [`with_conversion`]: typify::TypeSpaceSettings::with_conversion

use anyhow::{Context, Result};
use typify::{TypeSpace, TypeSpaceImpl, TypeSpaceSettings};

use crate::HEADER;

/// Formats the adopter owns by hand. The format is the vendor's; the type is
/// the adopter's, and typify substitutes it rather than emitting its own.
const REPLACED: &[(&str, &str)] = &[("money", "api_types::Money")];

pub(crate) fn emit(api: &openapiv3::OpenAPI) -> Result<String> {
    let components = api
        .components
        .as_ref()
        .context("the overlaid document has no components")?;

    let definitions = components
        .schemas
        .iter()
        .map(|(name, schema)| {
            let value = serde_json::to_value(schema)
                .with_context(|| format!("schema `{name}` is not representable as JSON"))?;
            Ok((name.clone(), as_json_schema(value)))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut settings = TypeSpaceSettings::default();
    settings.with_derive("PartialEq".to_owned());
    for (format, rust) in REPLACED {
        for shape in shapes_declaring(&definitions, format) {
            let shape: schemars::schema::SchemaObject = serde_json::from_value(shape)
                .with_context(|| format!("a `format: {format}` shape typify does not accept"))?;
            settings.with_conversion(
                shape,
                rust,
                [TypeSpaceImpl::Display, TypeSpaceImpl::FromStr].into_iter(),
            );
        }
    }
    let mut space = TypeSpace::new(&settings);

    let definitions = definitions
        .into_iter()
        .map(|(name, value)| {
            let schema: schemars::schema::Schema = serde_json::from_value(value)
                .with_context(|| format!("schema `{name}` is not a JSON Schema typify accepts"))?;
            Ok((name, schema))
        })
        .collect::<Result<Vec<_>>>()?;
    space.add_ref_types(definitions)?;

    let file: syn::File = syn::parse2(space.to_stream())?;
    Ok(format!("{HEADER}{}", prettyplease::unparse(&file)))
}

/// Every distinct schema in the document that declares `format`, in document
/// order.
///
/// typify matches a conversion on the whole shape, so a document that spells
/// one format two ways gets one conversion per spelling rather than a silent
/// miss on the second.
fn shapes_declaring(
    definitions: &[(String, serde_json::Value)],
    format: &str,
) -> Vec<serde_json::Value> {
    let mut found = Vec::new();
    for (_, schema) in definitions {
        collect(schema, format, &mut found);
    }
    found
}

fn collect(value: &serde_json::Value, format: &str, found: &mut Vec<serde_json::Value>) {
    let serde_json::Value::Object(fields) = value else {
        if let serde_json::Value::Array(items) = value {
            for item in items {
                collect(item, format, found);
            }
        }
        return;
    };
    if fields.get("format") == Some(&serde_json::Value::String(format.to_owned()))
        && !found.contains(value)
    {
        found.push(value.clone());
    }
    for nested in fields.values() {
        collect(nested, format, found);
    }
}

/// An OpenAPI schema object as the JSON Schema typify understands: only the
/// `$ref` spelling differs.
fn as_json_schema(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    let value = match (key.as_str(), &value) {
                        ("$ref", serde_json::Value::String(target)) => serde_json::Value::String(
                            target.replace("#/components/schemas/", "#/definitions/"),
                        ),
                        _ => as_json_schema(value),
                    };
                    (key, value)
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(as_json_schema).collect())
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => value,
    }
}
