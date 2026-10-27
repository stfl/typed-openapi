//! typify's panics, as refusals that name the schema.
//!
//! typify panics where it has no type to write for a shape, and the shapes are
//! the vendor's: enum values that become one variant name (`active` and
//! `ACTIVE`), a `$ref` that leads nowhere. A panic ends a bless step with exit 101
//! and a line of typify's own source, after the reduction has accepted the
//! document, so it names no schema and no repair. [`GenerateError::Panicked`] is
//! the same event with the schema in it.
//!
//! The rule typify applies is its own and is not public (`sanitize` is private to
//! `typify-impl`), and a copy of it here would be the second spelling of a rule
//! that [`names`](super::names) exists to keep this crate from owning. So the
//! generator asks typify: it converts, and where typify panics it converts again,
//! one schema at a time, to find the schema.
//!
//! A stack overflow is not a panic, and nothing here sees it:
//! [`cycles`](super::cycles) finds the one shape of the document that causes it.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{self, AssertUnwindSafe};

use schemars::schema::Schema;
use typify::{TypeSpace, TypeSpaceSettings};

use super::GenerateError;
use super::types::refers_to;

/// `convert`, and what it panicked with where it did.
///
/// The message is the payload of the panic when that is text, which is what
/// `panic!` makes of its arguments.
pub(super) fn caught<T>(convert: impl FnOnce() -> T) -> Result<T, String> {
    panic::catch_unwind(AssertUnwindSafe(convert)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "typify panicked with no message".to_owned())
    })
}

/// One conversion of one schema: an error of typify is [`GenerateError::Typify`]
/// and a panic of it is [`GenerateError::Panicked`] for `schema`.
///
/// `schema` is built only where typify panicked, because the sentence is for a
/// person and nothing else reads it.
pub(super) fn converted<T>(
    schema: impl FnOnce() -> String,
    convert: impl FnOnce() -> Result<T, typify::Error>,
) -> Result<T, GenerateError> {
    match caught(convert) {
        Ok(converted) => converted.map_err(|error| GenerateError::Typify(Box::new(error))),
        Err(message) => Err(GenerateError::Panicked {
            schema: schema(),
            message: message.into(),
        }),
    }
}

/// `definitions` added to `space`, which converts all of them in one call.
///
/// typify takes the whole set at once, because a schema may refer to one that
/// stands after it, so a panic does not say which of them it was converting. It is
/// found by [`culprit`].
pub(super) fn add_ref_types(
    settings: &TypeSpaceSettings,
    space: &mut TypeSpace,
    definitions: Vec<(String, Schema)>,
) -> Result<(), GenerateError> {
    let kept = definitions.clone();
    converted(
        || {
            culprit(settings, &kept).map_or_else(
                || "a schema of the document".to_owned(),
                |name| format!("the schema `{name}`"),
            )
        },
        || space.add_ref_types(definitions),
    )
}

/// The first schema that typify panics on when it is converted with every schema it
/// refers to, among the schemas that refer to the fewest.
///
/// Each trial is a space of its own that holds one schema and what it refers to,
/// at any depth, because typify reads what a reference leads to (an `allOf` merges
/// the schemas of its members) and nothing else of the document. The trial of a
/// schema that refers to one that panics panics too, so the schemas are tried by
/// the number of schemas in the trial, fewest first, and the one that panics first
/// is the one that holds the cause. Equal numbers keep the document's order.
fn culprit<'d>(
    settings: &TypeSpaceSettings,
    definitions: &'d [(String, Schema)],
) -> Option<&'d str> {
    let named: BTreeMap<&str, &Schema> = definitions
        .iter()
        .map(|(name, schema)| (name.as_str(), schema))
        .collect();
    let mut trials: Vec<(&str, Vec<&str>)> = definitions
        .iter()
        .map(|(name, _)| (name.as_str(), closure(name, &named)))
        .collect();
    trials.sort_by_key(|(_, members)| members.len());
    trials
        .into_iter()
        .find(|(_, members)| {
            let held = members.iter().filter_map(|member| {
                named
                    .get(member)
                    .map(|schema| ((*member).to_owned(), (*schema).clone()))
            });
            caught(|| TypeSpace::new(settings).add_ref_types(held)).is_err()
        })
        .map(|(name, _)| name)
}

/// `name` and every schema that it refers to, at any depth, as names of the
/// document. A reference that leads out of the document is left out.
fn closure<'d>(name: &'d str, named: &BTreeMap<&'d str, &Schema>) -> Vec<&'d str> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut members = Vec::new();
    let mut pending = vec![name];
    while let Some(next) = pending.pop() {
        let Some((held, schema)) = named.get_key_value(next) else {
            continue;
        };
        if !seen.insert(held) {
            continue;
        }
        members.push(*held);
        for referred in refers_to(schema) {
            if let Some((held, _)) = named.get_key_value(referred.as_str()) {
                pending.push(held);
            }
        }
    }
    members
}
