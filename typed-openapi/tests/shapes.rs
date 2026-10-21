//! What `client::fits` makes of the types typify writes.
//!
//! `fits` is held to a type by reading a body into it and writing it back, so
//! what it sees of a body depends on what serde makes of the type: an untagged
//! union is read from a buffer, and so are an internally tagged union, a
//! `flatten`ed struct and an adjacently tagged union whose content comes before
//! its tag. An externally tagged enum is not. Which of those typify writes for
//! which schema is typify's call, and a hand-written type in `src/client.rs`
//! holds only what its author thought of. So these types are typify's own,
//! written from a JSON Schema in `tests/fixtures/shapes.json` by `import_types!`,
//! and a release of typify that writes a schema another way is named by the case
//! that stood on it.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use typed_openapi::client::{BodyError, fits};

#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    dead_code,
    missing_debug_implementations,
    unreachable_pub,
    unused,
    reason = "typify writes this module and it is not graded on style"
)]
mod types {
    typify::import_types!(schema = "tests/fixtures/shapes.json");
}

/// What `fits` answered, as the part of it that a case states.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Fits,
    Mismatch(String),
    Undeclared(String),
    NotAnObject(String),
}

/// One body, held to `T`.
fn verdict<T: DeserializeOwned + Serialize>(body: &Value) -> Verdict {
    match fits::<T>("probe", body) {
        Ok(()) => Verdict::Fits,
        Err(BodyError::Mismatch { path, .. }) => Verdict::Mismatch(path),
        Err(BodyError::Undeclared { path, .. }) => Verdict::Undeclared(path),
        Err(BodyError::NotAnObject { path, .. }) => Verdict::NotAnObject(path),
        Err(other) => panic!("a type typify writes can be written back: {other}"),
    }
}

fn fits_ok() -> Verdict {
    Verdict::Fits
}

fn mismatch(path: &str) -> Verdict {
    Verdict::Mismatch(path.to_owned())
}

fn undeclared(path: &str) -> Verdict {
    Verdict::Undeclared(path.to_owned())
}

fn not_an_object(path: &str) -> Verdict {
    Verdict::NotAnObject(path.to_owned())
}

/// A `Contact` with one more property than its required `name`.
fn with(property: &str, value: impl Into<Value>) -> Value {
    json!({"name": "Acme Ltd", property: value.into()})
}

fn street() -> Value {
    json!({"street": "1 Example Way", "city": "Vienna"})
}

fn street_and(property: &str, value: impl Into<Value>) -> Value {
    json!({"street": "1 Example Way", "city": "Vienna", property: value.into()})
}

/// Run every case against `T`, and name the one that differs.
fn all<T: DeserializeOwned + Serialize>(cases: Vec<(&str, Value, Verdict)>) {
    for (name, body, expected) in cases {
        assert_eq!(verdict::<T>(&body), expected, "{name}: {body}");
    }
}

#[test]
fn a_struct_refuses_a_property_it_has_no_field_for_at_every_depth() {
    all::<types::Contact>(vec![
        ("a body that fits", with("memo", json!("hi")), fits_ok()),
        (
            "a property of the body",
            with("undeclaredField", json!("x")),
            undeclared("undeclaredField"),
        ),
        (
            "a property of a nested object",
            with("address", street_and("extra", json!("x"))),
            undeclared("address.extra"),
        ),
        (
            "a property of an element of a list",
            with(
                "positions",
                json!([street(), street_and("extra", json!(1))]),
            ),
            undeclared("positions[1].extra"),
        ),
    ]);
}

