//! Merge recursion, checked on the schemas typify receives.
//!
//! A merge follows references through top-level composition and carries merges
//! inside inline properties, items and maps into its result. A loop reachable
//! from a merge is refused before typify's `merge::try_merge_schema` can overflow.
//! Ordinary property and item references remain named recursive types.
//!
//! The walk conservatively refuses a named object merged with a named array
//! whose items merge that object, although typify converts it. Break the loop
//! in an Overlay to repair this shape too.
//!
//! A singleton reference wrapper may have a title, description or `type: object`.
//! The property description idiom is not carried as a merge. An `allOf` with
//! multiple members still starts reference resolution, so even a description-only
//! second member is refused when its composition reaches a loop. The measured rows of `a_loop_that_is_no_merge_generates`
//! hold these exemptions. Other keywords conservatively count as a merge; an
//! Overlay that breaks the loop repairs a refusal even when typify could convert it.

use std::collections::{BTreeMap, BTreeSet};

use schemars::schema::{InstanceType, Schema, SchemaObject, SingleOrVec, SubschemaValidation};

use super::GenerateError;
use super::names::Inline;
use super::types::DEFINITIONS;

struct Node<'a> {
    schema: &'a Schema,
    place: String,
    name: Option<&'a str>,
    edges: Vec<usize>,
}

pub(super) fn check(
    definitions: &[(String, Schema)],
    inline: &[Inline<Schema>],
) -> Result<(), GenerateError> {
    let mut nodes = Vec::new();
    let mut positions = BTreeMap::new();
    let mut named = BTreeMap::new();
    for (name, schema) in definitions {
        named.insert(name.as_str(), nodes.len());
        tree(
            &mut nodes,
            &mut positions,
            schema,
            format!("the schema `{name}`"),
            Some(name),
        );
    }
    for inline in inline {
        tree(
            &mut nodes,
            &mut positions,
            &inline.schema,
            inline.described(),
            None,
        );
    }
    connect(&mut nodes, &positions, &named);
    for (start, node) in nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| starts_merge(node.schema))
    {
        if let Some(path) = cycle(&nodes, start) {
            let held: BTreeSet<_> = path
                .iter()
                .filter_map(|at| nodes.get(*at).and_then(|node| node.name))
                .collect();
            let schemas = definitions
                .iter()
                .filter(|(name, _)| held.contains(name.as_str()))
                .map(|(name, _)| name.clone())
                .collect();
            return Err(GenerateError::CyclicAllOf {
                schemas,
                merge: node.name.is_none().then(|| node.place.clone()),
            });
        }
    }
    Ok(())
}

fn connect(
    nodes: &mut [Node<'_>],
    positions: &BTreeMap<*const Schema, usize>,
    named: &BTreeMap<&str, usize>,
) {
    for node in nodes {
        let schema = node.schema;
        let mut edges = Vec::new();
        if let Schema::Object(object) = schema {
            if let Some(target) = object
                .reference
                .as_deref()
                .and_then(|r| r.strip_prefix(DEFINITIONS))
                .and_then(|name| named.get(name))
            {
                edges.push(*target);
            }
            edges.extend(
                composition(object)
                    .into_iter()
                    .filter_map(|member| positions.get(&std::ptr::from_ref(member)))
                    .copied(),
            );
        }
        // typify copies inline subschemas into the merged type, where each merge
        // is converted again. A plain reference in a property is not followed.
        let mut pending: Vec<_> = children(schema)
            .into_iter()
            .map(|(_, child)| child)
            .collect();
        while let Some(child) = pending.pop() {
            if is_merge(child)
                && let Some(target) = positions.get(&std::ptr::from_ref(child))
            {
                edges.push(*target);
            }
            pending.extend(children(child).into_iter().map(|(_, child)| child));
        }
        node.edges = edges;
    }
}

fn tree<'a>(
    nodes: &mut Vec<Node<'a>>,
    positions: &mut BTreeMap<*const Schema, usize>,
    schema: &'a Schema,
    place: String,
    name: Option<&'a str>,
) {
    let mut pending = vec![(schema, place, name)];
    while let Some((schema, place, name)) = pending.pop() {
        positions.insert(std::ptr::from_ref(schema), nodes.len());
        nodes.push(Node {
            schema,
            place: place.clone(),
            name,
            edges: Vec::new(),
        });
        for (part, child) in children(schema).into_iter().rev() {
            pending.push((child, format!("{place}.{part}"), None));
        }
    }
}

