//! A cycle of `allOf` among the named schemas, found before typify can run on it.
//!
//! typify merges the members of an `allOf` by following each `$ref` in them to
//! the schema it names, and merging that schema with the rest. Its source says
//! what happens where the references lead back: "need to mitigate circular
//! references so we don't go into a spin loop"
//! (`typify-impl`'s `merge::try_merge_schema`). The loop is a recursion, and the
//! process dies of a stack overflow, which `catch_unwind` does not see, on a
//! document that the reduction loads. So the generator reads the cycle off the
//! document and refuses it by the names of its schemas.
//!
//! # What is a cycle
//!
//! A schema has an edge to each schema that it refers to through its
//! composition: the `$ref`s that stand in its `allOf`, `anyOf` and `oneOf`
//! members and in the members of theirs, at any depth. A `$ref` in a property,
//! in the items of a list or in `additionalProperties` is no edge, because a
//! reference that closes a loop there is a recursive type, which typify writes.
//!
//! An edge is a *merge* where an `allOf` stands above the reference that is not a
//! bare wrapper: an `allOf` of two or more members, or of one beside other keywords
//! of the schema, and every composition below it. An `allOf` of one member and
//! nothing else is the reference, which typify reads as a newtype with no merge.
//!
//! A cycle is refused where one of its edges is a merge. typify was measured on
//! `allOf` of two members that lead to each other, to themselves, in a longer
//! cycle, inside a nested `allOf`, inside a `oneOf` or an `anyOf` of one, beside a
//! wrapper of one element and beside a `oneOf`; each one ends in a stack overflow.
//! A loop of bare wrappers, of `oneOf`s alone and one that runs through a property
//! or a list converts. The idiom that puts a sentence beside a reference, an
//! `allOf` of the schema and `{description: …}` in a property of the schema,
//! converts too, so a property is no edge.
//!
//! # What is not found
//!
//! A property, or the items of a list, whose own `allOf` adds keywords and names
//! the schema that holds it (`A.p` is `allOf: [{$ref: A}, {properties: …}]`)
//! overflows too, and is not found here. typify merges the two, and the merged
//! type holds the same property again, so its conversion never ends: the recursion
//! is in a type that typify builds out of the merge and not along a reference that
//! the document spells out. A member that adds nothing but a sentence is the
//! idiom above and converts, so the line between the two is typify's reading of
//! what a merge adds, which this crate does not copy. Measured, and left to the
//! next release that moves typify.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use schemars::schema::{Schema, SchemaObject, SubschemaValidation};

use super::GenerateError;
use super::types::DEFINITIONS;

/// One reference that a schema's composition makes, and whether an `allOf` merges
/// it.
struct Edge<'d> {
    to: &'d str,
    merged: bool,
}

/// Refuses the first cycle with a merge in it, in the order of the document.
pub(super) fn check(definitions: &[(String, Schema)]) -> Result<(), GenerateError> {
    let names: Vec<&str> = definitions.iter().map(|(name, _)| name.as_str()).collect();
    let known: BTreeSet<&str> = names.iter().copied().collect();
    let edges: BTreeMap<&str, Vec<Edge<'_>>> = definitions
        .iter()
        .map(|(name, schema)| {
            let mut edges = Vec::new();
            composed(schema, false, &mut edges);
            edges.retain(|edge| known.contains(edge.to));
            (name.as_str(), edges)
        })
        .collect();
    for name in &names {
        let Some(leaving) = edges.get(name) else {
            continue;
        };
        for merge in leaving.iter().filter(|edge| edge.merged) {
            if let Some(path) = path_back(&edges, merge.to, name) {
                return Err(GenerateError::CyclicAllOf {
                    schemas: in_document_order(&names, name, &path),
                });
            }
        }
    }
    Ok(())
}

/// The shortest way from `from` to `to`, as the schemas on it with both ends in,
/// and none where `to` cannot be reached.
fn path_back<'d>(
    edges: &BTreeMap<&'d str, Vec<Edge<'d>>>,
    from: &'d str,
    to: &str,
) -> Option<Vec<&'d str>> {
    let mut came_from: BTreeMap<&str, Option<&str>> = BTreeMap::from([(from, None)]);
    let mut pending = VecDeque::from([from]);
    while let Some(at) = pending.pop_front() {
        if at == to {
            let mut path = vec![at];
            let mut back = at;
            while let Some(Some(before)) = came_from.get(back) {
                path.push(before);
                back = before;
            }
            path.reverse();
            return Some(path);
        }
        for edge in edges.get(at).into_iter().flatten() {
            if !came_from.contains_key(edge.to) {
                came_from.insert(edge.to, Some(at));
                pending.push_back(edge.to);
            }
        }
    }
    None
}

/// The schemas of the cycle, starting with the one that comes first in the
/// document, in the order that `allOf` leads from one to the next.
///
/// `start` is the schema whose merge closed the cycle and `path` leads from the
/// schema that merge names back to it, so the cycle is `start` and then `path`
/// without its last schema, which is `start` again.
fn in_document_order(names: &[&str], start: &str, path: &[&str]) -> Vec<String> {
    let mut cycle: Vec<&str> = vec![start];
    cycle.extend(path.iter().copied().take(path.len().saturating_sub(1)));
    let first = cycle
        .iter()
        .enumerate()
        .min_by_key(|(_, schema)| names.iter().position(|name| name == *schema))
        .map_or(0, |(at, _)| at);
    cycle.rotate_left(first);
    cycle.into_iter().map(str::to_owned).collect()
}

/// Every schema that `schema` refers to through its composition, with whether an
/// `allOf` merges the reference. `merging` is whether one stands above `schema`.
fn composed<'d>(schema: &'d Schema, merging: bool, edges: &mut Vec<Edge<'d>>) {
    let Schema::Object(object) = schema else {
        return;
    };
    if let Some(name) = object
        .reference
        .as_deref()
        .and_then(|reference| reference.strip_prefix(DEFINITIONS))
    {
        edges.push(Edge {
            to: name,
            merged: merging,
        });
    }
    let Some(subschemas) = &object.subschemas else {
        return;
    };
    if let Some(all_of) = &subschemas.all_of {
        let merges = merging || !is_a_wrapper(object, all_of);
        for member in all_of {
            composed(member, merges, edges);
        }
    }
    for member in subschemas.any_of.iter().chain(&subschemas.one_of).flatten() {
        composed(member, merging, edges);
    }
}

/// Whether `object` is an `allOf` of one member and nothing else, which typify
/// reads as the member.
fn is_a_wrapper(object: &SchemaObject, all_of: &[Schema]) -> bool {
    if all_of.len() != 1 {
        return false;
    }
    let mut rest = object.clone();
    rest.metadata = None;
    rest.extensions.clear();
    if let Some(subschemas) = rest.subschemas.as_mut() {
        subschemas.all_of = None;
    }
    if rest.subschemas.as_deref() == Some(&SubschemaValidation::default()) {
        rest.subschemas = None;
    }
    rest == SchemaObject::default()
}