/// typify's `skip_serializing_if` leaves an optional property that holds
/// nothing out of what the type writes, and the body is not refused for it.
#[test]
fn an_optional_property_that_holds_nothing_is_not_an_undeclared_one() {
    all::<types::Contact>(vec![
        (
            "an optional string, null",
            with("memo", Value::Null),
            fits_ok(),
        ),
        (
            "an optional list, empty",
            with("tags", json!([])),
            fits_ok(),
        ),
        (
            "an optional object, null",
            with("address", Value::Null),
            fits_ok(),
        ),
        (
            "a property nothing declares, null",
            with("undeclaredField", Value::Null),
            undeclared("undeclaredField"),
        ),
        (
            "a property nothing declares, an empty list",
            with("undeclaredField", json!([])),
            undeclared("undeclaredField"),
        ),
        (
            "a property of a nested object, null",
            with("address", street_and("extra", Value::Null)),
            undeclared("address.extra"),
        ),
    ]);
}

/// Serde fills a struct from a list by place, so `["1 Example Way", "Vienna"]`
/// is an address as far as the type goes. A body is sent as it was written, and
/// the server reads it as a list.
#[test]
fn a_list_where_the_type_reads_an_object_is_refused_at_every_depth() {
    let by_place = json!(["1 Example Way", "Vienna"]);
    all::<types::Contact>(vec![
        (
            "a property",
            with("address", by_place.clone()),
            not_an_object("address"),
        ),
        (
            "an element of a list",
            with("positions", json!([street(), by_place.clone()])),
            not_an_object("positions[1]"),
        ),
        (
            "a list that no struct reads is a value of the wrong type",
            with("address", json!(["1 Example Way"])),
            mismatch("address"),
        ),
        (
            "the body itself, of any length",
            json!(["x"]),
            not_an_object("."),
        ),
    ]);
    all::<types::Holder>(vec![
        (
            "a value of a map of objects",
            json!({"am": {"k": by_place.clone()}}),
            not_an_object("am.k"),
        ),
        (
            "a struct that allOf merged, from a list of its three properties",
            json!({"m": ["1 Example Way", "Vienna", "1010"]}),
            not_an_object("m"),
        ),
        (
            "an object that may be null",
            json!({"na": by_place}),
            not_an_object("na"),
        ),
        ("an object that is null", json!({"na": null}), fits_ok()),
        (
            "a value of a map of objects, with a property of its own",
            json!({"am": {"k": street_and("extra", json!(1))}}),
            undeclared("am.k.extra"),
        ),
    ]);
}

/// Serde cannot read an object from a scalar, so below the top a scalar where
/// the type takes an object is a value of the wrong type. The body itself is
/// asked first and is refused for what it is.
#[test]
fn a_scalar_where_the_type_reads_an_object_is_a_mismatch_below_the_top() {
    all::<types::Contact>(vec![
        (
            "a string for a property",
            with("address", json!("1 Example Way")),
            mismatch("address"),
        ),
        (
            "a number for a property",
            with("address", json!(5)),
            mismatch("address"),
        ),
        (
            "a string for an externally tagged union",
            with("ei", json!("x")),
            mismatch("ei"),
        ),
        ("the body itself, a string", json!("x"), not_an_object(".")),
        ("the body itself, a number", json!(5), not_an_object(".")),
    ]);
}

/// A `oneOf` of closed objects that share no property typify tells apart by
/// value is an `untagged` enum, and serde reads it from a buffer.
#[test]
fn a_union_is_held_to_the_member_that_serde_read() {
    all::<types::Contact>(vec![
        (
            "an extra property",
            with("spot", street_and("extra", json!("x"))),
            undeclared("spot.extra"),
        ),
        (
            "a list",
            with("spot", json!(["1 Example Way", "Vienna"])),
            not_an_object("spot"),
        ),
        (
            "the other member",
            with("spot", json!({"x": 1, "y": 2})),
            fits_ok(),
        ),
    ]);
    all::<types::Untagged>(vec![
        (
            "the body, extra",
            street_and("extra", json!("x")),
            undeclared("extra"),
        ),
        (
            "the body, a list",
            json!(["1 Example Way", "Vienna"]),
            not_an_object("."),
        ),
        (
            "the body, a list that no member reads",
            json!(["1 Example Way"]),
            mismatch("."),
        ),
    ]);
}

