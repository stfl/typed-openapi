//! States of typify 0.8.0's merge calls, retaining document identity.
//!
//! Bodies retain the constraints of an operand after its composition is consumed.
//! They cannot re-enter that composition. Negation is separate because a single
//! `not: $ref` does not resolve the reference, while an intersection does.

use std::collections::{BTreeMap, BTreeSet};

use schemars::schema::{ArrayValidation, Schema, SchemaObject, SingleOrVec};

use super::{NodeId, Schemas, annotation_only};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Part {
    Whole(NodeId),
    Body(NodeId),
    Not(NodeId),
}

impl Part {
    fn id(self) -> NodeId {
        match self {
            Self::Whole(id) | Self::Body(id) | Self::Not(id) => id,
        }
    }
}

type Operands = BTreeSet<Part>;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Negative {
    Stated(NodeId),
    Merged(Operands),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum State {
    Merge(Operands),
    Subtract {
        positive: Operands,
        negative: Negative,
    },
}

impl State {
    pub(super) fn start(id: NodeId) -> Self {
        Self::Merge(BTreeSet::from([Part::Whole(id)]))
    }

    pub(super) fn nodes(&self) -> Vec<NodeId> {
        let parts: Vec<_> = match self {
            Self::Merge(parts) => parts.iter().copied().collect(),
            Self::Subtract { positive, negative } => positive
                .iter()
                .copied()
                .chain(match negative {
                    Negative::Stated(id) => vec![Part::Whole(*id)],
                    Negative::Merged(parts) => parts.iter().copied().collect(),
                })
                .collect(),
        };
        // Context bodies persist while a merge resolves a tail. Only whole
        // operands participate in naming the loop, so the tail is not blamed.
        parts
            .into_iter()
            .filter_map(|part| match part {
                Part::Whole(id) | Part::Not(id) => Some(id),
                Part::Body(_) => None,
            })
            .collect()
    }
}

impl Schemas<'_> {
    pub(super) fn transitions(&self, state: &State) -> Vec<State> {
        match state {
            State::Merge(parts) => self.merge(parts),
            State::Subtract { positive, negative } => self.subtract(positive, negative),
        }
    }

    fn object(&self, part: Part) -> Option<&SchemaObject> {
        match (part, self.node(part.id()).schema) {
            (Part::Whole(_) | Part::Body(_), Schema::Object(object)) => Some(object),
            (Part::Not(_), _) | (_, Schema::Bool(_)) => None,
        }
    }

    fn operands(schemas: impl IntoIterator<Item = NodeId>) -> Operands {
        schemas.into_iter().map(Part::Whole).collect()
    }

    fn replacing(parts: &Operands, old: Part, new: impl IntoIterator<Item = Part>) -> Operands {
        parts
            .iter()
            .copied()
            .filter(|part| *part != old)
            .chain(new)
            .collect()
    }

    /// try_merge_schema resolves references before merge_schema_object consumes
    /// the bodies and their top-level composition.
    fn expansion(&self, parts: &Operands) -> Option<Vec<Operands>> {
        if let Some(next) = self.booleans(parts) {
            return Some(next);
        }
        for part in parts {
            if let Part::Whole(id) = part
                && let Some(target) = self.node(*id).target
            {
                return Some(vec![Self::replacing(parts, *part, [Part::Whole(target)])]);
            }
        }
        parts.iter().find_map(|part| {
            let Part::Whole(id) = part else { return None };
            let object = self.object(*part)?;
            object.subschemas.as_ref()?;
            Some(self.composition(parts, *id, object))
        })
    }

    // try_merge_schema short-circuits boolean operands before references.
    fn booleans(&self, parts: &Operands) -> Option<Vec<Operands>> {
        if parts.iter().any(
            |part| matches!(part, Part::Whole(id) if self.node(*id).schema == &Schema::Bool(false)),
        ) {
            return Some(Vec::new());
        }
        let without_true: Operands = parts
            .iter()
            .copied()
            .filter(|part| {
                !matches!(part,
            Part::Whole(id) if self.node(*id).schema == &Schema::Bool(true))
            })
            .collect();
        (without_true.len() != parts.len()).then(|| {
            if without_true.len() < 2 {
                Vec::new()
            } else {
                vec![without_true]
            }
        })
    }

    // try_merge_with_subschemas consumes allOf and applies each not constraint.
    fn composition(&self, parts: &Operands, id: NodeId, object: &SchemaObject) -> Vec<Operands> {
        let Some(sub) = object.subschemas.as_deref() else {
            return Vec::new();
        };
        let mut body = object.clone();
        body.subschemas = None;
        let mut consumed = Vec::new();
        if !annotation_only(body) {
            consumed.push(Part::Body(id));
        }
        consumed.extend(
            sub.all_of
                .iter()
                .flatten()
                .map(|member| Part::Whole(self.id(member))),
        );
        if let Some(not) = &sub.not {
            consumed.push(Part::Not(self.id(not)));
        }
        let base = Self::replacing(parts, Part::Whole(id), consumed);
        match sub.any_of.as_ref().or(sub.one_of.as_ref()) {
            Some(members) => self.alternatives(&base, members),
            None => vec![base],
        }
    }

    // try_merge_with_each_subschema first tries each pair. Its result can carry
    // an allOf with not(other members) back to conversion; retain that path too.
    fn alternatives(&self, base: &Operands, members: &[Schema]) -> Vec<Operands> {
        let mut branches = Vec::new();
        for (at, member) in members.iter().enumerate() {
            let branch: Operands = base
                .iter()
                .copied()
                .chain([Part::Whole(self.id(member))])
                .collect();
            branches.push(branch.clone());
            if members.len() > 1 {
                branches.push(
                    branch
                        .into_iter()
                        .chain(
                            members
                                .iter()
                                .enumerate()
                                .filter(|(other, _)| *other != at)
                                .map(|(_, member)| Part::Not(self.id(member))),
                        )
                        .collect(),
                );
            }
        }
        branches
    }

    fn merge(&self, parts: &Operands) -> Vec<State> {
        let mut next = self.carried(parts);
        if let Some(expanded) = self.expansion(parts) {
            next.extend(expanded.into_iter().map(State::Merge));
            return next;
        }
        // `merge_schema_object` merges object/array bodies before applying not.
        next.extend(self.pairs(parts).into_iter().map(State::Merge));
        for part in parts {
            if let Part::Not(id) = part {
                next.push(State::Subtract {
                    positive: Self::replacing(parts, *part, []),
                    negative: Negative::Stated(*id),
                });
            }
        }
        next
    }

    fn carried(&self, parts: &Operands) -> Vec<State> {
        parts
            .iter()
            .filter(|part| !matches!(part, Part::Not(_)))
            .flat_map(|part| &self.node(part.id()).carries)
            .copied()
            .map(State::start)
            .collect()
    }

    /// `merge_so_object`: shared properties and additionalProperties are merged.
    /// `filter_prop` -> `merge_additional` constructs a deferred allOf when a
    /// property in one operand meets a schema-valued map in another operand.
    fn pairs(&self, parts: &Operands) -> Vec<Operands> {
        let objects: Vec<_> = parts.iter().filter_map(|part| self.object(*part)).collect();
        let mut next = Vec::new();
        let mut properties: BTreeMap<&str, Vec<NodeId>> = BTreeMap::new();
        let mut maps = Vec::new();
        for validation in objects.iter().filter_map(|object| object.object.as_deref()) {
            for (key, schema) in &validation.properties {
                properties.entry(key).or_default().push(self.id(schema));
            }
            if let Some(schema) = &validation.additional_properties {
                maps.push(self.id(schema));
            }
        }
        if maps.len() > 1 {
            next.push(Self::operands(maps));
        }
        for (key, declared) in properties {
            let mut property = declared;
            property.extend(objects.iter().filter_map(|object| {
                let validation = object.object.as_deref()?;
                let schema = (!validation.properties.contains_key(key))
                    .then_some(validation.additional_properties.as_deref())
                    .flatten()?;
                match schema {
                    Schema::Object(_) => Some(self.id(schema)),
                    Schema::Bool(_) => None,
                }
            }));
            if property.len() > 1 {
                next.push(Self::operands(property));
            }
        }
        // merge_so_array -> try_merge_schema, merge_items_array, and
        // merge_additional_items -> merge_additional_properties.
        let arrays: Vec<_> = objects
            .iter()
            .filter_map(|object| object.array.as_deref())
            .collect();
        next.extend(self.array_pairs(&arrays));
        next
    }

    fn array_pairs(&self, arrays: &[&ArrayValidation]) -> Vec<Operands> {
        let tuple_len = arrays
            .iter()
            .filter_map(|array| match &array.items {
                Some(SingleOrVec::Vec(items)) => Some(items.len()),
                Some(SingleOrVec::Single(_)) | None => None,
            })
            .max();
        let slots = tuple_len.unwrap_or(1);
        let limit = arrays
            .iter()
            .filter_map(|array| array.max_items)
            .min()
            .filter(|limit| *limit > 0);
        let slots = if tuple_len.is_some() {
            limit.map_or(slots, |limit| slots.min(limit as usize))
        } else {
            slots
        };
        let mut next = Vec::new();
        for at in 0..slots {
            let schemas: Vec<_> = arrays
                .iter()
                .filter_map(|array| match &array.items {
                    Some(SingleOrVec::Single(schema)) => Some(self.id(schema)),
                    Some(SingleOrVec::Vec(items)) => items
                        .get(at)
                        .or(array.additional_items.as_deref())
                        .map(|schema| self.id(schema)),
                    None => None,
                })
                .collect();
            if schemas.len() > 1 {
                next.push(Self::operands(schemas));
            }
        }
        // merge_so_array's single/tuple tail and merge_additional_items' tuple/
        // tuple tail both call try_merge_schema when the tail is permitted.
        if tuple_len.is_some_and(|len| limit.is_none_or(|limit| len < limit as usize)) {
            let schemas: Vec<_> = arrays
                .iter()
                .filter_map(|array| match &array.items {
                    Some(SingleOrVec::Single(schema)) => Some(self.id(schema)),
                    Some(SingleOrVec::Vec(_)) => array
                        .additional_items
                        .as_deref()
                        .map(|schema| self.id(schema)),
                    None => None,
                })
                .collect();
            if schemas.len() > 1 {
                next.push(Self::operands(schemas));
            }
        }
        // contains is compared, not merged. patternProperties/propertyNames are
        // dropped by merge_so_object (filter_prop asserts they are absent).
        next
    }

    fn subtract(&self, positive: &Operands, negative: &Negative) -> Vec<State> {
        match negative {
            Negative::Stated(id) => self.subtract_stated(positive, *id),
            Negative::Merged(parts) => self.subtract_merged(positive, parts),
        }
    }

    fn subtract_merged(&self, positive: &Operands, parts: &Operands) -> Vec<State> {
        let mut next = vec![State::Merge(parts.clone())];
        if let Some(expanded) = self.expansion(parts) {
            next.extend(expanded.into_iter().map(|parts| State::Subtract {
                positive: positive.clone(),
                negative: Negative::Merged(parts),
            }));
            return next;
        }
        next.extend(self.subtract_properties(positive, parts));
        for part in parts {
            if let Part::Not(id) = part {
                next.push(State::Merge(
                    positive.iter().copied().chain([Part::Whole(*id)]).collect(),
                ));
            }
        }
        next
    }

    /// try_merge_schema_not -> try_merge_schema_object_not: a single reference
    /// is not resolved. Its shared properties call merge_schema_not recursively.
    fn subtract_stated(&self, positive: &Operands, id: NodeId) -> Vec<State> {
        let mut next = self.subtract_properties(positive, &BTreeSet::from([Part::Whole(id)]));
        let Schema::Object(object) = self.node(id).schema else {
            return next;
        };
        let Some(sub) = object.subschemas.as_deref() else {
            return next;
        };
        // try_merge_with_subschemas_not: double negation calls try_merge_schema.
        if let Some(inner) = &sub.not {
            next.push(State::Merge(
                positive
                    .iter()
                    .copied()
                    .chain([Part::Whole(self.id(inner))])
                    .collect(),
            ));
        }
        // not(anyOf) becomes allOf(not ...), then merge_schema_object applies
        // every subtraction. Retain them together so double negations can meet.
        if let Some(members) = &sub.any_of {
            next.push(State::Merge(
                positive
                    .iter()
                    .copied()
                    .chain(members.iter().map(|schema| Part::Not(self.id(schema))))
                    .collect(),
            ));
        }
        // not(allOf) first calls try_merge_all, then subtracts the merged result.
        if let Some(members) = &sub.all_of {
            match members.as_slice() {
                [] => {} // try_merge_all panics under the guard.
                [only] => next.push(State::Subtract {
                    positive: positive.clone(),
                    negative: Negative::Stated(self.id(only)),
                }),
                _ => next.push(State::Subtract {
                    positive: positive.clone(),
                    negative: Negative::Merged(Self::operands(
                        members.iter().map(|schema| self.id(schema)),
                    )),
                }),
            }
        }
        // not(oneOf) and an empty subschema validation return without recursion.
        // if/then/else and combinations not handled by typify panic under guard.
        next
    }

    fn subtract_properties(&self, positive: &Operands, negative: &Operands) -> Vec<State> {
        let mut next = Vec::new();
        let mut properties: BTreeMap<&str, Vec<NodeId>> = BTreeMap::new();
        for validation in positive
            .iter()
            .filter_map(|part| self.object(*part)?.object.as_deref())
        {
            for (key, schema) in &validation.properties {
                properties.entry(key).or_default().push(self.id(schema));
            }
        }
        let mut subtracted: BTreeMap<&str, Vec<NodeId>> = BTreeMap::new();
        for validation in negative
            .iter()
            .filter_map(|part| self.object(*part)?.object.as_deref())
        {
            for (key, schema) in &validation.properties {
                subtracted.entry(key).or_default().push(self.id(schema));
            }
        }
        for (key, schemas) in subtracted {
            let Some(parts) = properties.get(key) else {
                continue;
            };
            let negative = match schemas.as_slice() {
                [only] => Negative::Stated(*only),
                _ => Negative::Merged(Self::operands(schemas)),
            };
            next.push(State::Subtract {
                positive: Self::operands(parts.iter().copied()),
                negative,
            });
        }
        next
    }
}
