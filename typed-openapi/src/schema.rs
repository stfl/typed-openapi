//! Following `$ref`s, and deciding what one schema is worth on a command line.
//!
//! *Requires the `document` feature.*
//!
//! This is the whole of what this crate understands about JSON Schema: a schema
//! either fits on one flag — it is a [`Scalar`] — or it does not, and then the
//! body goes through a file. Nothing richer is modelled, because nothing richer
//! has a command-line spelling.

use openapiv3::{
    Components, ReferenceOr, Schema, SchemaKind, StringFormat, StringType, Type,
    VariantOrUnknownOrEmpty,
};
use thiserror::Error;

use crate::scalar::Scalar;

/// A `$ref` that does not lead anywhere.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("`{reference}` does not resolve")]
pub struct RefError {
    pub reference: String,
}

/// How deep a chain of `$ref`s may go before it is called a cycle.
const MAX_HOPS: usize = 8;

/// Follow `#/components/<section>/<name>` hops until an item appears.
pub fn resolve<'c, T>(
    value: &'c ReferenceOr<T>,
    section: impl Fn(&str) -> Option<&'c ReferenceOr<T>>,
    name: &str,
) -> Result<&'c T, RefError> {
    let prefix = format!("#/components/{name}/");
    let mut current = value;
    for _ in 0..MAX_HOPS {
        match current {
            ReferenceOr::Item(item) => return Ok(item),
            ReferenceOr::Reference { reference } => {
                current = reference
                    .strip_prefix(&prefix)
                    .and_then(&section)
                    .ok_or_else(|| RefError {
                        reference: reference.clone(),
                    })?;
            }
        }
    }
    Err(RefError {
        reference: "a reference cycle".to_owned(),
    })
}

pub fn resolve_schema<'c>(
    schema: &'c ReferenceOr<Schema>,
    components: &'c Components,
) -> Result<&'c Schema, RefError> {
    resolve(schema, |key| components.schemas.get(key), "schemas")
}

/// `Some(scalar)` when this schema fits on one flag, `None` when it does not.
pub fn scalar_of(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<Option<Scalar>, RefError> {
    let schema = resolve_schema(schema, components)?;
    let SchemaKind::Type(ty) = &schema.schema_kind else {
        return Ok(None);
    };
    Ok(match ty {
        Type::String(s) => Some(string_scalar(s)),
        Type::Number(_) => Some(Scalar::Number),
        Type::Integer(_) => Some(Scalar::Integer),
        Type::Boolean(_) => Some(Scalar::Boolean),
        Type::Object(_) | Type::Array(_) => None,
    })
}

/// An enumeration completes; `format: money` is checked; everything else is
/// text, carrying whatever `pattern` the document states for the help line.
fn string_scalar(s: &StringType) -> Scalar {
    let choices: Vec<String> = s.enumeration.iter().flatten().cloned().collect();
    if !choices.is_empty() {
        return Scalar::Choice(choices);
    }
    match &s.format {
        VariantOrUnknownOrEmpty::Unknown(format) if format == "money" => {
            Scalar::Money(s.pattern.clone())
        }
        VariantOrUnknownOrEmpty::Unknown(_)
        | VariantOrUnknownOrEmpty::Empty
        | VariantOrUnknownOrEmpty::Item(
            StringFormat::Date
            | StringFormat::DateTime
            | StringFormat::Password
            | StringFormat::Byte
            | StringFormat::Binary,
        ) => Scalar::Text(s.pattern.clone()),
    }
}

/// `application/json`, `application/merge-patch+json`, and anything with
/// parameters after the essence.
#[must_use]
pub fn is_json(media_type: &str) -> bool {
    essence(media_type) == "application/json" || essence(media_type).ends_with("+json")
}

/// A media type this crate can assemble from `--file` and `--field` parts.
#[must_use]
pub fn is_multipart(media_type: &str) -> bool {
    essence(media_type) == "multipart/form-data"
}

fn essence(media_type: &str) -> String {
    media_type
        .split(';')
        .next()
        .unwrap_or(media_type)
        .trim()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_recognised_through_suffixes_and_parameters() {
        assert!(is_json("application/json"));
        assert!(is_json("application/json; charset=utf-8"));
        assert!(is_json("application/merge-patch+json"));
        assert!(!is_json("form-data"));
        assert!(!is_json("multipart/form-data"));
    }

    #[test]
    fn only_the_correctly_spelled_multipart_type_is_assembled() {
        assert!(is_multipart("multipart/form-data"));
        assert!(is_multipart("Multipart/Form-Data; boundary=x"));
        // The vendor's misspelling. It is not multipart, and the CLI says so
        // rather than guessing what the vendor meant.
        assert!(!is_multipart("form-data"));
    }
}