/// A `oneOf` whose members share a constant property is an internally tagged
/// enum.
#[test]
fn an_internally_tagged_union_is_held_to_the_member_its_tag_names() {
    all::<types::Contact>(vec![
        (
            "a dog",
            with("pet", json!({"kind": "dog", "name": "Rex"})),
            fits_ok(),
        ),
        (
            "a cat",
            with("pet", json!({"kind": "cat", "lives": 9})),
            fits_ok(),
        ),
        (
            "an extra property",
            with("pet", json!({"kind": "dog", "name": "Rex", "extra": 1})),
            undeclared("pet.extra"),
        ),
        (
            "a property of the other member",
            with("pet", json!({"kind": "dog", "name": "Rex", "lives": 9})),
            undeclared("pet.lives"),
        ),
        (
            "a list",
            with("pet", json!(["dog", "Rex"])),
            not_an_object("pet"),
        ),
    ]);
    all::<types::Pet>(vec![(
        "the body, a list",
        json!(["dog", "Rex"]),
        not_an_object("."),
    )]);
}

/// A `oneOf` of closed objects that each have one required property is an
/// externally tagged enum, and the key of the member is in the body.
#[test]
fn an_externally_tagged_union_puts_the_variants_key_in_the_path() {
    all::<types::Contact>(vec![
        (
            "a member",
            with("place", json!({"point": {"x": 1, "y": 2}})),
            fits_ok(),
        ),
        (
            "an extra property of the member",
            with("place", json!({"address": street_and("extra", json!("x"))})),
            undeclared("place.address.extra"),
        ),
        (
            "a list in place of the member",
            with("place", json!({"address": ["1 Example Way", "Vienna"]})),
            not_an_object("place.address"),
        ),
    ]);
}

/// The two shapes of a union where the document says more than the type reads.
/// A body the type cannot send is refused, because the typed call cannot send
/// it either.
#[test]
fn a_union_that_serde_reads_as_one_member_refuses_what_the_other_one_holds() {
    all::<types::Contact>(vec![
        (
            "an anyOf, filled as both members",
            with("any", json!({"street": "1", "city": "V", "x": 1, "y": 2})),
            undeclared("any.x"),
        ),
        (
            "an anyOf, one member",
            with("any", street_and("extra", json!("x"))),
            undeclared("any.extra"),
        ),
    ]);
    all::<types::Holder>(vec![
        (
            "an anyOf whose members overlap, filled as both",
            json!({"o2": {"street": "1", "city": "V", "name": "n"}}),
            undeclared("o2.name"),
        ),
        (
            "an anyOf whose members overlap, a list",
            json!({"o2": ["1", "V"]}),
            not_an_object("o2"),
        ),
        (
            "a oneOf of an object and a list, given a list the object reads",
            json!({"ol": ["1", "V"]}),
            not_an_object("ol"),
        ),
        (
            "a oneOf of an object and a list, given a list it does not",
            json!({"ol": ["1", "V", "W"]}),
            fits_ok(),
        ),
    ]);
    all::<types::Overlap>(vec![(
        "an anyOf as the body, extra",
        street_and("extra", json!("x")),
        undeclared("extra"),
    )]);
}

/// The property that holds nothing, which `fits` can tell from an optional one
/// only where the type skips it while it reads. A buffer skips nothing, so
/// there it passes; an externally tagged enum is read directly, so it does not,
/// and the key of its member is in the path.
#[test]
fn an_undeclared_property_that_holds_nothing_is_seen_outside_a_buffer_only() {
    all::<types::Contact>(vec![
        (
            "in an untagged union",
            with("spot", street_and("extra", Value::Null)),
            fits_ok(),
        ),
        (
            "in an internally tagged union",
            with("pet", json!({"kind": "dog", "name": "Rex", "extra": []})),
            fits_ok(),
        ),
        (
            "in a flattened struct",
            with("al", json!({"a": "x", "zzz": null})),
            fits_ok(),
        ),
        (
            "in an externally tagged union, in a newtype variant",
            with(
                "place",
                json!({"address": street_and("extra", Value::Null)}),
            ),
            undeclared("place.address.extra"),
        ),
        (
            "in an externally tagged union, in a struct variant",
            with("ei", json!({"address": {"street": "s", "extra": []}})),
            undeclared("ei.address.extra"),
        ),
        (
            "in an externally tagged union, an empty object",
            with("ei", json!({"point": {"x": 1, "extra": {}}})),
            undeclared("ei.point.extra"),
        ),
    ]);
}

