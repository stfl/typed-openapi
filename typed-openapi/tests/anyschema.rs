//! A scalar keeps its flag, and a shape this crate cannot read is refused under
//! its own reason.
//!
//! `openapiv3` holds a schema it cannot fit in one of its types as `AnySchema`:
//! an integer with a bound spelled `0.0` or `1e3`, a `string` beside an `allOf`,
//! an `enum` that lists a value of another type. A schema whose `type` is a
//! scalar is that scalar, and a parameter of it is not "neither a value nor a list
//! of values", which is false of a number and would leave a body with no flags. A
//! schema that is none says why, with the keyword. The documents are the input of
//! the probes `A3b/scalars` `schemas` and `wrapper` behind F-43, built from the
//! same cases. A case that refuses asserts the variant and the words beside it.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use typed_openapi::model::{Body, Operation, Shape, Unsupported};
use typed_openapi::scalar::Limit;
use typed_openapi::tree;
use typed_openapi::{Document, LoadError, RefError, Scalar};

/// One operation with the schema as the query parameter `v`, and one with it as
/// the property `v` of a JSON body, as the probe builds them.
fn document(schema: &str, required: bool) -> String {
    format!(
        "openapi: 3.0.3
info: {{ title: t, version: \"1\" }}
servers: [{{ url: 'http://localhost:9999' }}]
paths:
  /x:
    get:
      operationId: op
      parameters:
        - name: v
          in: query
          required: {required}
          schema: {schema}
      responses: {{ '200': {{ description: OK }} }}
    post:
      operationId: body
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                v: {schema}
                w: {{ type: string }}
      responses: {{ '200': {{ description: OK }} }}
components:
  schemas:
    Code: {{ type: string, pattern: '^[A-Z]{{3}}$' }}
"
    )
}

fn loaded(schema: &str) -> Document {
    Document::load(&document(schema, false), &[]).expect("the document loads")
}

fn operation<'d>(doc: &'d Document, id: &str) -> &'d Operation {
    doc.get(id).expect("the operation")
}

/// What the parameter `v` is worth: its scalar, or why it has none.
fn param_of(doc: &Document) -> Result<Scalar, Unsupported> {
    match operation(doc, "op").param("v").expect("v").shape() {
        Shape::Flag { scalar, .. } => Ok(scalar.clone()),
        Shape::Unreachable(why) => Err(why.clone()),
    }
}

/// What the property `v` of the body is worth: its scalar, and `None` where the
/// body goes whole.
fn property_of(doc: &Document) -> Option<Scalar> {
    let body = operation(doc, "body").body();
    assert!(
        matches!(body, Body::JsonFields { .. } | Body::JsonWhole { .. }),
        "a JSON body: {body:?}"
    );
    body.fields()
        .iter()
        .find(|field| field.name() == "v")
        .map(|field| field.scalar().clone())
}

fn is_integer(scalar: &Scalar) -> bool {
    matches!(scalar, Scalar::Integer(_))
}

/// A bound that `openapiv3` holds as an `f64` because it is not an `i64`, and
/// that is the integer all the same.
#[test]
fn an_integer_with_a_bound_spelled_as_a_float_is_an_integer() {
    for (schema, refused, admitted) in [
        ("{ type: integer, minimum: 0.0 }", "-1", "0"),
        ("{ type: integer, maximum: 1e3 }", "1001", "1000"),
        ("{ type: integer, multipleOf: 2.0 }", "3", "4"),
        (
            "{ type: integer, minimum: 0.0, exclusiveMinimum: true }",
            "0",
            "1",
        ),
        ("{ type: integer, minimum: -5e0, maximum: 5.0 }", "6", "-5"),
    ] {
        let doc = loaded(schema);
        let scalar = param_of(&doc).unwrap_or_else(|why| panic!("{schema}: {why}"));
        assert!(is_integer(&scalar), "{schema}");
        assert!(scalar.parse(refused).is_err(), "{schema}: {refused}");
        assert!(scalar.parse(admitted).is_ok(), "{schema}: {admitted}");
        // The property of a body reads the same, and so the body keeps its flags.
        let property = property_of(&doc).unwrap_or_else(|| panic!("{schema}: the body went whole"));
        assert!(property.parse(refused).is_err(), "{schema}: {refused}");
        assert!(property.parse(admitted).is_ok(), "{schema}: {admitted}");
    }
}

/// The probe's second case: a parameter that the document requires loads when its
/// schema is an integer with a bound spelled as a float, because that is a value.
#[test]
fn a_required_integer_with_a_bound_spelled_as_a_float_loads() {
    let doc = Document::load(&document("{ type: integer, minimum: 0.0 }", true), &[])
        .expect("a required integer is a value");
    assert!(operation(&doc, "op").param("v").expect("v").required());
}

/// The format travels beside the rules, and an integer that `openapiv3` held as
/// `AnySchema` has one like any other.
#[test]
fn the_format_of_a_scalar_read_from_an_any_schema_is_kept() {
    let doc = loaded("{ type: integer, format: int32, minimum: 0.0 }");
    assert_eq!(
        operation(&doc, "op").param("v").expect("v").format(),
        Some("int32")
    );
}

/// The range of an integer: its two bounds, or the keyword that refuses it.
type Range = Result<(Option<Limit<i64>>, Option<Limit<i64>>), &'static str>;

/// What the schema `v` is worth as an integer, read from the parameter and from
/// the property of a body alike: its bounds, or the keyword that refuses it. The
/// body goes whole where the property is refused.
fn range_of(schema: &str) -> Range {
    let doc = loaded(schema);
    let property = property_of(&doc);
    match param_of(&doc) {
        Ok(Scalar::Integer(bounds)) => {
            let Some(Scalar::Integer(same)) = property else {
                panic!("{schema}: the parameter is an integer and the property is {property:?}");
            };
            assert_eq!(bounds, same, "{schema}");
            Ok((bounds.low, bounds.high))
        }
        Ok(other) => panic!("{schema}: {other:?}"),
        Err(Unsupported::Keyword(keyword)) => {
            assert!(property.is_none(), "{schema}: the body goes whole");
            Err(match keyword.as_str() {
                "minimum" => "minimum",
                "maximum" => "maximum",
                "multipleOf" => "multipleOf",
                other => panic!("{schema}: refused by `{other}`"),
            })
        }
        Err(other) => panic!("{schema}: {other:?}"),
    }
}

