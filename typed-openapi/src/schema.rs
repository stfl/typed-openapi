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
//!
//! Two other things a schema says travel with those rules and are not rules
//! themselves: the sentence that describes it, and the `format` that names what
//! kind of value it is. [`description_of`] and [`format_of`] read them.
//!
//! [`template`] is the other half of the same reading. A body that does not fit
//! on flags goes through a file, and the shape of that file is the one thing
//! `--help` cannot state — so the walk that decided the body was not flat also
//! writes down what it saw, as the JSON skeleton a user fills in.

use std::collections::BTreeSet;

use openapiv3::{
    ArrayType, Components, IntegerType, NumberType, ObjectType, ReferenceOr, Schema, SchemaKind,
    StringType, Type, VariantOrUnknownOrEmpty,
};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::scalar::{Bounds, Limit, Scalar, Text};

/// A `$ref` this crate cannot follow to what it names.
///
/// A reference nothing answers and a reference that leads back to itself are two
/// things to say, and worth saying apart: the first is a name to go and look
/// for, the second a document describing a value of no finite depth. Saying
/// `cycle` where there is none sends an adopter looking for something their
/// document does not contain.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RefError {
    /// A reference naming nothing this document holds.
    #[error("`{reference}` does not resolve")]
    Missing { reference: String },
    /// A reference reached a second time while following one chain.
    #[error("`{reference}` is a reference cycle")]
    Cycle { reference: String },
}

/// How deep a body template spells the document out.
///
/// A property that points back at the schema holding it — a voucher whose
/// parent is a voucher — describes a value of no finite depth, so the walk
/// wants a floor rather than a way to recognise that one case: a cycle and an
/// honestly deep document stop in the same place, and neither hands a user a
/// template that scrolls past what they came to read. Eight levels is past
/// anything read in one sitting, and what stands at the floor is the empty
/// object or the empty list — the template says a value belongs here and stops
/// spelling it out, which is the most it can truthfully say.
const MAX_DEPTH: usize = 8;

/// Follow `#/components/<section>/<name>` hops until an item appears.
pub fn resolve<'c, T>(
    value: &'c ReferenceOr<T>,
    section: impl Fn(&str) -> Option<&'c ReferenceOr<T>>,
    name: &str,
) -> Result<&'c T, RefError> {
    follow(value, &section, name, &mut BTreeSet::new())
}

/// The same walk, with the references already taken handed in.
///
/// What terminates it is that set and not a count of the hops, which is the
/// argument [`crate::required`] makes about its own walk and it holds here for
/// the same reason. A count is a floor under how deep a *legal* document may
/// go: a chain of nine references is finite, resolves, and describes one value,
/// and a limit of eight refuses it while telling the adopter their document has
/// a cycle it does not contain. The set terminates on the cycle itself, and
/// truncates nothing — so the word `cycle` is true wherever it appears.
///
/// [`stated`] hands in one set across every hop it makes, because a composition
/// it unwraps is the same chain seen through `allOf`: a schema whose single
/// member leads back to it returns to a reference this walk has already taken,
/// and only a set spanning both sees that.
fn follow<'c, T>(
    value: &'c ReferenceOr<T>,
    section: &impl Fn(&str) -> Option<&'c ReferenceOr<T>>,
    name: &str,
    seen: &mut BTreeSet<&'c str>,
) -> Result<&'c T, RefError> {
    let prefix = format!("#/components/{name}/");
    let mut current = value;
    loop {
        match current {
            ReferenceOr::Item(item) => return Ok(item),
            ReferenceOr::Reference { reference } => {
                if !seen.insert(reference.as_str()) {
                    return Err(RefError::Cycle {
                        reference: reference.clone(),
                    });
                }
                current = reference
                    .strip_prefix(&prefix)
                    .and_then(section)
                    .ok_or_else(|| RefError::Missing {
                        reference: reference.clone(),
                    })?;
            }
        }
    }
}

pub fn resolve_schema<'c>(
    schema: &'c ReferenceOr<Schema>,
    components: &'c Components,
) -> Result<&'c Schema, RefError> {
    resolve(schema, schemas(components), "schemas")
}

/// The section every schema reference in this module resolves against.
///
/// One closure rather than one per call site, because [`stated`] hands the same
/// one to every hop of the walk it shares a `seen` set with.
fn schemas<'c>(components: &'c Components) -> impl Fn(&str) -> Option<&'c ReferenceOr<Schema>> {
    move |key| components.schemas.get(key)
}

