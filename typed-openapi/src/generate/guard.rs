//! Panics of typify become refusals when the process unwinds.
//!
//! A named-schema trial holds a schema and every reference it reaches. The
//! smallest trial that panics is named in document order; it can hold several
//! mutually dependent schemas. Stack overflows are checked by `cycles` first.

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
        || trial_name(settings, &kept),
        || space.add_ref_types(definitions),
    )
}

/// The smallest reference-closed trial that panics, in document order.
fn culprit<'d>(
    settings: &TypeSpaceSettings,
    definitions: &'d [(String, Schema)],
) -> Option<Vec<&'d str>> {
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
            caught(|| {
                let mut trial = TypeSpace::new(settings);
                trial.add_ref_types(held)?;
                let _ = trial.to_stream();
                Ok::<_, typify::Error>(())
            })
            .is_err()
        })
        .map(|(_, members)| {
            definitions
                .iter()
                .map(|(name, _)| name.as_str())
                .filter(|name| members.contains(name))
                .collect()
        })
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

fn trial_name(settings: &TypeSpaceSettings, definitions: &[(String, Schema)]) -> String {
    culprit(settings, definitions).map_or_else(
        || "a schema of the document".to_owned(),
        |names| {
            let plural = if names.len() > 1 { "schemas" } else { "schema" };
            let names = names
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("the {plural} {names}")
        },
    )
}

pub(super) fn stream(
    settings: &TypeSpaceSettings,
    space: &TypeSpace,
    definitions: &[(String, Schema)],
    inline: &[(String, Schema)],
) -> Result<proc_macro2::TokenStream, GenerateError> {
    caught(|| space.to_stream()).map_err(|message| GenerateError::Panicked {
        schema: stream_trial(settings, definitions, inline),
        message: message.into(),
    })
}

fn stream_trial(
    settings: &TypeSpaceSettings,
    definitions: &[(String, Schema)],
    inline: &[(String, Schema)],
) -> String {
    if culprit(settings, definitions).is_some() {
        return trial_name(settings, definitions);
    }
    inline
        .iter()
        .find_map(|(name, schema)| {
            inline_panics(settings, definitions, schema).then(|| name.clone())
        })
        .unwrap_or_else(|| "a schema of the document".to_owned())
}

fn inline_panics(
    settings: &TypeSpaceSettings,
    definitions: &[(String, Schema)],
    schema: &Schema,
) -> bool {
    caught(|| {
        let mut trial = TypeSpace::new(settings);
        trial.add_ref_types(definitions.iter().cloned())?;
        trial.add_type(schema)?;
        let _ = trial.to_stream();
        Ok::<_, typify::Error>(())
    })
    .is_err()
}