/// Iterative DFS: an active node, rather than an already finished node, closes a loop.
fn cycle(nodes: &[Node<'_>], start: usize) -> Option<Vec<usize>> {
    let mut active = BTreeMap::from([(start, 0)]);
    let mut finished = BTreeSet::new();
    let mut path = vec![(start, 0)];
    while let Some((at, edge)) = path.last_mut() {
        let Some(next) = nodes.get(*at)?.edges.get(*edge).copied() else {
            finished.insert(*at);
            active.remove(at);
            path.pop();
            continue;
        };
        *edge += 1;
        if let Some(offset) = active.get(&next) {
            return Some(path.iter().skip(*offset).map(|(at, _)| *at).collect());
        }
        if !finished.contains(&next) {
            active.insert(next, path.len());
            path.push((next, 0));
        }
    }
    None
}

fn composition(object: &SchemaObject) -> Vec<&Schema> {
    object
        .subschemas
        .as_deref()
        .into_iter()
        .flat_map(|sub| {
            sub.all_of
                .iter()
                .chain(&sub.any_of)
                .chain(&sub.one_of)
                .flatten()
        })
        .collect()
}

// A second member invokes typify's merge even if it only has a description.
// Such an inline wrapper is not carried back into its parent's conversion, but
// a loop of composition reached from it still makes reference resolution recur.
fn starts_merge(schema: &Schema) -> bool {
    let Schema::Object(object) = schema else {
        return false;
    };
    object
        .subschemas
        .as_ref()
        .and_then(|sub| sub.all_of.as_ref())
        .is_some_and(|all| all.len() != 1)
        || is_merge(schema)
}

fn is_merge(schema: &Schema) -> bool {
    let Schema::Object(object) = schema else {
        return false;
    };
    let Some(all) = object
        .subschemas
        .as_ref()
        .and_then(|sub| sub.all_of.as_deref())
    else {
        return false;
    };
    let references = all.iter().filter(|member| member.is_ref()).count();
    references != 1
        || !all.iter().all(|member| {
            let Schema::Object(member) = member else {
                return false;
            };
            let mut rest = member.clone();
            rest.reference = None;
            annotation_only(rest)
        })
        || !annotation_only({
            let mut rest = object.clone();
            if let Some(sub) = rest.subschemas.as_mut() {
                sub.all_of = None;
            }
            rest
        })
}

fn annotation_only(mut object: SchemaObject) -> bool {
    if let Some(metadata) = object.metadata.as_mut() {
        metadata.title = None;
        metadata.description = None;
    }
    if object.metadata.as_deref() == Some(&schemars::schema::Metadata::default()) {
        object.metadata = None;
    }
    if object.instance_type == Some(SingleOrVec::Single(Box::new(InstanceType::Object))) {
        object.instance_type = None;
    }
    if object.subschemas.as_deref() == Some(&SubschemaValidation::default()) {
        object.subschemas = None;
    }
    object == SchemaObject::default()
}

/// Inline schema children, without following references or visiting example data.
pub(super) fn children(schema: &Schema) -> Vec<(String, &Schema)> {
    let Schema::Object(object) = schema else {
        return Vec::new();
    };
    let mut children: Vec<(String, &Schema)> = Vec::new();
    if let Some(sub) = &object.subschemas {
        for (key, members) in [
            ("allOf", &sub.all_of),
            ("anyOf", &sub.any_of),
            ("oneOf", &sub.one_of),
        ] {
            for (at, child) in members.iter().flatten().enumerate() {
                children.push((format!("{key}[{at}]"), child));
            }
        }
        for (key, child) in [
            ("not", &sub.not),
            ("if", &sub.if_schema),
            ("then", &sub.then_schema),
            ("else", &sub.else_schema),
        ] {
            if let Some(child) = child {
                children.push((key.to_owned(), child));
            }
        }
    }
    children.extend(container_children(object));
    children
}

fn container_children(object: &SchemaObject) -> Vec<(String, &Schema)> {
    let mut children: Vec<(String, &Schema)> = Vec::new();
    if let Some(array) = &object.array {
        if let Some(items) = &array.items {
            match items {
                SingleOrVec::Single(child) => children.push(("items".to_owned(), child)),
                SingleOrVec::Vec(items) => children.extend(
                    items
                        .iter()
                        .enumerate()
                        .map(|(at, child)| (format!("items[{at}]"), child)),
                ),
            }
        }
        for (key, child) in [
            ("additionalItems", &array.additional_items),
            ("contains", &array.contains),
        ] {
            if let Some(child) = child {
                children.push((key.to_owned(), child));
            }
        }
    }
    if let Some(object) = &object.object {
        for (key, members) in [
            ("properties", &object.properties),
            ("patternProperties", &object.pattern_properties),
        ] {
            for (name, child) in members {
                children.push((format!("{key}.{name}"), child));
            }
        }
        for (key, child) in [
            ("additionalProperties", &object.additional_properties),
            ("propertyNames", &object.property_names),
        ] {
            if let Some(child) = child {
                children.push((key.to_owned(), child));
            }
        }
    }
    children
}