/// The schema that states the rules: `$ref` hops followed, and a single-element
/// `allOf` read as the element it wraps.
///
/// OpenAPI 3.0 has a `$ref` erase everything written beside it, so a document
/// that both names a rule and says something about the field pointing at it has
/// exactly one spelling for the pair — the reference wrapped in an `allOf` of
/// one element, with the sentence outside the wrapper. That wrapper composes
/// nothing; it is a `$ref` that kept its siblings, and what it describes is
/// what its element describes.
///
/// One element is where this stops. An `allOf` of two schemas is a real
/// composition, and a composition is not a scalar: it comes back as itself, and
/// [`scalar_of`] answers `None` for it.
///
/// Unwrapping and following are one walk, so they share one set of the
/// references taken: a schema whose only member leads back to the schema is a
/// value of no finite depth however many wrappers stand between the two ends,
/// and a set spanning both is what meets it. Nothing bounds the number of
/// wrappers, because every one of them that is not a reference is a step
/// further into a document that is finite.
fn stated<'c>(
    schema: &'c ReferenceOr<Schema>,
    components: &'c Components,
) -> Result<&'c Schema, RefError> {
    let schemas = schemas(components);
    let mut seen = BTreeSet::new();
    let mut current = follow(schema, &schemas, "schemas", &mut seen)?;
    loop {
        let SchemaKind::AllOf { all_of } = &current.schema_kind else {
            return Ok(current);
        };
        let [only] = all_of.as_slice() else {
            return Ok(current);
        };
        current = follow(only, &schemas, "schemas", &mut seen)?;
    }
}

