//! Following `$ref`s, and deciding what one schema is worth on a command line.
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

use http::HeaderValue;
use openapiv3::{
    ArrayType, BooleanType, Components, IntegerType, NumberType, ObjectType, ReferenceOr, Schema,
    SchemaKind, StringType, Type, VariantOrUnknownOrEmpty,
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
    Missing {
        /// The `$ref` as the document spells it, such as
        /// `#/components/schemas/Voucher`.
        reference: String,
    },
    /// A reference into the inside of a component this document holds, such as
    /// `#/components/schemas/Holder/properties/code`. This crate follows a
    /// component and goes no further in. A reference that goes inside a name
    /// that no component has is [`RefError::Missing`].
    #[error(
        "`{reference}` points inside a component, and this crate follows only \
         `#/components/<section>/<name>`"
    )]
    Inside {
        /// The `$ref` as the document spells it.
        reference: String,
    },
    /// A reference reached a second time while following one chain.
    #[error("`{reference}` is a reference cycle")]
    Cycle {
        /// The `$ref` that was reached a second time, spelled as the document
        /// spells it.
        reference: String,
    },
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
pub(crate) fn resolve<'c, T>(
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
                // The component is what comes before the first `/`, and it is
                // looked up before the rest is read: a pointer into a component
                // that does not exist is a name to go and look for, and not a
                // pointer that goes in too far.
                let rest = reference.strip_prefix(&prefix);
                let component = rest.and_then(|rest| rest.split('/').next());
                let Some(found) = component.and_then(section) else {
                    return Err(RefError::Missing {
                        reference: reference.clone(),
                    });
                };
                if rest.is_some_and(|rest| rest.contains('/')) {
                    return Err(RefError::Inside {
                        reference: reference.clone(),
                    });
                }
                current = found;
            }
        }
    }
}

pub(crate) fn resolve_schema<'c>(
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
pub(crate) fn scalar_of(
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
        Type::Boolean(b) => Some(Scalar::Boolean(only_boolean(b))),
        Type::Object(_) | Type::Array(_) => None,
    })
}

