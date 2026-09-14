//! Following `$ref`s, and deciding what one schema is worth on a command line.
//!
//! *Requires the `document` feature.*
//!
//! This is the whole of what this crate understands about JSON Schema: a schema
//! either fits on one flag — it is a [`Scalar`] — or it does not, and then the
//! body goes through a file. Nothing richer is modelled, because nothing richer
//! has a command-line spelling.
//!
//! A `$ref` is followed before the schema is read, so a property pointed at a
//! named schema carries that schema's rules onto the flag.

use openapiv3::{
    Components, IntegerType, NumberType, ReferenceOr, Schema, SchemaKind, StringType, Type,
};
use thiserror::Error;

use crate::scalar::{Bounds, Limit, Scalar, Text};

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
        Type::Number(n) => Some(Scalar::Number(number_bounds(n))),
        Type::Integer(i) => Some(Scalar::Integer(integer_bounds(i))),
        Type::Boolean(_) => Some(Scalar::Boolean),
        Type::Object(_) | Type::Array(_) => None,
    })
}

/// An enumeration completes; everything else is text carrying the rules the
/// document states about it.
///
/// `format` is not read at all. A format is a name for a rule, and a name is
/// not a rule: the document that says what an amount looks like says so with
/// `pattern`, which every consumer of the document can run.
fn string_scalar(s: &StringType) -> Scalar {
    let choices: Vec<String> = s.enumeration.iter().flatten().cloned().collect();
    if choices.is_empty() {
        Scalar::Text(Text {
            pattern: s.pattern.clone(),
            min_length: s.min_length,
            max_length: s.max_length,
        })
    } else {
        Scalar::Choice(choices)
    }
}

fn number_bounds(n: &NumberType) -> Bounds<f64> {
    Bounds {
        low: limit(n.minimum, n.exclusive_minimum),
        high: limit(n.maximum, n.exclusive_maximum),
        multiple_of: n.multiple_of,
    }
}

fn integer_bounds(i: &IntegerType) -> Bounds<i64> {
    Bounds {
        low: limit(i.minimum, i.exclusive_minimum),
        high: limit(i.maximum, i.exclusive_maximum),
        multiple_of: i.multiple_of,
    }
}

/// One end of a range. OpenAPI 3.0 states exclusivity as a flag beside the
/// number, so a flag with no number beside it states nothing.
fn limit<T>(value: Option<T>, exclusive: bool) -> Option<Limit<T>> {
    value.map(|value| {
        if exclusive {
            Limit::Exclusive(value)
        } else {
            Limit::Inclusive(value)
        }
    })
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
