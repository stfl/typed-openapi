//! Merge recursion, checked on the schemas typify receives.
//!
//! A state holds the document nodes merged together. Transitions follow typify
//! 0.8.0's merge calls: references, composition, negation, shared container
//! schemas and merges carried into conversion. The iterative DFS retains
//! finished states across starts. Ordinary property/item references are not
//! followed unless another operand merges with them.
//!
//! Conservative refusals include an object merged with an array whose items
//! merge that object, and equal reference operands leading into a composition
//! loop (for example `allOf: [A, A]` with a recursive `oneOf` behind A). typify
//! converts these shapes. Operand identity is not value equality. Break the
//! loop in an Overlay to repair either refusal.
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

mod merge;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct NodeId(usize);

struct Node<'a> {
    schema: &'a Schema,
    place: String,
    name: Option<&'a str>,
    target: Option<NodeId>,
    carries: Vec<NodeId>,
}

struct Schemas<'a> {
    nodes: Vec<Node<'a>>,
    positions: BTreeMap<*const Schema, NodeId>,
}

impl<'a> Schemas<'a> {
    #[expect(clippy::indexing_slicing, reason = "only this arena mints NodeId")]
    fn node(&self, id: NodeId) -> &Node<'a> {
        &self.nodes[id.0]
    }

    fn id(&self, schema: &Schema) -> NodeId {
        position(&self.positions, schema)
    }

    fn tree(&mut self, schema: &'a Schema, place: String, name: Option<&'a str>) {
        let mut pending = vec![(schema, place, name)];
        while let Some((schema, place, name)) = pending.pop() {
            self.positions
                .insert(std::ptr::from_ref(schema), NodeId(self.nodes.len()));
            self.nodes.push(Node {
                schema,
                place: place.clone(),
                name,
                target: None,
                carries: Vec::new(),
            });
            for (part, child) in children(schema).into_iter().rev() {
                pending.push((child, format!("{place}.{part}"), None));
            }
        }
    }

    fn connect(&mut self, named: &BTreeMap<&str, NodeId>) {
        for node in &mut self.nodes {
            if let Schema::Object(object) = node.schema {
                node.target = object
                    .reference
                    .as_deref()
                    .and_then(|r| r.strip_prefix(DEFINITIONS))
                    .and_then(|name| named.get(name))
                    .copied();
            }
            node.carries = carried(node.schema, &self.positions);
        }
    }
}

fn position(positions: &BTreeMap<*const Schema, NodeId>, schema: &Schema) -> NodeId {
    positions[&std::ptr::from_ref(schema)]
}

// Conversion of a merged result starts each carried inline merge. Reference
// children remain named types; only inline trees are read.
fn carried(schema: &Schema, positions: &BTreeMap<*const Schema, NodeId>) -> Vec<NodeId> {
    let mut found = Vec::new();
    let mut pending: Vec<_> = children(schema)
        .into_iter()
        .map(|(_, child)| child)
        .collect();
    while let Some(child) = pending.pop() {
        if is_merge(child) {
            found.push(position(positions, child));
        }
        pending.extend(children(child).into_iter().map(|(_, child)| child));
    }
    found
}

pub(super) fn check(
    definitions: &[(String, Schema)],
    inline: &[Inline<Schema>],
) -> Result<(), GenerateError> {
    let mut graph = Schemas {
        nodes: Vec::new(),
        positions: BTreeMap::new(),
    };
    let mut named = BTreeMap::new();
    for (name, schema) in definitions {
        named.insert(name.as_str(), NodeId(graph.nodes.len()));
        graph.tree(schema, format!("the schema `{name}`"), Some(name));
    }
    for inline in inline {
        graph.tree(&inline.schema, inline.described(), None);
    }
    graph.connect(&named);
    // Finished states are shared by every start, including carried merges.
    let mut finished = BTreeSet::new();
    for (start, node) in graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| starts_merge(node.schema))
    {
        if let Some(path) = cycle(&graph, NodeId(start), &mut finished) {
            let held: BTreeSet<_> = path
                .iter()
                .flat_map(merge::State::nodes)
                .filter_map(|id| graph.node(id).name)
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

struct Frame {
    state: merge::State,
    next: std::vec::IntoIter<merge::State>,
}

/// An active merge state closes a loop; a finished state is never expanded again.
fn cycle(
    graph: &Schemas<'_>,
    start: NodeId,
    finished: &mut BTreeSet<merge::State>,
) -> Option<Vec<merge::State>> {
    let start = merge::State::start(start);
    if finished.contains(&start) {
        return None;
    }
    let mut active = BTreeMap::from([(start.clone(), 0)]);
    let mut path = vec![Frame {
        next: graph.transitions(&start).into_iter(),
        state: start,
    }];
    while let Some(frame) = path.last_mut() {
        let Some(next) = frame.next.next() else {
            finished.insert(frame.state.clone());
            active.remove(&frame.state);
            path.pop();
            continue;
        };
        if let Some(offset) = active.get(&next) {
            return Some(
                path.iter()
                    .skip(*offset)
                    .map(|frame| frame.state.clone())
                    .collect(),
            );
        }
        if !finished.contains(&next) {
            active.insert(next.clone(), path.len());
            path.push(Frame {
                next: graph.transitions(&next).into_iter(),
                state: next,
            });
        }
    }
    None
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