/// A key of the body that spells like the member holding it is still the
/// property, and not the member.
#[test]
fn a_property_spelled_like_the_member_that_holds_it_is_named_whole() {
    all::<types::Contact>(vec![(
        "in an externally tagged union, in a struct variant",
        with(
            "ei",
            json!({"address": {"street": "s", "address": Value::Null}}),
        ),
        undeclared("ei.address.address"),
    )]);
}

/// The key of an externally tagged member can be the name of the first property
/// of its content, and then the body holds the same key twice in a row. The path
/// is the same whatever the skipped property holds: `serde_ignored` sees one that
/// holds nothing and the walk of the body sees the rest.
#[test]
fn a_member_key_that_its_content_repeats_is_in_the_path_whatever_the_value() {
    for property in [Value::Null, json!([]), json!({}), json!("x")] {
        let body = json!({"a": {"a": {"x": 1, "extra": property}, "b": 2}});
        all::<types::Contact>(vec![
            (
                "a struct variant",
                with("rep", body.clone()),
                undeclared("rep.a.a.extra"),
            ),
            (
                "a newtype variant",
                with("repn", body.clone()),
                undeclared("repn.a.a.extra"),
            ),
        ]);
        all::<types::ExternalRepeat>(vec![(
            "the body, a struct variant",
            body.clone(),
            undeclared("a.a.extra"),
        )]);
        all::<types::ExternalRepeatNewtype>(vec![(
            "the body, a newtype variant",
            body,
            undeclared("a.a.extra"),
        )]);
    }
}

/// A `oneOf` of objects that each require a constant tag and one content
/// property is an adjacently tagged enum. Serde reads it directly while the tag
/// comes first, and from a buffer when the content comes first, so a property
/// that holds nothing is seen in the first order and passes in the second.
#[test]
fn an_adjacently_tagged_union_is_held_to_the_member_its_tag_names() {
    let content_first = |data: Value| json!({"data": data, "kind": "a"});
    let tag_first = |data: Value| json!({"kind": "a", "data": data});
    all::<types::Contact>(vec![
        (
            "a member",
            with("adj", tag_first(json!({"x": 1}))),
            fits_ok(),
        ),
        (
            "an extra property, the tag first",
            with("adj", tag_first(json!({"x": 1, "extra": "x"}))),
            undeclared("adj.data.extra"),
        ),
        (
            "an extra property, the content first",
            with("adj", content_first(json!({"x": 1, "extra": "x"}))),
            undeclared("adj.data.extra"),
        ),
        (
            "an extra property that holds nothing, the tag first",
            with("adj", tag_first(json!({"x": 1, "extra": null}))),
            undeclared("adj.data.extra"),
        ),
        (
            "an extra property that holds nothing, the content first",
            with("adj", content_first(json!({"x": 1, "extra": null}))),
            fits_ok(),
        ),
        (
            "a list in the place of the content",
            with("adj", tag_first(json!([1]))),
            not_an_object("adj.data"),
        ),
    ]);
}