/// Whether this schema's `enum` lists values and every one of them is `null`.
///
/// A flag cannot send `null`, so [`scalar_of`] leaves such an `enum` out, and the
/// scalar it reads then admits every value, where JSON Schema admits none that is
/// not `null`. The reduction asks this beside `scalar_of` and refuses the schema,
/// because reading it as no rule at all would be the approximation it forbids.
pub(crate) fn lists_only_null(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<bool, RefError> {
    let schema = stated(schema, components)?;
    let SchemaKind::Type(ty) = &schema.schema_kind else {
        return Ok(false);
    };
    Ok(match ty {
        Type::String(s) => only_null(&s.enumeration),
        Type::Number(n) => only_null(&n.enumeration),
        Type::Integer(i) => only_null(&i.enumeration),
        Type::Boolean(b) => only_null(&b.enumeration),
        Type::Object(_) | Type::Array(_) => false,
    })
}

/// Whether an `enum` lists something, and that something is `null` alone.
fn only_null<T>(enumeration: &[Option<T>]) -> bool {
    !enumeration.is_empty() && enumeration.iter().all(Option::is_none)
}

/// What a property says about itself, and what it inherits by pointing
/// somewhere else.
///
/// The field's own sentence wins: it is about this field, where the named
/// schema's is about every field that shares the rule. A field that says
/// nothing of its own takes the named schema's, which is better than nothing
/// and is all a bare `$ref` can leave behind.
pub(crate) fn description_of(
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
pub(crate) fn format_of(
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
pub(crate) fn template(
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
        Scalar::Integer(bounds) => bounds
            .allowed
            .first()
            .map_or_else(|| Value::from(0), |first| Value::from(*first)),
        Scalar::Number(bounds) => bounds
            .allowed
            .first()
            .map_or_else(|| Value::from(0.0), |first| Value::from(*first)),
        Scalar::Boolean(only) => Value::Bool(only.unwrap_or(false)),
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
        allowed: n.enumeration.iter().flatten().copied().collect(),
    }
}

fn integer_bounds(i: &IntegerType) -> Bounds<i64> {
    Bounds {
        low: limit(i.minimum, i.exclusive_minimum),
        high: limit(i.maximum, i.exclusive_maximum),
        multiple_of: i.multiple_of,
        allowed: i.enumeration.iter().flatten().copied().collect(),
    }
}

/// The one value a boolean's `enum` allows, and `None` where it allows both or
/// lists none: `[true, false]` says what `type: boolean` says already.
fn only_boolean(b: &BooleanType) -> Option<bool> {
    let mut listed = b.enumeration.iter().flatten().copied();
    let first = listed.next()?;
    listed.all(|value| value == first).then_some(first)
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
pub(crate) fn is_media_type(media_type: &str) -> bool {
    has_type_and_subtype(&essence(media_type))
}

/// Whether this key can be a request's `Content-Type` as it stands.
///
/// [`is_media_type`] reads a key as a person does and is asked of every key that
/// is picked. This is asked only of a key that goes out as the header, a JSON
/// body's and one that is sent verbatim: a `multipart/form-data` body writes its
/// own `Content-Type`, with a boundary, and its key is never sent. The header is
/// read as RFC 9110 reads it: a space and a tab before the type and after the
/// subtype are optional white space and are trimmed, and no other white space is,
/// so a no-break space there is no `token` character and a server reads no type
/// in it. The whole key must also be a header value, which refuses an ASCII
/// control character other than a tab, in a parameter as much as in the essence.
/// The rest of a parameter is not parsed, for the reason that [`is_media_type`]
/// gives.
#[must_use]
pub(crate) fn is_content_type(media_type: &str) -> bool {
    has_type_and_subtype(before_parameters(media_type).trim_matches([' ', '\t']))
        && HeaderValue::from_str(media_type).is_ok()
}

/// Whether the essence is `type/subtype`: two non-empty `token`s either side of
/// the one `/` that is not a `token` character.
fn has_type_and_subtype(essence: &str) -> bool {
    essence
        .split_once('/')
        .is_some_and(|(ty, subtype)| is_token(ty) && is_token(subtype))
}

/// Whether this is a media range — `*/*`, `image/*` — and so a set of types and
/// not one.
///
/// A `content` key may be one, because the document can say that a body is
/// anything that the range admits. A request is sent under one type, and the
/// range does not name it: `*` is a `token` character, so a range passes
/// [`is_media_type`] and, sent as it stands, is a `Content-Type` that no server
/// reads. The `*` is looked for in the type and the subtype and nowhere else,
/// because a parameter after the `;` is the vendor's to spell. No registered
/// media type has a `*` in its name, so `application/*+json` is a pattern too,
/// and a range in the same sense: the vendor means a family, and this crate
/// cannot choose the member.
#[must_use]
pub(crate) fn is_media_range(media_type: &str) -> bool {
    essence(media_type).contains('*')
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
///
/// The suffix is all that is read, so `application/*+json` names JSON too. A
/// request body and a response pick their `content` key by this, and a request
/// body that picks a range is refused as one ([`is_media_range`]).
#[must_use]
pub(crate) fn is_json(media_type: &str) -> bool {
    essence(media_type) == "application/json" || essence(media_type).ends_with("+json")
}

/// A media type this crate can assemble from `--file` and `--field` parts.
#[must_use]
pub(crate) fn is_multipart(media_type: &str) -> bool {
    essence(media_type) == "multipart/form-data"
}

fn essence(media_type: &str) -> String {
    before_parameters(media_type).trim().to_ascii_lowercase()
}

/// What stands before the first `;`: the essence and the white space round it.
fn before_parameters(media_type: &str) -> &str {
    media_type.split(';').next().unwrap_or(media_type)
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

    /// A key that goes out as the `Content-Type` is a header value, so a byte
    /// that no header carries is no part of one, in a parameter as much as in the
    /// essence.
    #[test]
    fn a_content_type_is_a_header_value() {
        assert!(!is_content_type("application/json; x=\"a\u{1}b\""));
        assert!(!is_content_type("application/json; x=\"a\nb\""));
        assert!(!is_content_type("application/json;x=\u{7f}"));
        // A tab is one a header carries, and so is a byte past ASCII, a C1
        // control character among them.
        assert!(is_content_type("text/plain; x=\"a\tb\""));
        assert!(is_content_type("text/plain; x=\"é\""));
        assert!(is_content_type("text/plain; x=\"a\u{85}b\""));
        // `is_media_type` leaves a parameter unread, so a multipart key with such
        // a byte is a media type: the key is never sent and is not held to the
        // header.
        assert!(is_media_type("multipart/form-data; x=\"a\u{1}b\""));
    }

    /// RFC 9110's optional white space is a space and a tab. `str::trim`, which
    /// `is_media_type` reads the essence with, takes every Unicode white space, so
    /// a key with another before its type or after its subtype is a media type
    /// and no content type.
    #[test]
    fn a_content_type_is_trimmed_of_a_space_and_a_tab_and_no_other_white_space() {
        assert!(is_content_type(" text/plain\t"));
        assert!(is_content_type("application/json ; charset=utf-8"));

        for key in [
            "application/json\u{a0}",
            "\u{a0}application/json",
            "\u{85}application/json",
            "application/json\u{3000}",
            "application/merge-patch+json\u{a0}",
            "text/plain\u{2003}; charset=utf-8",
            // A vertical tab, a form feed and a carriage return are control
            // characters, and a header refuses them in the essence too.
            "application/json\u{b}",
            "application/json\u{c}",
            "application/json\r",
        ] {
            assert!(!is_content_type(key), "{key:?}");
        }
        // `is_media_type` and `is_json` trim every Unicode white space, and the
        // pick of a key and the typing of a response read through them, so
        // neither depends on this question.
        assert!(is_media_type("application/json\u{a0}"));
        assert!(is_json("application/json\u{a0}"));
    }

    #[test]
    fn a_range_has_a_star_in_its_type_or_its_subtype_and_nowhere_else() {
        assert!(is_media_range("*/*"));
        assert!(is_media_range("image/*"));
        assert!(is_media_range("*/json"));
        assert!(is_media_range("application/*+json"));
        assert!(is_media_range("Image/*; q=0.5"));

        assert!(!is_media_range("image/png"));
        assert!(!is_media_range("application/vnd.api+json"));
        // A parameter is the vendor's to spell, and is read no further than the
        // `;` that starts it.
        assert!(!is_media_range(r#"multipart/form-data; boundary="*""#));
        // Not a media type, which is another refusal and asked first.
        assert!(!is_media_range("form-data"));
    }

    #[test]
    fn json_is_recognised_through_suffixes_and_parameters() {
        assert!(is_json("application/json"));
        assert!(is_json("application/json; charset=utf-8"));
        assert!(is_json("application/merge-patch+json"));
        assert!(!is_json("form-data"));
        assert!(!is_json("multipart/form-data"));
        assert!(!is_json("*/*"));
        // The suffix is all that is read, so a range that ends in it is read as
        // JSON: it is the key a request body is picked under and then refused,
        // and the key a response is typed from.
        assert!(is_json("application/*+json"));
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