/// One bound as the document spells it, whether `exclusive…` is set beside it, and
/// what the integer comes to.
type Row = (&'static str, bool, Range);

/// What an integer's `minimum` comes to when `openapiv3` holds it as a double,
/// because it is no `i64`.
fn minimums() -> Vec<Row> {
    use Limit::{Exclusive, Inclusive};
    let low = |limit| Ok((limit, None));
    vec![
        // A fraction: the ceiling, whatever `exclusiveMinimum` says.
        ("0.5", false, low(Some(Inclusive(1)))),
        ("2.5", false, low(Some(Inclusive(3)))),
        ("-0.5", false, low(Some(Inclusive(0)))),
        ("-2.5", false, low(Some(Inclusive(-2)))),
        ("0.5", true, low(Some(Inclusive(1)))),
        ("-2.5", true, low(Some(Inclusive(-2)))),
        // Whole, below 2^53: that integer, exclusive where the document says so.
        ("0.0", false, low(Some(Inclusive(0)))),
        ("0.0", true, low(Some(Exclusive(0)))),
        (
            "9007199254740991.0",
            false,
            low(Some(Inclusive(9_007_199_254_740_991))),
        ),
        (
            "-9007199254740991.0",
            false,
            low(Some(Inclusive(-9_007_199_254_740_991))),
        ),
        // Whole, from 2^53 to 2^63, both ends included: no one integer.
        ("9007199254740992.0", false, Err("minimum")),
        ("-9007199254740992.0", false, Err("minimum")),
        ("9223372036854775807.0", false, Err("minimum")),
        ("9223372036854775808", false, Err("minimum")),
        ("-9223372036854775808.0", false, Err("minimum")),
        // Above 2^63: no integer of a flag meets it.
        ("9223372036854777856.0", false, Err("minimum")),
        ("18446744073709551615", false, Err("minimum")),
        ("1e30", false, Err("minimum")),
        // Below −2^63: every integer of a flag meets it.
        ("-9223372036854777856.0", false, low(None)),
        ("-1e30", false, low(None)),
        ("-1e30", true, low(None)),
    ]
}

/// What an integer's `maximum` comes to, the other way round.
fn maximums() -> Vec<Row> {
    use Limit::{Exclusive, Inclusive};
    let high = |limit| Ok((None, limit));
    vec![
        // A fraction: the floor, whatever `exclusiveMaximum` says.
        ("5.5", false, high(Some(Inclusive(5)))),
        ("0.5", false, high(Some(Inclusive(0)))),
        ("-0.5", false, high(Some(Inclusive(-1)))),
        ("-2.5", false, high(Some(Inclusive(-3)))),
        ("5.5", true, high(Some(Inclusive(5)))),
        ("-2.5", true, high(Some(Inclusive(-3)))),
        // Whole, below 2^53.
        ("1e3", false, high(Some(Inclusive(1000)))),
        ("1e3", true, high(Some(Exclusive(1000)))),
        (
            "9007199254740991.0",
            false,
            high(Some(Inclusive(9_007_199_254_740_991))),
        ),
        // Whole, from 2^53 to 2^63, both ends included.
        ("9007199254740992.0", false, Err("maximum")),
        ("9007199254740993.0", false, Err("maximum")),
        ("-9007199254740992.0", false, Err("maximum")),
        ("9223372036854775807.0", false, Err("maximum")),
        ("9223372036854775808", false, Err("maximum")),
        ("-9223372036854775808.0", false, Err("maximum")),
        // Above 2^63: every integer of a flag is below it, so it bounds nothing.
        ("9223372036854777856.0", false, high(None)),
        ("18446744073709551615", false, high(None)),
        ("1e30", false, high(None)),
        ("1e30", true, high(None)),
        // Below −2^63: no integer of a flag meets it.
        ("-9223372036854777856.0", false, Err("maximum")),
        ("-1e30", false, Err("maximum")),
    ]
}

/// An integer's range as the document spells it, with `keyword` as `minimum` or
/// `maximum` and the exclusive flag beside it where `exclusive` is set.
fn spelled(keyword: &str, bound: &str, exclusive: bool) -> String {
    let flag = match (exclusive, keyword) {
        (false, _) => "",
        (true, "minimum") => ", exclusiveMinimum: true",
        (true, _) => ", exclusiveMaximum: true",
    };
    format!("{{ type: integer, {keyword}: {bound}{flag} }}")
}

/// An integer's `minimum`, which `openapiv3` holds as a double when it is no
/// `i64`. A double with a fraction is the text of a number that is no integer, so
/// the integer next inside it is the bound, with or without `exclusiveMinimum`. A
/// whole double below 2^53 is that integer, whatever text gave it. From 2^53 to 2^63, both ends included,
/// more than one integer's text is that double, and the bound is refused. A
/// double above 2^63 is above every integer a flag carries, so none meets it, and
/// one below −2^63 is below every one, so it bounds nothing.
#[test]
fn an_integers_minimum_that_openapiv3_holds_as_a_double_is_read_where_the_double_is_exact() {
    for (bound, exclusive, expected) in minimums() {
        let schema = spelled("minimum", bound, exclusive);
        assert_eq!(range_of(&schema), expected, "{schema}");
    }
}

/// An integer's `maximum`, the other way round: a double with a fraction is the
/// integer next inside it, a double above 2^63 bounds nothing, and one below
/// −2^63 is below every integer a flag carries.
#[test]
fn an_integers_maximum_that_openapiv3_holds_as_a_double_is_read_where_the_double_is_exact() {
    for (bound, exclusive, expected) in maximums() {
        let schema = spelled("maximum", bound, exclusive);
        assert_eq!(range_of(&schema), expected, "{schema}");
    }
}

/// An unsigned 64-bit id is the case that matters: `openapiv3` holds it as an
/// `AnySchema` because its maximum is no `i64`, and it is an integer of at least 0.
/// A document that requires such a parameter loads.
#[test]
fn an_unsigned_64_bit_id_is_an_integer_of_at_least_0() {
    let schema = "{ type: integer, minimum: 0, maximum: 18446744073709551615 }";
    assert_eq!(
        range_of(schema),
        Ok((Some(Limit::Inclusive(0)), None)),
        "{schema}"
    );
    let doc = Document::load(&document(schema, true), &[]).expect("a required id is a value");
    assert!(operation(&doc, "op").param("v").expect("v").required());

    // The probe's own input: a bound that no integer meets, which refuses the
    // document that requires it, by the keyword.
    let schema = "{ type: integer, minimum: 9223372036854775808 }";
    let refused = Document::load(&document(schema, true), &[]).expect_err("no integer meets it");
    assert!(
        matches!(&refused, LoadError::Parameter { why: Unsupported::Keyword(found), .. } if found == "minimum"),
        "{refused:?}"
    );
}

/// A step is read as an integer only where it is whole: a whole double below 2^53 is
/// that integer, whatever text gave it, as for a bound, so `2.00000000000000001` is
/// the step 2. A double with a fraction
/// and a whole one from 2^53 are refused by their keyword: `2.5` states that an
/// integer is a multiple of 5, and no step that this crate reads says so.
#[test]
fn an_integers_step_that_openapiv3_holds_as_a_double_is_read_only_where_it_is_whole() {
    for (schema, expected) in [
        ("{ type: integer, minimum: 0.0, multipleOf: 2.0 }", Ok(())),
        (
            "{ type: integer, minimum: 0.0, multipleOf: 9007199254740991.0 }",
            Ok(()),
        ),
        (
            "{ type: integer, minimum: 0.0, multipleOf: 2.5 }",
            Err("multipleOf"),
        ),
        (
            "{ type: integer, minimum: 0.0, multipleOf: 9007199254740992.0 }",
            Err("multipleOf"),
        ),
        (
            "{ type: integer, minimum: 0.0, multipleOf: 1e30 }",
            Err("multipleOf"),
        ),
    ] {
        assert_eq!(range_of(schema).map(drop), expected, "{schema}");
    }
    // The reader keeps the double and not the text.
    let doc = loaded("{ type: integer, minimum: 0.0, multipleOf: 2.00000000000000001 }");
    let scalar = param_of(&doc).expect("a scalar");
    assert!(scalar.parse("4").is_ok());
    assert!(scalar.parse("3").is_err());
}

/// An `enum` that lists a value of another type is why `openapiv3` holds the
/// schema as `AnySchema`, and the keyword that holds it is the refusal.
#[test]
fn an_enum_that_lists_a_value_of_another_type_is_refused_by_its_keyword() {
    for schema in [
        "{ type: string, enum: [a, 1] }",
        "{ type: string, format: date, enum: [a, 1] }",
        "{ type: integer, enum: [1, 2.5] }",
        "{ type: integer, enum: [1.0] }",
        "{ type: number, enum: [1, x] }",
        "{ type: boolean, enum: [true, x] }",
    ] {
        let why = param_of(&loaded(schema)).expect_err(schema);
        assert!(
            matches!(&why, Unsupported::Keyword(found) if found == "enum"),
            "{schema}: {why:?}"
        );
    }
}

/// A `null` in an `enum` is the value `null`, which a flag cannot send, and an
/// `enum` that lists nothing else admits nothing a flag could carry. Both are read
/// the way they are when `openapiv3` holds the schema as a type.
#[test]
fn a_null_in_the_enum_of_a_scalar_read_from_an_any_schema_is_read_as_null() {
    let doc = loaded("{ type: integer, minimum: 0.0, enum: [1, 2, null] }");
    let scalar = param_of(&doc).expect("a scalar");
    assert!(scalar.parse("2").is_ok());
    assert!(scalar.parse("3").is_err());

    let refused = Document::load(
        &document("{ type: integer, minimum: 0.0, enum: [null] }", false),
        &[],
    )
    .expect_err("an enum of null alone");
    assert!(
        matches!(refused, LoadError::Unrunnable { ref name, .. } if name == "v"),
        "{refused:?}"
    );
}

/// A keyword that the scalar `type` does not have is named, and the reason is not
/// the false one.
#[test]
fn a_keyword_the_type_does_not_have_is_named() {
    for (schema, keyword) in [
        (
            "{ type: string, allOf: [{ $ref: '#/components/schemas/Code' }] }",
            "allOf",
        ),
        (
            "{ type: string, oneOf: [{ $ref: '#/components/schemas/Code' }] }",
            "oneOf",
        ),
        ("{ type: integer, pattern: '^[0-9]+$' }", "pattern"),
        ("{ type: string, minimum: 0 }", "minimum"),
        ("{ type: boolean, format: flag }", "format"),
        ("{ type: boolean, minLength: 1 }", "minLength"),
        // Several keywords the type does not have: the compositions come first,
        // so the same schema names the same keyword every time.
        (
            "{ type: integer, pattern: '^a$', allOf: [{ $ref: '#/components/schemas/Code' }] }",
            "allOf",
        ),
        (
            "{ type: string, minimum: 1, oneOf: [{ $ref: '#/components/schemas/Code' }], \
             items: { type: string } }",
            "oneOf",
        ),
        ("{ type: number, items: { type: string } }", "items"),
        (
            "{ type: string, properties: { a: { type: string } } }",
            "properties",
        ),
    ] {
        let doc = loaded(schema);
        let why = param_of(&doc).expect_err(schema);
        assert!(
            matches!(&why, Unsupported::Keyword(found) if found == keyword),
            "{schema}: {why:?}"
        );
        assert!(
            !why.to_string().contains("neither a value"),
            "{schema}: {why}"
        );
        // The body goes whole, as it does for any property that has no flag.
        assert!(property_of(&doc).is_none(), "{schema}");
    }
}

/// Each keyword that `openapiv3` holds in an `AnySchema`, with a value that it
/// reads, as the document spells it.
const KEYWORDS: [(&str, &str); 23] = [
    ("allOf", "[{ type: string }]"),
    ("oneOf", "[{ type: string }]"),
    ("anyOf", "[{ type: string }]"),
    ("not", "{ type: string }"),
    ("properties", "{ a: { type: string } }"),
    ("required", "[a]"),
    ("additionalProperties", "true"),
    ("minProperties", "1"),
    ("maxProperties", "1"),
    ("items", "{ type: string }"),
    ("minItems", "1"),
    ("maxItems", "1"),
    ("uniqueItems", "true"),
    ("pattern", "'^a$'"),
    ("minLength", "1"),
    ("maxLength", "1"),
    ("minimum", "1"),
    ("maximum", "1"),
    ("exclusiveMinimum", "true"),
    ("exclusiveMaximum", "true"),
    ("multipleOf", "2"),
    ("enum", "[a]"),
    ("format", "x"),
];

/// The keywords each `type` has, from JSON Schema, and not from this crate.
const OWN: [(&str, &[&str]); 6] = [
    (
        "string",
        &["pattern", "minLength", "maxLength", "enum", "format"],
    ),
    (
        "number",
        &[
            "multipleOf",
            "minimum",
            "maximum",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "enum",
            "format",
        ],
    ),
    (
        "integer",
        &[
            "multipleOf",
            "minimum",
            "maximum",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "enum",
            "format",
        ],
    ),
    ("boolean", &["enum"]),
    (
        "object",
        &[
            "properties",
            "required",
            "additionalProperties",
            "minProperties",
            "maxProperties",
        ],
    ),
    ("array", &["items", "minItems", "maxItems", "uniqueItems"]),
];

/// A keyword that the `type` does not have is named, whichever keyword and
/// whichever type of the six. A keyword that the reading quietly dropped would be
/// the vendor's rule gone with no word, so every keyword is put beside every type
/// that does not have it.
///
/// `required` is left to the unit test of `any_type`, which reads the schema with
/// no document round it: beside a `type` that has no `properties` it names a key
/// that nothing declares, and a document with one is refused as `Phantom` before
/// it is reduced.
#[test]
fn every_keyword_beside_a_type_that_does_not_have_it_is_named() {
    for (ty, own) in OWN {
        for (keyword, value) in KEYWORDS {
            if own.contains(&keyword) || keyword == "required" {
                continue;
            }
            let schema = format!("{{ type: {ty}, {keyword}: {value} }}");
            let doc = loaded(&schema);
            let why = param_of(&doc).expect_err(&schema);
            assert!(
                matches!(&why, Unsupported::Keyword(found) if found == keyword),
                "{schema}: {why:?}"
            );
            assert!(property_of(&doc).is_none(), "{schema}: the body goes whole");
        }
    }
}

/// The words of the refusal, once: in the long help, where an optional parameter
/// says where its flag went, and in the refusal of a document that requires it.
#[test]
fn the_reason_is_said_where_a_flag_would_have_been_and_when_the_document_requires_it() {
    let schema = "{ type: string, allOf: [{ $ref: '#/components/schemas/Code' }] }";
    let doc = loaded(schema);
    let help = tree::command(operation(&doc, "op"))
        .get_long_about()
        .expect("a long help")
        .to_string();
    assert!(
        help.contains(
            "`v` has no flag: it is stated with `allOf`, which this CLI does not read as a \
             value. The request goes out without it."
        ),
        "{help}"
    );

    let refused =
        Document::load(&document(schema, true), &[]).expect_err("the document requires it");
    assert!(
        matches!(&refused, LoadError::Parameter { why: Unsupported::Keyword(found), .. } if found == "allOf"),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "op: parameter `v` is stated with `allOf`, which this CLI does not read as a value, \
         and the document requires it; correct the parameter in an Overlay, or drop its \
         `required`"
    );
}

/// An object or an array is no value, and the reason says so where nothing else
/// is the matter. Where the schema states a keyword that its type does not have,
/// the reason is that keyword, as it is for a scalar: a list of strings that
/// states `minLength` is no list of values to be called one.
#[test]
fn an_object_or_an_array_is_structured_unless_it_states_a_keyword_it_does_not_have() {
    for schema in [
        "{ type: object, properties: { a: { type: string } } }",
        "{ type: object, minProperties: 1, properties: { a: { type: string } } }",
        "{ type: array, items: { type: object } }",
    ] {
        let why = param_of(&loaded(schema)).expect_err(schema);
        assert_eq!(why, Unsupported::Structured, "{schema}");
        assert_eq!(why.to_string(), "neither a value nor a list of values");
    }
    for (schema, keyword) in [
        (
            "{ type: object, allOf: [{ $ref: '#/components/schemas/Code' }] }",
            "allOf",
        ),
        ("{ type: object, pattern: x }", "pattern"),
        (
            "{ type: array, items: { type: string }, pattern: x }",
            "pattern",
        ),
        (
            "{ type: array, items: { type: string }, minLength: 1 }",
            "minLength",
        ),
        (
            "{ type: array, items: { type: string }, enum: [[a]] }",
            "enum",
        ),
        (
            "{ type: array, items: { type: string }, allOf: [{ $ref: '#/components/schemas/Code' }] }",
            "allOf",
        ),
        // The first the schema states, the compositions first.
        (
            "{ type: array, items: { type: string }, pattern: x, oneOf: [{ type: string }] }",
            "oneOf",
        ),
    ] {
        let why = param_of(&loaded(schema)).expect_err(schema);
        assert!(
            matches!(&why, Unsupported::Keyword(found) if found == keyword),
            "{schema}: {why:?}"
        );
        assert!(
            !why.to_string().contains("neither a value"),
            "{schema}: {why}"
        );
    }
}

/// `openapiv3` holds an array as `AnySchema` for a keyword that states nothing:
/// `properties: {}`, `required: []`, `enum: []`, `allOf: []`, `oneOf: []`,
/// `anyOf: []`. It is the list of
/// values it is without the keyword, so it has the list flag its twin has, with
/// the rules of the array, and a required one loads. A keyword that states
/// something is named, as it is above.
#[test]
fn an_array_that_openapiv3_holds_as_any_for_an_empty_keyword_is_a_list() {
    for empty in [
        "properties: {}",
        "required: []",
        "enum: []",
        "allOf: []",
        "oneOf: []",
        "anyOf: []",
    ] {
        let schema = format!(
            "{{ type: array, items: {{ type: string }}, {empty}, minItems: 1, maxItems: 3, \
             uniqueItems: true }}"
        );
        for required in [false, true] {
            let doc = Document::load(&document(&schema, required), &[])
                .unwrap_or_else(|why| panic!("{schema}: {why}"));
            let Shape::Flag { list, scalar, .. } =
                operation(&doc, "op").param("v").expect("v").shape()
            else {
                panic!("{schema}: no flag");
            };
            let list = list.as_ref().unwrap_or_else(|| panic!("{schema}: no list"));
            assert_eq!(list.items.min, Some(1), "{schema}");
            assert_eq!(list.items.max, Some(3), "{schema}");
            assert!(list.items.unique, "{schema}");
            assert!(matches!(scalar, Scalar::Text(_)), "{schema}: {scalar:?}");
        }
    }
    // Without the items there is nothing to put on a flag, and the reason is
    // the one an array has.
    let why = param_of(&loaded("{ type: array, properties: {} }")).expect_err("no items");
    assert_eq!(why, Unsupported::Structured);
}

/// The template of a JSON body of this schema, and the paths it leaves out.
fn template_of_body(schema: &str) -> (serde_json::Value, Vec<String>) {
    let doc = Document::load(&operation_with("", "", schema), &[]).expect("the document loads");
    let Body::JsonWhole {
        template: Some(template),
        read_only,
        ..
    } = operation(&doc, "op").body()
    else {
        panic!("a list body goes whole: {:?}", operation(&doc, "op").body());
    };
    (
        serde_json::from_str(template).expect("JSON"),
        read_only.clone(),
    )
}

/// The long help of the subcommand of a JSON body of this schema.
fn long_help_of_body(schema: &str) -> String {
    let doc = Document::load(&operation_with("", "", schema), &[]).expect("the document loads");
    tree::command(operation(&doc, "op"))
        .get_long_about()
        .expect("a long help")
        .to_string()
}

/// The array is the array in a body too: the template writes one element of it,
/// and takes its `example` less the properties the API sets, as it does for an
/// array that `openapiv3` holds as a type.
#[test]
fn the_template_of_a_body_that_openapiv3_holds_as_any_for_an_empty_keyword_is_a_list() {
    let items = "items: { type: object, required: [sku], properties: { sku: { type: string }, \
                 id: { type: integer, readOnly: true } } }";
    let (template, read_only) = template_of_body(&format!("{{ type: array, enum: [], {items} }}"));
    assert_eq!(template, serde_json::json!([{ "sku": "" }]));
    assert!(read_only.is_empty(), "{read_only:?}");

    let (template, read_only) = template_of_body(&format!(
        "{{ type: array, enum: [], example: [{{ id: 1, sku: a }}], {items} }}"
    ));
    assert_eq!(template, serde_json::json!([{ "sku": "a" }]));
    assert_eq!(read_only, ["[].id"]);
}

/// The long help names the properties of the items that the template leaves out,
/// for a body that is such an array and for a property that a body requires and
/// whose schema is one: `[].id` and `tags[].id`.
#[test]
fn the_read_only_properties_of_the_items_of_such_an_array_are_named_by_their_paths() {
    let list = "{ type: array, enum: [], items: { type: object, required: [id, sku], properties: \
                { sku: { type: string }, id: { type: integer, readOnly: true } } } }";
    let (template, read_only) = template_of_body(list);
    assert_eq!(template, serde_json::json!([{ "sku": "" }]));
    assert_eq!(read_only, ["[].id"]);
    assert!(
        long_help_of_body(list).contains("`[].id` is `readOnly`"),
        "{}",
        long_help_of_body(list)
    );

    let property = format!("{{ type: object, required: [tags], properties: {{ tags: {list} }} }}");
    let (template, read_only) = template_of_body(&property);
    assert_eq!(template, serde_json::json!({ "tags": [{ "sku": "" }] }));
    assert_eq!(read_only, ["tags[].id"]);
    assert!(
        long_help_of_body(&property).contains("`tags[].id` is `readOnly`"),
        "{}",
        long_help_of_body(&property)
    );
}

/// A composition is not a value that fits on a flag, and it names itself.
#[test]
fn a_composition_names_its_keyword() {
    for (schema, keyword) in [
        ("{ oneOf: [{ type: string }, { type: integer }] }", "oneOf"),
        ("{ anyOf: [{ type: string }, { type: integer }] }", "anyOf"),
        (
            "{ allOf: [{ type: string }, { $ref: '#/components/schemas/Code' }] }",
            "allOf",
        ),
        ("{ not: { type: string } }", "not"),
        (
            "{ oneOf: [{ type: string }], properties: { a: { type: string } } }",
            "oneOf",
        ),
        // A composition beside another keyword is `AnySchema` with no `type`, and
        // each of the four is named there.
        ("{ allOf: [{ type: string }], pattern: x }", "allOf"),
        ("{ anyOf: [{ type: string }], pattern: x }", "anyOf"),
        ("{ not: { type: string }, pattern: x }", "not"),
        (
            "{ not: { type: string }, anyOf: [{ type: string }], pattern: x }",
            "anyOf",
        ),
        (
            "{ not: { type: string }, oneOf: [{ type: string }], allOf: [{ type: string }] }",
            "allOf",
        ),
    ] {
        let why = param_of(&loaded(schema)).expect_err(schema);
        assert!(
            matches!(&why, Unsupported::Keyword(found) if found == keyword),
            "{schema}: {why:?}"
        );
    }
}

/// A schema that says nothing of what it is can be any value, and a flag needs to
/// know which. No keyword is to be named, so the reason says there is no `type`.
#[test]
fn a_schema_with_no_type_says_so() {
    for schema in [
        "{}",
        "{ description: anything }",
        "{ enum: [a, b] }",
        "{ pattern: '^a$' }",
        "{ properties: { a: { type: string } } }",
    ] {
        let why = param_of(&loaded(schema)).expect_err(schema);
        assert_eq!(why, Unsupported::Untyped, "{schema}");
        assert_eq!(
            why.to_string(),
            "stated with no `type`, so this CLI cannot tell what it is"
        );
    }
}

/// A `type` that is none of the six is a keyword the schema states and this crate
/// does not read.
#[test]
fn a_type_that_is_none_of_the_six_is_named() {
    let why = param_of(&loaded("{ type: file }")).expect_err("a type nobody reads");
    assert!(
        matches!(&why, Unsupported::Keyword(found) if found == "type"),
        "{why:?}"
    );
}

/// A schema that `openapiv3` holds as a type reads the same way whatever else the
/// crate reads from an `AnySchema`: its scalar is the one its type states.
#[test]
fn a_scalar_that_openapiv3_holds_as_a_type_is_read_by_its_type() {
    let doc = loaded("{ type: integer, minimum: 0, maximum: 10, enum: [1, 2, 3] }");
    let scalar = param_of(&doc).expect("a scalar");
    assert!(scalar.parse("2").is_ok());
    assert!(scalar.parse("4").is_err());
    assert!(property_of(&doc).is_some());
    let doc = loaded("{ type: string, nullable: true, pattern: '^a$' }");
    let scalar = param_of(&doc).expect("a scalar");
    assert!(scalar.parse("a").is_ok());
    assert!(scalar.parse("b").is_err());
}

/// One operation of this test's own: `parameters` is its parameter list, and
/// `gates` what it says in `x-cli-gates`. Its body is `body`, as the JSON schema.
/// The schema `Arr` is a list of strings, for a parameter that points at one.
fn operation_with(parameters: &str, gates: &str, body: &str) -> String {
    format!(
        "openapi: 3.0.3
info: {{ title: t, version: \"1\" }}
servers: [{{ url: 'http://localhost:9999' }}]
paths:
  /x:
    post:
      operationId: op
      x-cli-writes: true
      {gates}
      parameters: [{parameters}]
      requestBody:
        content:
          application/json:
            schema: {body}
      responses: {{ '200': {{ description: OK }} }}
components:
  schemas:
    Arr: {{ type: array, items: {{ type: string }} }}
"
    )
}

/// An operation whose one parameter is `name`, in `place` (`query` or `header`),
/// with `schema`, and whose body is an object with no properties. `gates` is what
/// the operation says in `x-cli-gates`.
fn one_parameter(place: &str, name: &str, schema: &str, gates: &str) -> String {
    operation_with(
        &format!("{{ name: '{name}', in: {place}, schema: {schema} }}"),
        gates,
        "{ type: object }",
    )
}

/// The documents of a rule that cannot run on a scalar that `openapiv3` holds as
/// `AnySchema`, or on a wrapper of one element that a list's items or a JSON body
/// read through.
fn rules_that_cannot_run() -> Vec<(&'static str, String)> {
    let property = |schema: &str| {
        operation_with(
            "",
            "",
            &format!("{{ type: object, properties: {{ v: {schema} }} }}"),
        )
    };
    let parameter = |schema: &str| one_parameter("query", "v", schema, "");
    vec![
        (
            "a step of 0 on a parameter",
            parameter("{ type: integer, minimum: 0.0, multipleOf: 0 }"),
        ),
        (
            "a negative step on a parameter",
            parameter("{ type: integer, minimum: 0.0, multipleOf: -2 }"),
        ),
        (
            "an `enum` of `null` on a parameter",
            parameter("{ type: integer, minimum: 0.0, enum: [null] }"),
        ),
        (
            "a step of 0 on a body property",
            property("{ type: integer, minimum: 0.0, multipleOf: 0 }"),
        ),
        (
            "an `enum` of `null` on a body property",
            property("{ type: integer, minimum: 0.0, enum: [null] }"),
        ),
        (
            "a step of 0 in the items of a wrapped list",
            parameter("{ allOf: [{ type: array, items: { type: integer, multipleOf: 0 } }] }"),
        ),
        (
            "a step of 0 on a property of a wrapped body",
            operation_with(
                "",
                "",
                "{ allOf: [{ type: object, properties: { v: { type: integer, multipleOf: 0 } } }] }",
            ),
        ),
        (
            "an `enum` of `null` on a property of a wrapped body",
            operation_with(
                "",
                "",
                "{ allOf: [{ type: object, properties: { v: { type: string, enum: [null] } } }] }",
            ),
        ),
        (
            "a step of 0 on a property of a body that goes whole",
            operation_with(
                "",
                "",
                "{ type: object, properties: { n: { type: object }, v: { type: integer, minimum: 0.0, multipleOf: 0 } } }",
            ),
        ),
    ]
}

/// A scalar that `openapiv3` holds as `AnySchema` has a flag, so it is held to
/// what every flag is held to, and so is a wrapper of one element, which a list's
/// items and a JSON body read through. A rule that cannot run is refused.
#[test]
fn a_scalar_read_from_an_any_schema_is_held_to_the_rules_that_a_flag_runs() {
    for (label, text) in rules_that_cannot_run() {
        let refused = Document::load(&text, &[]).expect_err(label);
        assert!(is_unrunnable(&refused), "{label}: {refused:?}");
        assert_eq!(refused.to_string(), UNRUNNABLE, "{label}");
    }
}

/// A header parameter that no header is named like has no flag while its schema is
/// read as no value, and the name is refused once the schema is a scalar. The names
/// of consent are held in `a_value_that_has_a_flag_is_held_to_the_words_consent_is_given_by`.
#[test]
fn a_scalar_read_from_an_any_schema_is_held_to_the_name_that_a_header_has() {
    let header = operation_with(
        "{ name: 'X V', in: header, schema: { type: integer, minimum: 0.0 } }",
        "",
        "{ type: object }",
    );
    let refused = Document::load(&header, &[]).expect_err("no header has this name");
    assert!(is_header_name(&refused), "{refused:?}");
    assert_eq!(refused.to_string(), HEADER_NAME);
}

/// A word that consent is given by: the gate `force` of `x-cli-gates`, or the
/// confirmation `commit`.
#[derive(Clone, Copy)]
enum Word {
    Gate,
    Confirmation,
}

impl Word {
    /// What a parameter or a property is called to be named like the word.
    const fn name(self) -> &'static str {
        match self {
            Self::Gate => "force",
            Self::Confirmation => "commit",
        }
    }

    /// What the operation says in `x-cli-gates`: the gate, or nothing.
    const fn gates(self) -> &'static str {
        match self {
            Self::Gate => "x-cli-gates: [force]",
            Self::Confirmation => "",
        }
    }

    /// The refusal's own words, for the `parameter` or the `property` that holds
    /// the name.
    fn words(self, what: &str) -> String {
        match self {
            Self::Gate => format!(
                "op: the {what} `force` and the gate `force` both want `--force`; rename the \
                 gate in `x-cli-gates`, to `gate-force` or another word"
            ),
            Self::Confirmation => format!(
                "op: the {what} `commit` and the confirmation both want `--commit`; confirm \
                 with another word \u{2014} `Loading::commit`, or `Settings::commit_word` where \
                 a bless step generates"
            ),
        }
    }

    /// Whether `refused` is the refusal of this word, for `what` and the name.
    fn refused_by(self, refused: &LoadError, what: &str) -> bool {
        match self {
            Self::Gate => matches!(
                refused,
                LoadError::GateTakenByName { what: found, name, gate, .. }
                    if *found == what && name == "force" && gate == "force"
            ),
            Self::Confirmation => matches!(
                refused,
                LoadError::CommitTakenByName { what: found, name, commit, .. }
                    if *found == what && name == "commit" && commit == "commit"
            ),
        }
    }
}

/// The documents in which a value that has a flag is named like a word that consent
/// is given by. Each is the place that holds the name, the word and the document.
/// The values are a scalar that `openapiv3` holds as `AnySchema`, as a query
/// parameter and as a header; a list of them, an array that states an empty
/// keyword, and a list inside an `allOf` of one element, each as a query
/// parameter; that scalar as a property of a JSON body; and, as a property of a
/// body that is an `allOf` of one element, that scalar and a plain `integer` and
/// `string`.
fn named_like_consent() -> Vec<(&'static str, Word, String)> {
    let scalar = "{ type: integer, minimum: 0.0 }";
    let lists = [
        format!("{{ type: array, items: {scalar} }}"),
        "{ type: array, items: { type: string }, properties: {} }".to_owned(),
        "{ allOf: [{ type: array, items: { type: string } }] }".to_owned(),
        "{ allOf: [{ type: array, items: { type: string }, properties: {} }] }".to_owned(),
    ];
    let mut rows = Vec::new();
    for word in [Word::Gate, Word::Confirmation] {
        let parameter =
            |place: &str, schema: &str| one_parameter(place, word.name(), schema, word.gates());
        let property = |schema: &str, wrapped: bool| {
            let body = format!(
                "{{ type: object, properties: {{ {}: {schema} }} }}",
                word.name()
            );
            let body = if wrapped {
                format!("{{ allOf: [{body}] }}")
            } else {
                body
            };
            operation_with("", word.gates(), &body)
        };
        rows.push(("parameter", word, parameter("query", scalar)));
        rows.push(("parameter", word, parameter("header", scalar)));
        for list in &lists {
            rows.push(("parameter", word, parameter("query", list)));
        }
        rows.push(("property", word, property(scalar, false)));
        // Every scalar property of a body that is an `allOf` of one element has a
        // flag, whatever the scalar is.
        for plain in [scalar, "{ type: integer }", "{ type: string }"] {
            rows.push(("property", word, property(plain, true)));
        }
    }
    rows
}

/// A value that has a flag takes its name from the namespace of its operation, so
/// every value that is read as one is held to the words that consent is given
/// by as any other is: each of the places above refuses the document, and the
/// refusal says its own words.
#[test]
fn a_value_that_has_a_flag_is_held_to_the_words_consent_is_given_by() {
    for (what, word, text) in named_like_consent() {
        let refused = Document::load(&text, &[]).expect_err(what);
        assert!(word.refused_by(&refused, what), "{text}: {refused:?}");
        assert_eq!(refused.to_string(), word.words(what), "{text}");
    }
}

/// The properties of a body, `NAME` for the one that is named like a word that
/// consent is given by, and whether the body is flat, which is whether the load asks
/// the property its name.
///
/// A property "of the first kind" is a scalar that `openapiv3` holds as
/// `AnySchema`, such as `{ type: integer, minimum: 0.0 }`. A property that is no
/// scalar this crate reads is one of `{ type: string, oneOf: [{ type: string }] }`,
/// `{ readOnly: true }` and an `allOf` of two schemas.
const NAMES_ASKED: [(&str, &str, bool); 22] = [
    ("a plain property alone", "NAME: { type: integer }", true),
    (
        "a property of the first kind alone",
        "NAME: { type: integer, minimum: 0.0 }",
        true,
    ),
    (
        "a property of the first kind before a plain one",
        "NAME: { type: integer, minimum: 0.0 }, name: { type: string }",
        true,
    ),
    (
        "a property of the first kind after a plain one",
        "name: { type: string }, NAME: { type: integer, minimum: 0.0 }",
        true,
    ),
    (
        "a plain property after a property of the first kind",
        "a: { type: integer, minimum: 0.0 }, NAME: { type: integer }",
        true,
    ),
    (
        "a plain property after a nested `readOnly` property",
        "a: { type: object, readOnly: true }, NAME: { type: integer }",
        true,
    ),
    (
        "a plain property before a nested `readOnly` property",
        "NAME: { type: integer }, a: { type: object, readOnly: true }",
        true,
    ),
    (
        "a plain property after a `readOnly` list",
        "a: { type: array, items: { type: string }, readOnly: true }, NAME: { type: integer }",
        true,
    ),
    (
        "a plain property after a `readOnly` property whose name has no spelling",
        "'*': { type: string, readOnly: true }, NAME: { type: integer }",
        true,
    ),
    (
        "a plain property after a `readOnly` property of no schema",
        "a: { readOnly: true }, NAME: { type: integer }",
        true,
    ),
    (
        "a plain property after a `readOnly` `oneOf`",
        "a: { type: string, oneOf: [{ type: string }], readOnly: true }, NAME: { type: integer }",
        true,
    ),
    (
        "a plain property after a `readOnly` `allOf` of two",
        "a: { allOf: [{ type: string }, { type: string }], readOnly: true }, NAME: { type: integer }",
        true,
    ),
    (
        "a plain property before a nested one",
        "NAME: { type: integer }, n: { type: object }",
        false,
    ),
    (
        "a property of the first kind before a nested one",
        "NAME: { type: integer, minimum: 0.0 }, n: { type: object }",
        false,
    ),
    (
        "a property before one whose name has no spelling",
        "NAME: { type: integer }, '*': { type: string }",
        false,
    ),
    (
        "a plain property after a nested one",
        "n: { type: object }, NAME: { type: integer }",
        false,
    ),
    (
        "a plain property after one whose name has no spelling",
        "'*': { type: string }, NAME: { type: integer }",
        false,
    ),
    (
        "a property of the first kind after a nested one",
        "n: { type: object }, NAME: { type: integer, minimum: 0.0 }",
        false,
    ),
    (
        "a plain property before a list",
        "NAME: { type: integer }, l: { type: array, items: { type: string } }",
        false,
    ),
    (
        "a plain property after a list",
        "l: { type: array, items: { type: string } }, NAME: { type: integer }",
        false,
    ),
    (
        "a plain property before a schema that is no scalar this crate reads",
        "NAME: { type: integer }, o: { type: string, oneOf: [{ type: string }] }",
        false,
    ),
    (
        "a plain property after a schema that is no scalar this crate reads",
        "o: { type: string, oneOf: [{ type: string }] }, NAME: { type: integer }",
        false,
    ),
];

/// The document whose JSON body has `properties`, with `NAME` the word that consent
/// is given by, and the body wrapped in an `allOf` of one element or not.
fn body_with_properties(properties: &str, word: Word, wrapped: bool) -> String {
    let object = format!(
        "{{ type: object, properties: {{ {} }} }}",
        properties.replace("NAME", word.name())
    );
    let body = if wrapped {
        format!("{{ allOf: [{object}] }}")
    } else {
        object
    };
    operation_with("", word.gates(), &body)
}

/// Each row of `NAMES_ASKED` for each word, bare and wrapped: what the case is,
/// the word, whether the body is flat, and the document.
fn names_asked() -> Vec<(String, Word, bool, String)> {
    let mut rows = Vec::new();
    for (label, properties, flat) in NAMES_ASKED {
        for word in [Word::Gate, Word::Confirmation] {
            for wrapped in [false, true] {
                let case = format!("{label}, `{}`, wrapped: {wrapped}", word.name());
                let text = body_with_properties(properties, word, wrapped);
                rows.push((case, word, flat, text));
            }
        }
    }
    rows
}

/// A property is asked its name only where its body is flat, because only a flat
/// body has a flag for the name to clash with. A body is flat when each property
/// that is not `readOnly` has a flag. A nested property, a list, a schema that is no
/// scalar this crate reads and a name with no spelling each send the body whole
/// wherever they stand in the list, so no property of such a body is asked, the ones
/// listed before it included. A `readOnly` property decides nothing, whatever its
/// schema is. Each row is tried bare and wrapped in an `allOf` of one element. A row
/// whose body is flat is refused with the variant and the words of its word, and a
/// row whose body goes whole loads as `Body::JsonWhole`.
#[test]
fn a_property_is_asked_its_name_only_where_its_body_is_flat() {
    for (case, word, flat, text) in names_asked() {
        let loaded = Document::load(&text, &[]);
        if flat {
            let refused = loaded.expect_err(&case);
            assert!(word.refused_by(&refused, "property"), "{case}: {refused:?}");
            assert_eq!(refused.to_string(), word.words("property"), "{case}");
        } else {
            let doc = loaded.unwrap_or_else(|why| panic!("{case}: {why}"));
            let body = operation(&doc, "op").body();
            assert!(matches!(body, Body::JsonWhole { .. }), "{case}: {body:?}");
        }
    }
}

/// A document that is refused, the variant that refuses it, its words and the
/// words of its cause.
struct Refusal {
    label: String,
    text: String,
    is: fn(&LoadError) -> bool,
    words: &'static str,
    cause: Option<&'static str>,
}

fn is_unrunnable(refused: &LoadError) -> bool {
    matches!(refused, LoadError::Unrunnable { name, .. } if name == "v")
}

fn is_unresolvable(refused: &LoadError) -> bool {
    matches!(
        refused,
        LoadError::Unresolvable { source: RefError::Missing { reference }, .. }
            if reference == "#/components/schemas/Missing"
    )
}

fn is_header_name(refused: &LoadError) -> bool {
    matches!(refused, LoadError::HeaderName { name, .. } if name == "X V")
}

const UNRUNNABLE: &str = "op: `v` states a rule that cannot run";
const STEP: &str = "`0` is not a step: `multipleOf` has to be more than 0";
const ONLY_NULL: &str = "`null` is all that the `enum` lists, and a flag cannot send it";
const UNRESOLVABLE: &str = "op: `v` has a `$ref` that cannot be followed";
const UNRESOLVABLE_BODY: &str = "op: `requestBody` has a `$ref` that cannot be followed";
const MISSING: &str = "`#/components/schemas/Missing` does not resolve";
const HEADER_NAME: &str =
    "op: the header parameter `X V` is not a header name; correct or remove it in an Overlay";

/// A list that has a flag because of how `openapiv3` holds it: an array that
/// states an empty keyword, or a list inside an `allOf` of one element. `items`
/// are its items.
fn list_of(wrapped: bool, items: &str) -> String {
    if wrapped {
        format!("{{ allOf: [{{ type: array, items: {items} }}] }}")
    } else {
        format!("{{ type: array, items: {items}, properties: {{}} }}")
    }
}

/// The query parameter `v` whose schema is such a list.
fn query_of(wrapped: bool, items: &str) -> String {
    one_parameter("query", "v", &list_of(wrapped, items), "")
}

/// The header `X V`, which is no header name, whose schema is such a list.
fn header_of(wrapped: bool) -> String {
    one_parameter("header", "X V", &list_of(wrapped, "{ type: string }"), "")
}

/// The documents of a list that has a flag and is refused for it, as the query
/// parameter `v` with items that no flag can carry and as the header `X V`, for
/// each of the two shapes of such a list.
fn lists_with_a_flag() -> Vec<Refusal> {
    let mut rows = Vec::new();
    for (shape, wrapped) in [
        ("an array with an empty keyword", false),
        ("a list inside an `allOf`", true),
    ] {
        let mut row = |what: &str, text, is, words, cause| {
            rows.push(Refusal {
                label: format!("{shape}: {what}"),
                text,
                is,
                words,
                cause,
            });
        };
        let step = query_of(wrapped, "{ type: integer, multipleOf: 0 }");
        row("a step of 0", step, is_unrunnable, UNRUNNABLE, Some(STEP));
        let only_null = query_of(wrapped, "{ type: integer, enum: [null] }");
        row(
            "an `enum` of `null`",
            only_null,
            is_unrunnable,
            UNRUNNABLE,
            Some(ONLY_NULL),
        );
        let missing = query_of(wrapped, "{ $ref: '#/components/schemas/Missing' }");
        row(
            "a `$ref` that leads nowhere",
            missing,
            is_unresolvable,
            UNRESOLVABLE,
            Some(MISSING),
        );
        row(
            "a header name that no header has",
            header_of(wrapped),
            is_header_name,
            HEADER_NAME,
            None,
        );
    }
    rows
}

/// The documents of a JSON body, or a property of one, whose `$ref` leads nowhere
/// where the template follows it: a body that is an array that states an empty
/// keyword, bare and inside an `allOf` of one element, and a property that the
/// body requires and whose schema is such an array.
fn bodies_the_template_follows() -> Vec<Refusal> {
    let list = "{ type: array, items: { $ref: '#/components/schemas/Missing' }, enum: [] }";
    let row = |what: &str, body: &str| Refusal {
        label: what.to_owned(),
        text: operation_with("", "", body),
        is: is_unresolvable,
        words: UNRESOLVABLE_BODY,
        cause: Some(MISSING),
    };
    vec![
        row("a body that is an array with an empty keyword", list),
        row(
            "the same inside an `allOf`",
            &format!("{{ allOf: [{list}] }}"),
        ),
        row(
            "a required property of a body",
            &format!("{{ type: object, required: [v], properties: {{ v: {list} }} }}"),
        ),
    ]
}

/// The documents of a property whose `$ref` leads nowhere in a JSON body that is an
/// `allOf` of one element. The body is read through the wrapper, so each property of
/// it has a flag to give, and the cause is the wrapper of the body and not of the
/// property: the same property in a body that is no wrapper is refused without it,
/// so the first row holds a bare `$ref`.
fn properties_of_a_wrapped_body() -> Vec<Refusal> {
    let row = |what: &str, property: &str| Refusal {
        label: what.to_owned(),
        text: operation_with(
            "",
            "",
            &format!("{{ allOf: [{{ type: object, properties: {{ v: {property} }} }}] }}"),
        ),
        is: is_unresolvable,
        words: UNRESOLVABLE,
        cause: Some(MISSING),
    };
    vec![
        row(
            "a bare `$ref` that leads nowhere",
            "{ $ref: '#/components/schemas/Missing' }",
        ),
        row(
            "the same inside an `allOf` of its own",
            "{ allOf: [{ $ref: '#/components/schemas/Missing' }] }",
        ),
    ]
}

/// Asserts that every row is refused as it says: the variant, the words, and the
/// words of the cause.
fn assert_refused(rows: impl IntoIterator<Item = Refusal>) {
    for row in rows {
        let refused = Document::load(&row.text, &[]).expect_err(&row.label);
        assert!((row.is)(&refused), "{}: {refused:?}", row.label);
        assert_eq!(refused.to_string(), row.words, "{}", row.label);
        let cause = std::error::Error::source(&refused).map(ToString::to_string);
        assert_eq!(cause.as_deref(), row.cause, "{}", row.label);
    }
}

/// A list that has a flag is held to what every flag is held to: its items'
/// rules must run, a `$ref` in them must lead somewhere, and a header must have a
/// header's name. A JSON body that is such an array, and a property that a body
/// requires and whose schema is one, are followed by the template, so a
/// reference that leads nowhere refuses the document. Each refusal says its own
/// words and its cause's.
#[test]
fn a_list_with_a_flag_is_held_to_what_a_flag_is_held_to() {
    assert_refused(
        lists_with_a_flag()
            .into_iter()
            .chain(bodies_the_template_follows()),
    );
}

/// A JSON body that is an `allOf` of one element is read through the wrapper, so
/// each of its properties is read as the property of a bare body is, and a `$ref`
/// that leads nowhere refuses the document.
#[test]
fn a_property_of_a_wrapped_body_is_held_to_what_a_flag_is_held_to() {
    assert_refused(properties_of_a_wrapped_body());
}

// ---------------------------------------------------------------- the wrapper

/// The single-element `allOf` wrapper, at the three places a schema is read: a
/// list parameter, a JSON body, and a multipart body. The probe `wrapper`'s
/// document, with the multipart body the finding names.
const WRAPPED: &str = "openapi: 3.0.3
info: { title: t, version: \"1\" }
servers: [{ url: 'http://localhost:9999' }]
paths:
  /a:
    post:
      operationId: bare
      parameters:
        - { name: days, in: query, schema: { $ref: '#/components/schemas/Days' } }
        - { name: code, in: query, schema: { $ref: '#/components/schemas/Code' } }
        - { name: tags, in: query, schema: { type: array, items: { $ref: '#/components/schemas/Code' } } }
      requestBody:
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Pet' }
      responses: { '200': { description: OK } }
  /b:
    post:
      operationId: wrapped
      parameters:
        - { name: days, in: query, schema: { allOf: [{ $ref: '#/components/schemas/Days' }], description: Booking days. } }
        - { name: code, in: query, schema: { allOf: [{ $ref: '#/components/schemas/Code' }], description: A code. } }
        - { name: tags, in: query, schema: { type: array, items: { allOf: [{ $ref: '#/components/schemas/Code' }], description: One tag. } } }
      requestBody:
        content:
          application/json:
            schema: { allOf: [{ $ref: '#/components/schemas/Pet' }], description: The pet to create. }
      responses: { '200': { description: OK } }
  /c:
    post:
      operationId: upload
      requestBody:
        content:
          multipart/form-data:
            schema: { allOf: [{ $ref: '#/components/schemas/Form' }], description: The upload. }
      responses: { '200': { description: OK } }
  /d:
    post:
      operationId: uploadBare
      requestBody:
        content:
          multipart/form-data:
            schema: { $ref: '#/components/schemas/Form' }
      responses: { '200': { description: OK } }
components:
  schemas:
    Code: { type: string, pattern: '^[A-Z]{3}$' }
    Days: { type: array, items: { type: string, pattern: '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' } }
    Pet: { type: object, properties: { name: { type: string } } }
    Form: { type: object, properties: { file: { type: string, format: binary }, note: { type: string } } }
";

/// What the parameter is worth, as the note of its flag says it: the list or the
/// scalar, and the rules of one value.
fn described(doc: &Document, id: &str, name: &str) -> String {
    match operation(doc, id)
        .param(name)
        .expect("the parameter")
        .shape()
    {
        Shape::Flag { scalar, list, .. } => {
            format!("Flag (list: {}, note {:?})", list.is_some(), scalar.note())
        }
        Shape::Unreachable(why) => format!("Unreachable ({why})"),
    }
}

/// The wrapper is the same spelling of a reference wherever a schema stands, so a
/// list, a body and a multipart body read through it as a bare reference does.
#[test]
fn a_wrapper_is_read_at_every_place_a_schema_is_read() {
    let doc = Document::load(WRAPPED, &[]).expect("the document loads");
    for name in ["days", "code", "tags"] {
        assert_eq!(
            described(&doc, "wrapped", name),
            described(&doc, "bare", name),
            "parameter `{name}`"
        );
    }
    assert!(
        described(&doc, "wrapped", "days").starts_with("Flag (list: true"),
        "{}",
        described(&doc, "wrapped", "days")
    );

    let flat = |id: &str| -> Vec<String> {
        let body = operation(&doc, id).body();
        assert!(matches!(body, Body::JsonFields { .. }), "{id}: {body:?}");
        body.fields().iter().map(|f| f.name().to_owned()).collect()
    };
    assert_eq!(flat("wrapped"), flat("bare"));
    assert_eq!(flat("wrapped"), ["name"]);

    let parts = |id: &str| -> Vec<String> {
        match operation(&doc, id).body() {
            Body::Multipart { names, .. } => names.clone(),
            Body::None | Body::JsonFields { .. } | Body::JsonWhole { .. } | Body::Opaque { .. } => {
                panic!("{id}: a multipart body")
            }
        }
    };
    assert_eq!(parts("upload"), parts("uploadBare"));
    assert_eq!(parts("upload"), ["file", "note"]);
}

/// A wrapper is followed like a reference, so one whose reference leads nowhere is
/// refused by the name of the body. Reading it as a body with no parts would load
/// a document whose upload names nothing, and the loss would show only when a
/// caller sent a file.
#[test]
fn a_wrapper_whose_reference_leads_nowhere_is_refused_at_a_multipart_body() {
    let refused = Document::load(
        "openapi: 3.0.3
info: { title: t, version: \"1\" }
servers: [{ url: 'http://localhost:9999' }]
paths:
  /c:
    post:
      operationId: upload
      requestBody:
        content:
          multipart/form-data:
            schema: { allOf: [{ $ref: '#/components/schemas/Gone' }] }
      responses: { '200': { description: OK } }
",
        &[],
    )
    .expect_err("the reference leads nowhere");
    assert!(
        matches!(&refused, LoadError::Unresolvable { op, name, .. } if op == "upload" && name == "requestBody"),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "upload: `requestBody` has a `$ref` that cannot be followed"
    );
}

// ----------------------------------------------- what a value with a flag changes

/// The flag of the parameter `name` of the operation `op`, and `None` where it has
/// none.
fn flag_of(doc: &Document, name: &str) -> Option<String> {
    match operation(doc, "op")
        .param(name)
        .expect("the parameter")
        .shape()
    {
        Shape::Flag { flag, .. } => Some(flag.clone()),
        Shape::Unreachable(_) => None,
    }
}

/// The flag of the property `name` of the JSON body of the operation `op`.
fn body_flag_of(doc: &Document, name: &str) -> String {
    let fields = operation(doc, "op").body().fields();
    let field = fields.iter().find(|field| field.name() == name);
    field.expect("the property").flag().to_owned()
}

/// A document in which two things want one flag, and the flag that each of them
/// gets.
struct Claim {
    label: &'static str,
    parameters: &'static str,
    body: &'static str,
    /// The flag of each parameter, by name.
    params: &'static [(&'static str, &'static str)],
    /// The flag of each property of the body, by name.
    fields: &'static [(&'static str, &'static str)],
}

const SCALAR: &str = "{ type: integer, minimum: 0.0 }";
const ID_BODY: &str = "{ type: object, properties: { id: { type: integer } } }";

/// The documents in which a value that has a flag meets a later parameter or a
/// property of the body with the same flag.
const CLAIMS: [Claim; 5] = [
    Claim {
        label: "a scalar, then a property of the body",
        parameters: "{ name: id, in: query, schema: { type: integer, minimum: 0.0 } }",
        body: ID_BODY,
        params: &[("id", "id")],
        fields: &[("id", "body-id")],
    },
    Claim {
        label: "a list inside an `allOf` of one element, then a property of the body",
        parameters: "{ name: id, in: query, schema: { allOf: [{ type: array, items: { type: string } }] } }",
        body: ID_BODY,
        params: &[("id", "id")],
        fields: &[("id", "body-id")],
    },
    Claim {
        label: "an array with an empty keyword, then a property of the body",
        parameters: "{ name: id, in: query, schema: { type: array, items: { type: string }, enum: [] } }",
        body: ID_BODY,
        params: &[("id", "id")],
        fields: &[("id", "body-id")],
    },
    Claim {
        label: "a scalar, then a header",
        parameters: "{ name: id, in: query, schema: { type: integer, minimum: 0.0 } }, \
                     { name: Id, in: header, schema: { type: string } }",
        body: "{ type: object }",
        params: &[("id", "id"), ("Id", "param-id")],
        fields: &[],
    },
    Claim {
        label: "a scalar, then a header with the same kebab-case spelling",
        parameters: "{ name: fooBar, in: query, schema: { type: integer, minimum: 0.0 } }, \
                     { name: foo-bar, in: header, schema: { type: string } }",
        body: "{ type: object }",
        params: &[("fooBar", "foo-bar"), ("foo-bar", "param-foo-bar")],
        fields: &[],
    },
];

/// A flag goes to the first thing that wants it, in the order of the parameters and
/// then the properties of the body. A value that has a flag takes it before the
/// parameters after it and before the properties of the body, so the later one is
/// `--param-<name>` or `--body-<name>`, and `--id` sets the value that has the
/// flag.
#[test]
fn a_value_that_has_a_flag_takes_it_before_the_things_after_it() {
    for claim in CLAIMS {
        let text = operation_with(claim.parameters, "", claim.body);
        let doc = Document::load(&text, &[]).unwrap_or_else(|why| panic!("{}: {why}", claim.label));
        for (name, flag) in claim.params {
            assert_eq!(
                flag_of(&doc, name).as_deref(),
                Some(*flag),
                "{}: {name}",
                claim.label
            );
        }
        for (name, flag) in claim.fields {
            assert_eq!(body_flag_of(&doc, name), *flag, "{}: {name}", claim.label);
        }
    }
}

/// The help of the flag `long` on the subcommand of `op`, or `None` where the
/// subcommand has no such flag.
fn help_of_flag(op: &Operation, long: &str) -> Option<String> {
    tree::command(op)
        .get_arguments()
        .find(|arg| arg.get_long() == Some(long))
        .map(|arg| arg.get_help().map(ToString::to_string).unwrap_or_default())
}

/// A body whose properties all have a flag is flat, and so is one that is an
/// `allOf` of one element around such an object. A body that has a scalar that
/// `openapiv3` holds as `AnySchema` for a property is flat as well. Each has a
/// flag for each property, `--json-body` with the help that calls it the base
/// document, and no `--json-body-template`.
#[test]
fn a_body_whose_properties_all_have_a_flag_is_flat() {
    let plain = "{ type: object, required: [name], properties: { name: { type: string } } }";
    let any = "{ type: object, required: [n, s], properties: \
               { n: { type: integer, minimum: 0.0 }, s: { type: string } } }";
    for (object, flags) in [(plain, &["name"][..]), (any, &["n", "s"][..])] {
        for body in [object.to_owned(), format!("{{ allOf: [{object}] }}")] {
            let doc =
                Document::load(&operation_with("", "", &body), &[]).expect("the document loads");
            let op = operation(&doc, "op");
            assert!(
                matches!(op.body(), Body::JsonFields { .. }),
                "{body}: {:?}",
                op.body()
            );
            let names: Vec<&str> = op
                .body()
                .fields()
                .iter()
                .map(typed_openapi::Field::name)
                .collect();
            assert_eq!(names, flags, "{body}");
            let base = help_of_flag(op, "json-body").expect("the base document");
            assert!(base.contains("It is the base document"), "{body}: {base}");
            assert_eq!(help_of_flag(op, "json-body-template"), None, "{body}");
        }
    }
}

/// A nested property sends the body whole, wrapped or not, and a body that goes
/// whole has the template and the flag that prints it.
#[test]
fn a_body_with_a_nested_property_goes_whole_and_has_the_template() {
    let object = "{ type: object, properties: { name: { type: string }, n: { type: object } } }";
    for body in [object.to_owned(), format!("{{ allOf: [{object}] }}")] {
        let doc = Document::load(&operation_with("", "", &body), &[]).expect("the document loads");
        let op = operation(&doc, "op");
        assert!(
            matches!(
                op.body(),
                Body::JsonWhole {
                    template: Some(_),
                    ..
                }
            ),
            "{body}: {:?}",
            op.body()
        );
        assert!(
            help_of_flag(op, "json-body-template").is_some(),
            "{body}: the flag that prints the template"
        );
    }
}

/// A body that is a scalar goes whole, and its template is the empty value of its
/// type, so the operation has `--json-body-template`: `0` for an integer with a
/// bound spelled as a float.
#[test]
fn a_body_that_is_a_scalar_has_the_template_of_its_type() {
    let (template, read_only) = template_of_body(SCALAR);
    assert_eq!(template, serde_json::json!(0));
    assert!(read_only.is_empty(), "{read_only:?}");

    let doc = Document::load(&operation_with("", "", SCALAR), &[]).expect("the document loads");
    assert!(
        help_of_flag(operation(&doc, "op"), "json-body-template").is_some(),
        "the flag that prints the template"
    );
}

/// A parameter that the document requires loads when its value has a flag: a list
/// inside an `allOf` of one element, bare and over a `$ref` to an array, as a
/// query, a header and a path parameter.
#[test]
fn a_required_list_inside_an_all_of_of_one_element_loads() {
    for place in ["query", "header", "path"] {
        for schema in [
            "{ allOf: [{ type: array, items: { type: string } }] }",
            "{ allOf: [{ $ref: '#/components/schemas/Arr' }] }",
        ] {
            let parameter = format!("{{ name: v, in: {place}, required: true, schema: {schema} }}");
            let path = if place == "path" { "/x/{v}" } else { "/x" };
            let text = operation_with(&parameter, "", "{ type: object }")
                .replace("/x:", &format!("{path}:"));
            let doc =
                Document::load(&text, &[]).unwrap_or_else(|why| panic!("{place}, {schema}: {why}"));
            assert!(flag_of(&doc, "v").is_some(), "{place}, {schema}");
        }
    }
}

/// A parameter whose `x-cli-requires` lists a value that has a flag loads. The
/// value is a scalar that `openapiv3` holds as `AnySchema`, an array with an empty
/// keyword, or a list inside an `allOf` of one element.
#[test]
fn a_parameter_that_requires_a_value_with_a_flag_loads() {
    for schema in [
        SCALAR.to_owned(),
        list_of(false, "{ type: string }"),
        list_of(true, "{ type: string }"),
    ] {
        let parameters = format!(
            "{{ name: a, in: query, schema: {{ type: string }}, x-cli-requires: [v] }}, \
             {{ name: v, in: query, schema: {schema} }}"
        );
        let doc = Document::load(&operation_with(&parameters, "", "{ type: object }"), &[])
            .unwrap_or_else(|why| panic!("{schema}: {why}"));
        assert_eq!(
            operation(&doc, "op").param("a").expect("a").requires(),
            ["v"],
            "{schema}"
        );
    }
}