/// What the body itself is, asked of the type before it is read. Serde reads a
/// struct, a map and an adjacently tagged enum with `deserialize_struct` or
/// `deserialize_map`, so for those a scalar and a list are `NotAnObject`. The
/// other unions are read from a buffer or by `deserialize_any`, which asks for
/// no shape: a scalar is a value of the wrong type, and a list is refused only
/// when a member reads it by position.
#[test]
fn a_scalar_or_a_list_as_the_body_is_refused_for_what_the_type_reads() {
    for scalar in [json!("x"), json!(5), json!(true), Value::Null] {
        all::<types::Contact>(vec![("a struct", scalar.clone(), not_an_object("."))]);
        all::<types::Adjacent>(vec![(
            "an adjacently tagged union",
            scalar.clone(),
            not_an_object("."),
        )]);
        all::<types::Untagged>(vec![("an untagged union", scalar.clone(), mismatch("."))]);
        all::<types::Pet>(vec![(
            "an internally tagged union",
            scalar.clone(),
            mismatch("."),
        )]);
        all::<types::External>(vec![("an externally tagged union", scalar, mismatch("."))]);
    }
    let by_place = json!(["dog", "Rex"]);
    let by_no_place = json!(["a", 1]);
    all::<types::Untagged>(vec![
        (
            "a list a member reads",
            by_place.clone(),
            not_an_object("."),
        ),
        ("a list no member reads", by_no_place.clone(), mismatch(".")),
    ]);
    // An internally tagged enum takes the first element of a list for its tag.
    all::<types::Pet>(vec![
        ("a list a member reads", by_place, not_an_object(".")),
        (
            "a list whose tag no member has",
            json!(["bird", 1]),
            mismatch("[0]"),
        ),
    ]);
    all::<types::External>(vec![("a list", by_no_place, mismatch("."))]);
}

/// typify writes an `anyOf` as an enum only when it judges the members
/// exclusive. Members that are objects with nothing required cannot be told
/// apart, so it writes a struct of `flatten`ed `Option`s, and serde reads
/// that from a buffer.
#[test]
fn an_anyof_that_typify_cannot_judge_exclusive_is_a_struct_of_flattened_options() {
    all::<types::Contact>(vec![
        ("one member", with("al", json!({"b": 1})), fits_ok()),
        (
            "both members",
            with("al", json!({"a": "x", "b": 1})),
            fits_ok(),
        ),
        (
            "a property of neither",
            with("al", json!({"a": "x", "zzz": 1})),
            undeclared("al.zzz"),
        ),
        (
            "a property of neither, holding nothing",
            with("al", json!({"a": "x", "zzz": null})),
            fits_ok(),
        ),
        (
            "a value of the wrong type for a member, which serde reads as that \
             member being absent",
            with("al", json!({"b": "s"})),
            undeclared("al.b"),
        ),
    ]);
    all::<types::Flat>(vec![
        (
            "the body, both members",
            json!({"a": "x", "b": 1}),
            fits_ok(),
        ),
        (
            "the body, a property of neither",
            json!({"a": "x", "zzz": 1}),
            undeclared("zzz"),
        ),
    ]);
}

#[test]
fn a_schema_that_leaves_the_rest_open_takes_it_into_a_map() {
    all::<types::Contact>(vec![
        (
            "additionalProperties: {}",
            with("openmap", json!({"a": 1, "b": 2})),
            fits_ok(),
        ),
        (
            "additionalProperties: {}, a nested object",
            with("openmap", json!({"a": 1, "b": {"c": 1}})),
            fits_ok(),
        ),
    ]);
}

/// typify reads `additionalProperties: true` as "ignore the rest" and writes no
/// field for it. The generator says it as `{}` for that reason, so a type it
/// writes takes the rest; this is the reading that rewrite stands in front of.
/// When it stops being refused, typify has changed and the rewrite is not
/// needed.
#[test]
fn typify_gives_true_beside_properties_no_field() {
    all::<types::Contact>(vec![(
        "additionalProperties: true",
        with("open", json!({"a": 1, "b": 2})),
        undeclared("open.b"),
    )]);
}

/// `additionalProperties: false` is closed by typify, so serde refuses first.
#[test]
fn a_schema_that_shuts_the_rest_is_closed_by_serde() {
    all::<types::Contact>(vec![(
        "additionalProperties: false",
        with("closed", json!({"name": "x", "extra": 1})),
        mismatch("closed.extra"),
    )]);
}