/// `Some(scalar)` when this schema fits on one flag, `None` when it does not.
pub fn scalar_of(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<Option<Scalar>, RefError> {
    let schema = stated(schema, components)?;
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

/// What a property says about itself, and what it inherits by pointing
/// somewhere else.
///
/// The field's own sentence wins: it is about this field, where the named
/// schema's is about every field that shares the rule. A field that says
/// nothing of its own takes the named schema's, which is better than nothing
/// and is all a bare `$ref` can leave behind.
pub fn description_of(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<Option<String>, RefError> {
    let own = &resolve_schema(schema, components)?.schema_data.description;
    if own.is_some() {
        return Ok(own.clone());
    }
    Ok(stated(schema, components)?.schema_data.description.clone())
}

/// The `format` the stated schema declares, in the document's own spelling.
///
/// The preference is the opposite of [`description_of`]'s, and deliberately so.
/// A description is about *this field*, so the field's own sentence wins; a
/// format is about the kind of value, which is the named schema's business —
/// one `Money` says what an amount is for every field that points at it, and a
/// field that redescribed its kind would be describing a different value from
/// the one it references. Reading it off the stated schema is also what leaves
/// an adoption one vocabulary instead of two: that is the schema a generator
/// hands typify, so the Rust type a format stands for and the format the
/// reduced model reports come off the same node.
///
/// Nothing in this crate acts on the answer, which is what keeps a format from
/// becoming a rule by the back door: [`Scalar`] carries none of this, so
/// `Scalar::parse` cannot read it and `Scalar::note` cannot advertise it. What
/// it is for is an adopter asking which of an operation's values are of a kind
/// the document names — a question only the document can settle and only the
/// adopter can answer.
pub fn format_of(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<Option<String>, RefError> {
    Ok(match &stated(schema, components)?.schema_kind {
        SchemaKind::Type(Type::String(s)) => as_written(&s.format),
        SchemaKind::Type(Type::Number(n)) => as_written(&n.format),
        SchemaKind::Type(Type::Integer(i)) => as_written(&i.format),
        // A format names the kind of a *value*, and none of these is one: an
        // object and an array hold values rather than being one, a boolean has
        // only two, and a composition is several schemas rather than a shape.
        SchemaKind::Type(Type::Object(_) | Type::Array(_) | Type::Boolean(_))
        | SchemaKind::OneOf { .. }
        | SchemaKind::AllOf { .. }
        | SchemaKind::AnyOf { .. }
        | SchemaKind::Not { .. }
        | SchemaKind::Any(_) => None,
    })
}

/// One `format` keyword as the document spells it.
///
/// `openapiv3` reads the handful of formats OpenAPI itself names into variants
/// and leaves every other one the string it was, so the spelling comes back
/// through `serde` rather than from a table here. A table would be a second
/// copy of those names, free to disagree with the first over `date-time`; the
/// serialisation is the one the document was parsed against.
fn as_written<T: Serialize>(format: &VariantOrUnknownOrEmpty<T>) -> Option<String> {
    match serde_json::to_value(format) {
        Ok(serde_json::Value::String(spelling)) => Some(spelling),
        Ok(_) | Err(_) => None,
    }
}

/// The example a schema states about itself, and the one it inherits by
/// pointing somewhere else.
///
/// Read the way [`description_of`] reads a description, and for the same
/// reason: a value stated about *this* field is about this field, where a named
/// schema's is about every field that shares the rule.
fn example_of<'c>(
    schema: &'c ReferenceOr<Schema>,
    components: &'c Components,
) -> Result<Option<&'c Value>, RefError> {
    let own = &resolve_schema(schema, components)?.schema_data.example;
    if own.is_some() {
        return Ok(own.as_ref());
    }
    Ok(stated(schema, components)?.schema_data.example.as_ref())
}

/// A skeleton of one JSON body, as the text a user redirects into a file.
///
/// The body that goes through a file is the body with no per-field flags, and
/// the flags are where this crate says what a field is called and what it
/// accepts — so that body is the one a `--help` page cannot describe. This is
/// what the walk that decided it was not flat saw on the way.
///
/// `None` where there is no shape to write down: a body the document gives no
/// schema, or one whose schema is a composition this crate has no reading for.
/// The absence travels, and a subcommand with nothing to print grows no flag to
/// ask for it.
///
/// Rendered once, while the document is reduced, and carried as the text it
/// renders to. A shipped binary prints it and reads nothing — the rule both
/// command names already follow.
pub fn template(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<Option<String>, RefError> {
    let skeleton = skeleton(schema, components, 0)?;
    if skeleton.is_null() {
        return Ok(None);
    }
    Ok(serde_json::to_string_pretty(&skeleton).ok())
}

/// One schema as the emptiest value that fits it.
///
/// `Value::Null` is what this says about a schema it has no reading for — a
/// composition, or a node that states nothing at all. Under a required property
/// that is the honest placeholder for a key the caller must supply and this
/// crate cannot describe; at the top it is what [`template`] reads as having no
/// template to offer.
fn skeleton(
    schema: &ReferenceOr<Schema>,
    components: &Components,
    depth: usize,
) -> Result<Value, RefError> {
    // An `example` is instance data, so it is taken whole and never read as a
    // schema. A vendor whose example spells out a form as `{"required": [...]}`
    // is writing a value that happens to use those words, and descending into
    // it would build a template out of somebody's sample. It comes first
    // because it is the better source: a value the document states round-trips,
    // where one derived from types alone is only a shape.
    if let Some(example) = example_of(schema, components)? {
        return Ok(example.clone());
    }
    if let Some(scalar) = scalar_of(schema, components)? {
        return Ok(empty(&scalar));
    }
    let node = stated(schema, components)?;
    if let SchemaKind::Type(Type::Object(object)) = &node.schema_kind {
        return object_skeleton(object, components, depth);
    }
    if let SchemaKind::Type(Type::Array(array)) = &node.schema_kind {
        return array_skeleton(array, components, depth);
    }
    Ok(Value::Null)
}

/// The required properties, in the order the document declares them, and
/// nothing else.
///
/// An optional key carrying an empty value is a key the caller never asked to
/// send: on a `PUT` it is an empty string written over a field somebody meant
/// to leave alone, and JSON has no comment to mark it as a suggestion with. So
/// a template carries the minimum the document demands and the document stays
/// where the rest is stated — the default-closed instinct the write gate has,
/// applied to a body. A schema that requires nothing renders as `{}`, which is
/// exactly what it asks of a caller.
fn object_skeleton(
    object: &ObjectType,
    components: &Components,
    depth: usize,
) -> Result<Value, RefError> {
    let mut out = serde_json::Map::new();
    if depth >= MAX_DEPTH {
        return Ok(Value::Object(out));
    }
    for (name, property) in &object.properties {
        if !object.required.iter().any(|required| required == name) {
            continue;
        }
        out.insert(
            name.clone(),
            skeleton(&property.clone().unbox(), components, depth + 1)?,
        );
    }
    Ok(Value::Object(out))
}

/// One element rather than none.
///
/// An empty list is a body the server accepts and the user learns nothing
/// from, and what goes *in* the list is exactly what they came here to find
/// out. One element, itself a skeleton, says that much and is written the same
/// way as every other value in the template. A list whose `items` the document
/// omits is the one case with nothing to put in it.
fn array_skeleton(
    array: &ArrayType,
    components: &Components,
    depth: usize,
) -> Result<Value, RefError> {
    if depth >= MAX_DEPTH {
        return Ok(Value::Array(Vec::new()));
    }
    let Some(items) = &array.items else {
        return Ok(Value::Array(Vec::new()));
    };
    Ok(Value::Array(vec![skeleton(
        &items.clone().unbox(),
        components,
        depth + 1,
    )?]))
}

/// The emptiest value of one kind.
///
/// Empty and zero where nothing better is known, on purpose: `""` and `0` claim
/// no more than that a value belongs here, so a key nobody filled in reads as a
/// skeleton and not as a suggestion. Against a `pattern` or a `minimum` they
/// are values the document itself rules out, and a server handed one refuses
/// it.
///
/// An enumeration is the one kind with no empty member, so it shows the first
/// value the document lists. A value the enumeration does not list would be a
/// lie about the API, and there is nothing else to show.
///
/// That floor is not a promise that a template cannot be sent. An
/// enumeration's first value is one the enumeration admits, and the `example`
/// [`skeleton`] takes whole is one the document states about the API — so a
/// template drawing every key from those two carries nothing the document
/// forbids. A template is checked against nothing and is not meant to go out
/// unread: it says where the values belong, and the values are the caller's to
/// put there.
fn empty(scalar: &Scalar) -> Value {
    match scalar {
        Scalar::Text(_) => Value::String(String::new()),
        Scalar::Integer(_) => Value::from(0),
        Scalar::Number(_) => Value::from(0.0),
        Scalar::Boolean => Value::Bool(false),
        Scalar::Choice(values) => values
            .first()
            .map_or(Value::Null, |first| Value::String(first.clone())),
    }
}

/// An enumeration completes; everything else is text carrying the rules the
/// document states about it.
///
/// No `format` reaches a [`Scalar`]. A format is a name for a rule, and a name
/// is not a rule: the document that says what an amount looks like says so with
/// `pattern`, which every consumer of the document can run. The name travels
/// beside the rules rather than among them — [`format_of`] is where it is read.
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

/// Whether this is a media type at all: `type/subtype`, with optional
/// parameters after a `;`.
///
/// The question every other one here presumes. A `content` key with no `/` in
/// it names nothing a server can read, so a body sent under it cannot arrive —
/// which makes it a shape to refuse rather than one to carry.
///
/// Only the essence is held to a grammar, and it is RFC 9110's own: two
/// non-empty `token`s either side of one `/`. That is deliberately the whole
/// of the rule. The parameters after the `;` are not checked, because a
/// parameter value may be a quoted string carrying a `;` or a `/`, and a rule
/// strict enough to judge one would refuse bodies servers accept — the defect
/// this question exists to catch is a key that was never a media type, not a
/// parameter spelled unusually. The token rule is the wire's, not a register
/// of types this crate knows: `application/x-www-form-urlencoded`,
/// `application/vnd.api+json` and every vendor type anyone coins pass it.
#[must_use]
pub fn is_media_type(media_type: &str) -> bool {
    essence(media_type)
        .split_once('/')
        .is_some_and(|(ty, subtype)| is_token(ty) && is_token(subtype))
}

/// RFC 9110's `token`: one or more of the characters a field value carries
/// unquoted. `/` is not one of them, which is what makes the split above the
/// whole of the parse.
fn is_token(word: &str) -> bool {
    !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
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

    /// The grammar has to be wide enough to admit everything a server reads
    /// and narrow enough to catch a key that names no type at all. The first
    /// half is the one worth guarding: a rule that turned away
    /// `application/x-www-form-urlencoded` would be worse than the defect.
    #[test]
    fn a_media_type_is_a_type_and_a_subtype_and_a_bare_word_is_neither() {
        assert!(is_media_type("application/pdf"));
        assert!(is_media_type("text/csv; charset=utf-8"));
        assert!(is_media_type("application/vnd.api+json"));
        assert!(is_media_type("multipart/form-data; boundary=x"));
        assert!(is_media_type("application/x-www-form-urlencoded"));
        // A parameter is the vendor's to spell, and is read no further than
        // the `;` that starts it.
        assert!(is_media_type(r#"multipart/form-data; boundary="a/b;c""#));

        // The vendor's misspelling: one word, and a word is not a type over a
        // subtype.
        assert!(!is_media_type("form-data"));
        assert!(!is_media_type(""));
        assert!(!is_media_type("application/"));
        assert!(!is_media_type("/json"));
        assert!(!is_media_type("application/ld/json"));
        // A space is not a `token` character, so neither of these is one type.
        assert!(!is_media_type("application/json charset=utf-8"));
    }

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
