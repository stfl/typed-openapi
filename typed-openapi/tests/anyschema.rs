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
use typed_openapi::tree;
use typed_openapi::{Document, LoadError, Scalar};

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

/// The probe's second case: the parameter is required, and the document was
/// refused for a reason that was not true.
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

/// A bound that is no whole number below 2^53 is not the integer a double
/// states, so the bound is refused by its keyword rather than rounded.
#[test]
fn an_integer_bound_that_a_double_does_not_state_exactly_is_refused_by_its_keyword() {
    for (schema, keyword) in [
        ("{ type: integer, minimum: 9223372036854775808 }", "minimum"),
        ("{ type: integer, maximum: 1e30 }", "maximum"),
        ("{ type: integer, minimum: 0.5 }", "minimum"),
        ("{ type: integer, multipleOf: 2.5 }", "multipleOf"),
        ("{ type: integer, maximum: 9007199254740993.0 }", "maximum"),
    ] {
        let doc = loaded(schema);
        let why = param_of(&doc).expect_err(schema);
        assert!(
            matches!(&why, Unsupported::Keyword(found) if found == keyword),
            "{schema}: {why:?}"
        );
        // A body property that has no flag sends the body whole.
        assert!(property_of(&doc).is_none(), "{schema}");
    }
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

/// An object or an array that `openapiv3` holds as `AnySchema` is what it is, and
/// the reason that says so is the true one.
#[test]
fn an_object_or_an_array_is_still_neither_a_value_nor_a_list_of_values() {
    for schema in [
        "{ type: object, properties: { a: { type: string } } }",
        "{ type: object, allOf: [{ $ref: '#/components/schemas/Code' }] }",
        "{ type: array, items: { type: string }, pattern: x }",
    ] {
        let why = param_of(&loaded(schema)).expect_err(schema);
        assert_eq!(why, Unsupported::Structured, "{schema}");
        assert_eq!(why.to_string(), "neither a value nor a list of values");
    }
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

/// What the scalar of a plain schema was before the change is what it is, so the
/// fix reads nothing that `openapiv3` already held as a type in another way.
#[test]
fn a_scalar_that_openapiv3_holds_as_a_type_is_read_as_before() {
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
