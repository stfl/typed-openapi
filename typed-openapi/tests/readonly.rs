//! An operator is never asked for a value the API says not to send.
//!
//! OpenAPI 3.0.3 says of a `readOnly` property that it "SHOULD NOT be sent as part
//! of the request" and that "the required will take effect on the response only".
//! So such a property has no flag, the body template leaves it out, the long help
//! names it, and a type that a request body reaches does not require it. A type
//! that only responses reach keeps the document's `required`. The first document
//! is the input of the probe `A3b/scalars` `readonly` behind F-42; the others are
//! the shapes that probe's finding names. A case that checks what the model holds
//! matches the variant, and a case about the generated types reads the source the
//! bless step writes.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use quote::ToTokens as _;
use serde_json::{Value, json};
use typed_openapi::generate::Settings;
use typed_openapi::model::{Body, Field, Operation};
use typed_openapi::tree::{self, Asked};
use typed_openapi::{Document, Invocation, LoadError, Recorder, Values};

/// A document of this test's own: `paths` and `components` and nothing else.
fn document(paths: &str, schemas: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         paths:\n{paths}\
         components:\n  schemas:\n{schemas}"
    )
}

/// A `POST /pets` whose body is `schema`, and the schemas it points at.
fn creating(schema: &str, schemas: &str) -> String {
    document(
        &format!(
            "  /pets:\n    post:\n      operationId: createPet\n      requestBody:\n        \
             required: true\n        content:\n          application/json:\n            \
             schema: {schema}\n      responses: {{ '201': {{ description: Created }} }}\n"
        ),
        schemas,
    )
}

/// The input of the probe: one schema for create and read, `id` required and
/// read-only, and a `writeOnly` property beside them.
const PET: &str = "    Pet:
      type: object
      required: [id, name]
      properties:
        id: { type: integer, readOnly: true }
        name: { type: string }
        secret: { type: string, writeOnly: true }
";

fn pet() -> String {
    creating("{ $ref: '#/components/schemas/Pet' }", PET)
}

fn loaded(text: &str) -> Document {
    Document::load(text, &[]).expect("the document loads")
}

fn operation<'d>(doc: &'d Document, id: &str) -> &'d Operation {
    doc.get(id).expect("the operation")
}

fn names(fields: &[Field]) -> Vec<&str> {
    fields.iter().map(Field::name).collect()
}

/// The long flags of an operation's subcommand, and its long help.
fn command_of(op: &Operation) -> (Vec<String>, String) {
    let command = tree::command(op);
    let longs = command
        .get_arguments()
        .filter_map(|arg| arg.get_long().map(str::to_owned))
        .collect();
    let help = command.get_long_about().expect("a long help").to_string();
    (longs, help)
}

/// The probe's input: `id` is required and read-only, and the caller is never
/// asked to invent an id that the API says not to send, so `--id` is no flag.
#[test]
fn a_read_only_property_gets_no_flag_even_when_the_schema_requires_it() {
    let doc = loaded(&pet());
    let op = operation(&doc, "createPet");

    let Body::JsonFields {
        fields, read_only, ..
    } = op.body()
    else {
        panic!("a body of scalars is flat: {:?}", op.body());
    };
    assert_eq!(names(fields), ["name", "secret"]);
    assert_eq!(read_only, &["id"]);
    assert_eq!(op.body().read_only(), &["id"]);

    // `required` is the schema's list, and `name` is on it.
    assert!(fields.first().expect("name").required());

    let (longs, _) = command_of(op);
    assert!(!longs.contains(&"id".to_owned()), "{longs:?}");
    assert!(longs.contains(&"name".to_owned()), "{longs:?}");
}

/// The command line the probe could not run: `--id` is demanded on the base.
#[test]
fn the_command_line_asks_for_the_name_and_not_for_the_id() {
    let doc = loaded(&pet());
    let command = || {
        clap::Command::new("t")
            .subcommand_required(true)
            .subcommands(tree::commands(&doc))
    };
    let matches = command()
        .try_get_matches_from(["t", "pets", "create", "--name", "Rex", "--commit"])
        .expect("clap asks for no id");
    let Asked::Run(selection) = tree::select(&doc, &matches).expect("a command") else {
        panic!("the command line runs the operation");
    };
    let client = Recorder::new();
    selection
        .send(&client, doc.base())
        .expect("the request is sent");
    let sent = client.take();
    let body: Value =
        serde_json::from_slice(sent.first().expect("a request").body()).expect("JSON");
    assert_eq!(body, json!({ "name": "Rex" }));

    // The id is a flag of no command, so a caller who types one is refused.
    let refused = command()
        .try_get_matches_from(["t", "pets", "create", "--name", "Rex", "--id", "3"])
        .expect_err("there is no such flag");
    assert_eq!(refused.kind(), ErrorKind::UnknownArgument);
}

/// The typed road is the same: the id is not demanded of the values.
#[test]
fn the_typed_road_asks_for_no_id_either() {
    let doc = loaded(&pet());
    let op = operation(&doc, "createPet");
    Invocation::new(op, Values::new().json(json!({ "name": "Rex" })))
        .expect("a body without the id satisfies the operation");
}

/// A parameter or a flag that vanishes without a word is a defect the long help
/// exists to prevent, so the help says where the flag went.
#[test]
fn the_long_help_names_a_read_only_property() {
    let doc = loaded(&pet());
    let (_, help) = command_of(operation(&doc, "createPet"));
    assert!(
        help.contains(
            "`id` has no flag: it is `readOnly`, so the API sets it and the request should \
             go out without it."
        ),
        "{help}"
    );
    assert!(!help.contains("`name` has no flag"), "{help}");
    assert!(!help.contains("`secret` has no flag"), "{help}");
}

/// The ways a document says that a property is read-only. A `$ref` erases what is
/// written beside it, so a vendor who wants a read-only property of a named type
/// writes the reference inside an `allOf` of one element, with the flag beside it.
const SPELLINGS: [(&str, &str, &str); 4] = [
    ("on the property", "{ type: integer, readOnly: true }", ""),
    (
        "on the schema a `$ref` leads to",
        "{ $ref: '#/components/schemas/Id' }",
        "    Id: { type: integer, readOnly: true }\n",
    ),
    (
        "beside a wrapper of one element",
        "{ allOf: [{ $ref: '#/components/schemas/Id' }], readOnly: true }",
        "    Id: { type: integer }\n",
    ),
    (
        "on the schema inside a wrapper",
        "{ allOf: [{ $ref: '#/components/schemas/Id' }] }",
        "    Id: { type: integer, readOnly: true }\n",
    ),
];

/// A `Pet` whose `id` is required and spelled as `id`, beside the schemas the
/// spelling points at.
fn pet_with(id: &str, extra: &str) -> String {
    creating(
        "{ $ref: '#/components/schemas/Pet' }",
        &format!(
            "    Pet:\n      type: object\n      required: [id, name]\n      properties:\n        \
             id: {id}\n        name: {{ type: string }}\n{extra}"
        ),
    )
}

#[test]
fn every_spelling_of_read_only_takes_the_flag_away() {
    for (label, id, extra) in SPELLINGS {
        let doc = loaded(&pet_with(id, extra));
        let body = operation(&doc, "createPet").body();
        assert!(matches!(body, Body::JsonFields { .. }), "{label}: {body:?}");
        assert_eq!(names(body.fields()), ["name"], "{label}");
        assert_eq!(body.read_only(), &["id"], "{label}");
    }
}

/// What a flag is for is what the document says is sent, so a property that is
/// not marked, or is marked the other way, keeps its flag.
#[test]
fn a_property_that_is_not_read_only_keeps_its_flag() {
    for id in [
        "{ type: integer, readOnly: false }",
        "{ type: integer }",
        "{ type: integer, writeOnly: true }",
        "{ allOf: [{ $ref: '#/components/schemas/Id' }], readOnly: false }",
    ] {
        let doc = loaded(&pet_with(id, "    Id: { type: integer }\n"));
        let body = operation(&doc, "createPet").body();
        assert_eq!(names(body.fields()), ["id", "name"], "{id}");
        assert!(body.read_only().is_empty(), "{id}");
    }
}

/// A body goes whole when a property has no flag for a reason of its own: it is
/// nested, or it has no spelling. A read-only property has no flag because it is
/// not part of the request, so it neither sends the body whole nor needs to be
/// a scalar. A body whose only property with no flag is a read-only one is flat:
/// it has a flag for each of its other properties and `--json-body`, and no
/// `--json-body-template`. A body that goes whole has the flag that prints its
/// template.
#[test]
fn a_read_only_property_does_not_send_the_body_whole() {
    let properties = |key: &str, meta: &str| {
        creating(
            "{ $ref: '#/components/schemas/Pet' }",
            &format!(
                "    Pet:\n      type: object\n      required: [name]\n      properties:\n        \
                 name: {{ type: string }}\n        {key}: {meta}\n        \
                 audit: {{ type: array, items: {{ type: string }}, readOnly: true }}\n"
            ),
        )
    };
    let nested = "{ type: object, properties: { a: { type: string } } }";

    let doc = loaded(&properties("meta", nested));
    let body = operation(&doc, "createPet").body();
    assert!(matches!(body, Body::JsonWhole { .. }), "{body:?}");
    // Whole, and `audit` is named all the same.
    assert_eq!(body.read_only(), &["audit"]);
    let (longs, _) = command_of(operation(&doc, "createPet"));
    assert!(
        longs.contains(&"json-body-template".to_owned()),
        "{longs:?}"
    );

    for (key, meta, read_only) in [
        (
            "meta",
            "{ type: object, readOnly: true, properties: { a: { type: string } } }",
            &["meta", "audit"],
        ),
        // A name with no spelling would send the body whole, and the API sets it.
        ("'*'", "{ type: string, readOnly: true }", &["*", "audit"]),
    ] {
        let doc = loaded(&properties(key, meta));
        let op = operation(&doc, "createPet");
        assert!(
            matches!(op.body(), Body::JsonFields { .. }),
            "{key}: {:?}",
            op.body()
        );
        assert_eq!(names(op.body().fields()), ["name"], "{key}");
        assert_eq!(op.body().read_only(), read_only, "{key}");
        let (longs, _) = command_of(op);
        assert!(longs.contains(&"name".to_owned()), "{key}: {longs:?}");
        assert!(longs.contains(&"json-body".to_owned()), "{key}: {longs:?}");
        assert!(
            !longs.contains(&"json-body-template".to_owned()),
            "{key}: {longs:?}"
        );
    }
}

/// A read-only property claims no flag, so a later property with the same
/// spelling takes it: `fooBar` and `foo-bar` are both `--foo-bar`, and the one the
/// API sets does not hold it. Without `readOnly` the first holds it and the second
/// is `--body-foo-bar`.
#[test]
fn a_read_only_property_gives_its_flag_to_a_later_property_with_the_same_spelling() {
    let body = |read_only: &str| {
        creating(
            &format!(
                "{{ type: object, properties: {{ fooBar: {{ type: string{read_only} }}, \
                 foo-bar: {{ type: string }} }} }}"
            ),
            "    Unused: { type: string }\n",
        )
    };
    let flags = |read_only: &str| {
        let doc = loaded(&body(read_only));
        let op = operation(&doc, "createPet");
        let flags: Vec<(String, String)> = op
            .body()
            .fields()
            .iter()
            .map(|field| (field.name().to_owned(), field.flag().to_owned()))
            .collect();
        let (longs, _) = command_of(op);
        (flags, longs, op.body().read_only().to_vec())
    };

    let (fields, longs, read_only) = flags(", readOnly: true");
    assert_eq!(fields, [("foo-bar".to_owned(), "foo-bar".to_owned())]);
    assert_eq!(read_only, ["fooBar"]);
    assert!(longs.contains(&"foo-bar".to_owned()), "{longs:?}");
    assert!(!longs.contains(&"body-foo-bar".to_owned()), "{longs:?}");

    let (fields, longs, read_only) = flags("");
    assert_eq!(
        fields,
        [
            ("fooBar".to_owned(), "foo-bar".to_owned()),
            ("foo-bar".to_owned(), "body-foo-bar".to_owned()),
        ]
    );
    assert!(read_only.is_empty(), "{read_only:?}");
    assert!(longs.contains(&"body-foo-bar".to_owned()), "{longs:?}");
}

/// The skeleton `--json-body-template` prints names the keys the document
/// demands, and a key the API sets is not one of them. It is left out at every
/// depth, whether the object stands inline, behind a `$ref` or in a list.
#[test]
fn the_body_template_leaves_a_read_only_property_out_at_every_depth() {
    let doc = loaded(&creating(
        "{ $ref: '#/components/schemas/Order' }",
        "    Order:
      type: object
      required: [id, lines, owner, stamp]
      properties:
        id: { type: integer, readOnly: true }
        stamp: { allOf: [{ $ref: '#/components/schemas/Stamp' }], readOnly: true }
        owner: { allOf: [{ $ref: '#/components/schemas/Owner' }], description: Who. }
        lines:
          type: array
          items: { $ref: '#/components/schemas/Line' }
    Owner:
      type: object
      required: [id, name]
      properties:
        id: { type: integer, readOnly: true }
        name: { type: string }
    Stamp: { type: object, properties: { at: { type: string } } }
    Line:
      type: object
      required: [sku, number]
      properties:
        sku: { type: string }
        number: { type: integer, readOnly: true }
",
    ));
    let op = operation(&doc, "createPet");
    let Body::JsonWhole {
        template: Some(template),
        read_only,
        ..
    } = op.body()
    else {
        panic!("a nested body goes whole: {:?}", op.body());
    };
    let template: Value = serde_json::from_str(template).expect("JSON");
    assert_eq!(
        template,
        json!({ "owner": { "name": "" }, "lines": [{ "sku": "" }] })
    );
    // The body's own object comes first, and then what the template leaves out
    // below it.
    assert_eq!(read_only, &["id", "stamp", "owner.id", "lines[].number"]);
    let (_, help) = command_of(op);
    for path in ["id", "stamp", "owner.id", "lines[].number"] {
        assert!(
            help.contains(&format!(
                "`{path}` is `readOnly`: the API sets it, so the body should go out without it."
            )),
            "{path}: {help}"
        );
    }
}

/// A body that goes whole is a file the operator writes, so the help names every
/// property the template leaves out, by its path, and not only the ones of the
/// body's own object. A property that the document does not require is one the
/// template never writes, so it is left out of nothing and is not named.
#[test]
fn the_long_help_names_each_read_only_property_the_template_leaves_out_by_its_path() {
    let doc = loaded(&creating(
        "{ $ref: '#/components/schemas/Order' }",
        "    Order:
      type: object
      required: [name, address, positions]
      properties:
        id: { type: integer, readOnly: true }
        name: { type: string }
        address:
          type: object
          required: [aid, city]
          properties:
            aid: { type: integer, readOnly: true }
            city: { type: string }
            stamp: { type: string, readOnly: true }
        positions:
          type: array
          items:
            type: object
            required: [id, qty]
            properties:
              id: { type: integer, readOnly: true }
              qty: { type: integer }
",
    ));
    let op = operation(&doc, "createPet");
    assert_eq!(
        op.body().read_only(),
        &["id", "address.aid", "positions[].id"]
    );
    let (_, help) = command_of(op);
    for path in ["id", "address.aid", "positions[].id"] {
        assert!(
            help.contains(&format!("`{path}` is `readOnly`")),
            "{path}: {help}"
        );
    }
    assert!(!help.contains("address.stamp"), "{help}");

    // A body that is a list names the properties of its items.
    let doc = loaded(&creating(
        "{ type: array, items: { $ref: '#/components/schemas/Line' } }",
        "    Line:
      type: object
      required: [id, sku]
      properties:
        id: { type: integer, readOnly: true }
        sku: { type: string }
",
    ));
    let op = operation(&doc, "createPet");
    assert_eq!(op.body().read_only(), &["[].id"]);
    let (_, help) = command_of(op);
    assert!(help.contains("`[].id` is `readOnly`"), "{help}");
}

/// The template of the body of `createPet` and the paths it leaves out, for a
/// body that goes whole.
fn template_of(doc: &Document) -> (Value, Vec<String>) {
    let op = operation(doc, "createPet");
    let Body::JsonWhole {
        template: Some(template),
        read_only,
        ..
    } = op.body()
    else {
        panic!("a nested body goes whole: {:?}", op.body());
    };
    (
        serde_json::from_str(template).expect("JSON"),
        read_only.clone(),
    )
}

/// An `example` is the better source of a template, and it is taken as the
/// document states it, less the properties the API sets: a template that carried
/// one would invite the operator to send what a request should not. The first
/// document has an example on the body's own object, and the second one on a
/// nested object.
#[test]
fn the_body_template_takes_an_example_less_the_properties_the_api_sets() {
    let doc = loaded(&creating(
        "{ $ref: '#/components/schemas/Pet' }",
        "    Pet:
      type: object
      required: [id, name, address]
      example: { id: 7, name: Rex, address: { street: Main } }
      properties:
        id: { type: integer, readOnly: true }
        name: { type: string }
        address: { type: object, properties: { street: { type: string } } }
",
    ));
    let (template, read_only) = template_of(&doc);
    assert_eq!(
        template,
        json!({ "name": "Rex", "address": { "street": "Main" } })
    );
    assert_eq!(read_only, ["id"]);

    let doc = loaded(&creating(
        "{ $ref: '#/components/schemas/Pet' }",
        "    Pet:
      type: object
      required: [name, address]
      properties:
        name: { type: string }
        address:
          type: object
          required: [aid, street]
          example: { aid: 9, street: Main }
          properties:
            aid: { type: integer, readOnly: true }
            street: { type: string }
",
    ));
    let (template, read_only) = template_of(&doc);
    assert_eq!(
        template,
        json!({ "name": "", "address": { "street": "Main" } })
    );
    assert_eq!(read_only, ["address.aid"]);
    let (_, help) = command_of(operation(&doc, "createPet"));
    assert!(help.contains("`address.aid` is `readOnly`"), "{help}");
}

/// The example is walked beside its schema at every depth of the value, through
/// a list. A property is dropped for each spelling of read-only (its own flag, a
/// bare reference to a read-only schema, a flag on a wrapper of one element, and
/// a wrapper over a read-only reference), whether the document requires it or
/// not. A key the schema does not declare stays. A property of several elements
/// of a list is named once.
#[test]
fn an_example_loses_a_read_only_property_at_every_depth_of_the_value() {
    let doc = loaded(&creating(
        "{ $ref: '#/components/schemas/Order' }",
        "    Order:
      type: object
      required: [name, positions, nested]
      properties:
        name: { type: string }
        positions:
          type: array
          example:
            - { id: 1, qty: 2, stamp: s, tag: t }
            - { id: 3, qty: 4, note: n, stamp: u, tag: v }
          items: { $ref: '#/components/schemas/Line' }
        nested:
          type: object
          example: { one: { id: 6, two: { id: 5, keep: k } }, free: 1 }
          properties:
            one:
              type: object
              properties:
                id: { type: integer, readOnly: true }
                two:
                  type: object
                  properties:
                    id: { allOf: [{ $ref: '#/components/schemas/Id' }] }
                    keep: { type: string }
    Line:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
        qty: { type: integer }
        stamp: { allOf: [{ type: string }], readOnly: true }
        tag: { $ref: '#/components/schemas/Id' }
    Id: { type: integer, readOnly: true }
",
    ));
    let (template, read_only) = template_of(&doc);
    assert_eq!(
        template,
        json!({
            "name": "",
            "positions": [{ "qty": 2 }, { "qty": 4, "note": "n" }],
            "nested": { "one": { "two": { "keep": "k" } }, "free": 1 },
        })
    );
    assert_eq!(
        read_only,
        [
            "positions[].id",
            "positions[].stamp",
            "positions[].tag",
            "nested.one.id",
            "nested.one.two.id",
        ]
    );
}

/// The schema of an example is read as a value's schema is, so a wrapper of one
/// element leads to the named schema whose read-only properties the example
/// loses, at a property and at the items of a list.
#[test]
fn an_example_under_a_wrapper_of_one_element_loses_the_read_only_properties_of_what_it_wraps() {
    let doc = loaded(&creating(
        "{ $ref: '#/components/schemas/Pet' }",
        "    Pet:
      type: object
      required: [name, address, lines]
      properties:
        name: { type: string }
        address:
          allOf: [{ $ref: '#/components/schemas/Address' }]
          example: { aid: 9, street: Main }
        lines:
          type: array
          example: [{ lid: 1, sku: a }]
          items: { allOf: [{ $ref: '#/components/schemas/Line' }] }
    Address:
      type: object
      properties:
        aid: { type: integer, readOnly: true }
        street: { type: string }
    Line:
      type: object
      properties:
        lid: { type: integer, readOnly: true }
        sku: { type: string }
",
    ));
    let (template, read_only) = template_of(&doc);
    assert_eq!(
        template,
        json!({
            "name": "",
            "address": { "street": "Main" },
            "lines": [{ "sku": "a" }],
        })
    );
    assert_eq!(read_only, ["address.aid", "lines[].lid"]);
}

/// A body that is a list has a list for an example, and the items' schema says
/// which properties of each element the API sets.
#[test]
fn the_example_of_a_body_that_is_a_list_loses_the_read_only_properties_of_its_items() {
    let doc = loaded(&creating(
        "{ type: array, example: [{ id: 1, sku: a }], items: { $ref: '#/components/schemas/Line' } }",
        "    Line:
      type: object
      properties:
        id: { type: integer, readOnly: true }
        sku: { type: string }
",
    ));
    let op = operation(&doc, "createPet");
    let Body::JsonWhole {
        template: Some(template),
        read_only,
        ..
    } = op.body()
    else {
        panic!("a list body goes whole: {:?}", op.body());
    };
    let template: Value = serde_json::from_str(template).expect("JSON");
    assert_eq!(template, json!([{ "sku": "a" }]));
    assert_eq!(read_only, &["[].id"]);
}

/// A template is no place to refuse a document that loads, so an example under a
/// reference that leads nowhere is taken whole, as it is where the schema cannot
/// say anything about it.
#[test]
fn an_example_under_a_reference_that_leads_nowhere_is_taken_whole() {
    let doc = loaded(&creating(
        "{ $ref: '#/components/schemas/Pet' }",
        "    Pet:
      type: object
      required: [name, address]
      properties:
        name: { type: string }
        address:
          type: object
          example: { owner: { id: 1 }, street: Main }
          properties:
            owner: { $ref: '#/components/schemas/Missing' }
            street: { type: string }
",
    ));
    let (template, read_only) = template_of(&doc);
    assert_eq!(
        template,
        json!({ "name": "", "address": { "owner": { "id": 1 }, "street": "Main" } })
    );
    assert!(read_only.is_empty(), "{read_only:?}");
}

/// The named schemas that the documents of `examples_the_walk_does_not_enter`
/// point at, each with a read-only property.
const WALK_SCHEMAS: &str = "    Pet:
      type: object
      properties:
        id: { type: integer, readOnly: true }
        name: { type: string }
    Address:
      type: object
      properties:
        aid: { type: integer, readOnly: true }
        street: { type: string }
    Sub:
      type: object
      properties:
        sid: { type: integer, readOnly: true }
        s: { type: string }
";

/// The documents in which the example walk does not go because of the schema
/// beside the example: a composition (an `allOf` of two schemas, an `anyOf`, a
/// `oneOf` at a property), an object that `openapiv3` holds as `AnySchema`, or an
/// example with a key that `properties` does not declare. Each is the label, the
/// document and the example, which the template is.
fn examples_the_walk_does_not_enter() -> Vec<(&'static str, String, Value)> {
    let named = |schema: &str| creating(schema, WALK_SCHEMAS);
    vec![
        (
            "an allOf of two schemas",
            named(
                "{ allOf: [{ $ref: '#/components/schemas/Pet' }, { type: object, properties: \
                 { note: { type: string } } }], example: { id: 7, name: Rex, note: x } }",
            ),
            json!({ "id": 7, "name": "Rex", "note": "x" }),
        ),
        (
            "an anyOf",
            named(
                "{ anyOf: [{ $ref: '#/components/schemas/Pet' }], example: { id: 7, name: Rex } }",
            ),
            json!({ "id": 7, "name": "Rex" }),
        ),
        (
            "a oneOf at a property",
            named(
                "{ type: object, required: [addr], example: { addr: { aid: 1, street: Main } }, \
                 properties: { addr: { oneOf: [{ $ref: '#/components/schemas/Address' }] } } }",
            ),
            json!({ "addr": { "aid": 1, "street": "Main" } }),
        ),
        (
            "an object that openapiv3 holds as AnySchema",
            named(
                "{ type: object, enum: [], required: [id, name], example: { id: 7, name: Rex }, \
                 properties: { id: { type: integer, readOnly: true }, name: { type: string } } }",
            ),
            json!({ "id": 7, "name": "Rex" }),
        ),
        (
            "a key that properties does not declare",
            named(
                "{ type: object, required: [name], example: { name: Rex, foo: { sid: 1, s: a } }, \
                 properties: { name: { type: string }, nested: { type: object } }, \
                 additionalProperties: { $ref: '#/components/schemas/Sub' } }",
            ),
            json!({ "name": "Rex", "foo": { "sid": 1, "s": "a" } }),
        ),
    ]
}

/// The documents in which the example walk does not go because the schema beside
/// the example is no object and no array that this crate reads: an array that
/// states a keyword its `type` does not have, a schema that has `properties` and no
/// `type`, as the body and as a required property, and a `not`. Each is the label,
/// the document and the example, which the template is.
fn examples_beside_a_schema_that_is_not_read() -> Vec<(&'static str, String, Value)> {
    let named = |schema: &str| creating(schema, WALK_SCHEMAS);
    let untyped = "properties: { id: { type: integer, readOnly: true }, name: { type: string } }";
    let rex = json!({ "id": 1, "name": "Rex" });
    vec![
        (
            "an array that states a keyword its type does not have",
            named(
                "{ type: array, minLength: 1, items: { $ref: '#/components/schemas/Pet' }, \
                 example: [{ id: 1, name: Rex }] }",
            ),
            json!([rex]),
        ),
        (
            "properties and no type",
            named(&format!("{{ {untyped}, example: {{ id: 1, name: Rex }} }}")),
            rex.clone(),
        ),
        (
            "properties and no type, as a required property",
            named(&format!(
                "{{ type: object, required: [pet], properties: {{ pet: {{ {untyped}, \
                 example: {{ id: 1, name: Rex }} }} }} }}"
            )),
            json!({ "pet": rex }),
        ),
        (
            "a not",
            named("{ not: { type: string }, example: { id: 1, name: Rex } }"),
            rex,
        ),
    ]
}

/// The example walk goes through an object and a list that this crate reads, and
/// into the keys that `properties` declares. Wherever it does not go, because the
/// schema beside the example is no object and no list that this crate reads or the
/// example has no key of the schema, the example is the template as the document
/// has it, a `readOnly` key in it included, and no path is named: the template is a
/// skeleton to read, and the walk refuses nothing it cannot follow.
#[test]
fn an_example_stays_as_the_document_has_it_where_the_walk_does_not_go() {
    for (label, text, example) in examples_the_walk_does_not_enter()
        .into_iter()
        .chain(examples_beside_a_schema_that_is_not_read())
    {
        let (template, read_only) = template_of(&loaded(&text));
        assert_eq!(template, example, "{label}");
        assert!(read_only.is_empty(), "{label}: {read_only:?}");
    }
}

/// An object that `openapiv3` holds as `AnySchema`, here one that states an empty
/// `enum`, is no object to this crate, so a body that is one goes whole as a body
/// does for any shape it does not read: no flag, no template, and no name for the
/// `readOnly` property that it states. With an `example` the template is the
/// example as the document has it, which
/// `an_example_stays_as_the_document_has_it_where_the_walk_does_not_go` holds.
#[test]
fn a_body_that_openapiv3_holds_as_any_schema_goes_whole_and_names_no_read_only_property() {
    let doc = loaded(&creating(
        "{ type: object, enum: [], required: [id, name], properties: { id: { type: integer, \
         readOnly: true }, name: { type: string } } }",
        PET,
    ));
    let body = operation(&doc, "createPet").body();
    assert!(
        matches!(body, Body::JsonWhole { template: None, read_only, .. } if read_only.is_empty()),
        "{body:?}"
    );
}

/// A property no flag could carry is read for no rule. A step of 0 is the one
/// rule that only a flag runs, so a read-only property that states it loads; the
/// same body with the property sent is refused.
#[test]
fn a_read_only_property_is_read_for_no_rule_of_a_flag() {
    let body = |id: &str| {
        creating(
            "{ $ref: '#/components/schemas/Pet' }",
            &format!(
                "    Pet:\n      type: object\n      properties:\n        id: {id}\n        \
                 name: {{ type: string }}\n"
            ),
        )
    };
    let doc = loaded(&body("{ type: integer, multipleOf: 0, readOnly: true }"));
    let op = operation(&doc, "createPet");
    assert_eq!(names(op.body().fields()), ["name"]);
    assert_eq!(op.body().read_only(), &["id"]);

    let refused = Document::load(&body("{ type: integer, multipleOf: 0 }"), &[])
        .expect_err("a step that admits no value");
    assert!(
        matches!(refused, LoadError::Unrunnable { .. }),
        "{refused:?}"
    );
}

/// A property no flag could carry takes no name from the namespace of its
/// operation, so a read-only property may be named `force` beside the gate `force`
/// and `commit` beside the confirmation. The same property without `readOnly` has a
/// flag, and the load refuses the name: `force` as the gate, `commit` as the
/// confirmation, each with its own words.
#[test]
fn a_read_only_property_may_be_named_like_a_word_of_consent() {
    let body = |name: &str, read_only: &str| {
        document(
            &format!(
                "  /pets:\n    post:\n      operationId: createPet\n      \
                 x-cli-gates: [force]\n      requestBody:\n        content:\n          \
                 application/json:\n            schema: {{ type: object, properties: \
                 {{ {name}: {{ type: integer{read_only} }} }} }}\n      \
                 responses: {{ '201': {{ description: Created }} }}\n"
            ),
            "    Unused: { type: string }\n",
        )
    };
    for name in ["force", "commit"] {
        let doc = loaded(&body(name, ", readOnly: true"));
        let op = operation(&doc, "createPet");
        assert!(op.body().fields().is_empty(), "{name}");
        assert_eq!(op.body().read_only(), [name]);
    }

    let gate = Document::load(&body("force", ""), &[]).expect_err("a flag named like the gate");
    assert!(
        matches!(
            &gate,
            LoadError::GateTakenByName { what: "property", name, gate, .. }
                if name == "force" && gate == "force"
        ),
        "{gate:?}"
    );
    assert_eq!(
        gate.to_string(),
        "createPet: the property `force` and the gate `force` both want `--force`; rename \
         the gate in `x-cli-gates`, to `gate-force` or another word"
    );

    let commit =
        Document::load(&body("commit", ""), &[]).expect_err("a flag named like the confirmation");
    assert!(
        matches!(
            &commit,
            LoadError::CommitTakenByName { what: "property", name, commit, .. }
                if name == "commit" && commit == "commit"
        ),
        "{commit:?}"
    );
    assert_eq!(
        commit.to_string(),
        "createPet: the property `commit` and the confirmation both want `--commit`; confirm \
         with another word \u{2014} `Loading::commit`, or `Settings::commit_word` where a \
         bless step generates"
    );
}

/// The pattern of a read-only property is a rule of the document that a generated
/// type runs on a response, so it is refused where no engine runs it. The
/// refusal is the walk over every schema, which names the schema as an Overlay
/// names a target, and not the flag, which the property does not have.
#[test]
fn a_pattern_no_engine_runs_is_refused_on_a_read_only_property_too() {
    let refused = Document::load(
        &creating(
            "{ $ref: '#/components/schemas/Pet' }",
            "    Pet:
      type: object
      properties:
        code: { type: string, readOnly: true, pattern: '^\\p{L}+$' }
        name: { type: string }
",
        ),
        &[],
    )
    .expect_err("a pattern that reads another way with the Unicode flag");
    assert!(
        matches!(&refused, LoadError::UnrunnablePattern { at, .. } if at == "$.components.schemas.Pet.properties.code"),
        "{refused:?}"
    );
}

// ---------------------------------------------------------- generated types

/// A directory of this test's own, emptied first.
fn out(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    drop(std::fs::remove_dir_all(&dir));
    dir
}

/// The `src/types.rs` that a bless step writes for `text`.
fn types_of(name: &str, text: &str) -> String {
    let dir = out(name);
    std::fs::create_dir_all(&dir).expect("a directory");
    let path = dir.join("doc.yaml");
    std::fs::write(&path, text).expect("the document");
    Settings::new(&path)
        .write_to(&dir)
        .expect("the document generates");
    std::fs::read_to_string(dir.join("src/types.rs")).expect("the types")
}

/// The source of `pub struct <name> { … }`, from its first line to its closing
/// brace at the left margin.
fn struct_of<'t>(types: &'t str, name: &str) -> &'t str {
    let head = format!("pub struct {name} {{");
    let start = types
        .find(&head)
        .unwrap_or_else(|| panic!("no `{head}` in\n{types}"));
    let rest = types.get(start..).expect("a boundary");
    let end = rest.find("\n}\n").expect("the end of the struct");
    rest.get(..end).expect("a boundary")
}

/// The type of the field `field` in a struct, as the source spells it.
fn field_type<'t>(body: &'t str, field: &str) -> &'t str {
    let head = format!("pub {field}: ");
    let start = body
        .find(&head)
        .unwrap_or_else(|| panic!("no `{head}` in\n{body}"));
    let rest = body.get(start + head.len()..).expect("a boundary");
    rest.split(',').next().expect("a type").trim()
}

const OPTIONAL_ID: &str = "::std::option::Option<i64>";

/// Whether a field is an `Option`, whatever it holds: a property that points at
/// a named schema holds that schema's type.
fn optional(ty: &str) -> bool {
    ty.starts_with("::std::option::Option<")
}

/// The probe's other half: the generated type of a request body does not require
/// `id`, so a body that leaves it out can be built.
#[test]
fn a_type_a_request_body_reaches_does_not_require_a_read_only_property() {
    let types = types_of("readonly-reached", &pet());
    let pet = struct_of(&types, "Pet");
    assert_eq!(field_type(pet, "id"), OPTIONAL_ID, "{pet}");
    assert_eq!(field_type(pet, "name"), "::std::string::String", "{pet}");
}

/// The same spellings as the model reads, so the two roads cannot part: the flag
/// is gone exactly where the type stops requiring the property.
#[test]
fn every_spelling_of_read_only_is_optional_in_the_type() {
    for (label, id, extra) in SPELLINGS {
        let types = types_of("readonly-spellings", &pet_with(id, extra));
        let pet = struct_of(&types, "Pet");
        assert!(optional(field_type(pet, "id")), "{label}:\n{pet}");
    }
    for id in [
        "{ type: integer, readOnly: false }",
        "{ type: integer, writeOnly: true }",
        "{ allOf: [{ $ref: '#/components/schemas/Id' }], readOnly: false }",
    ] {
        let types = types_of(
            "readonly-controls",
            &pet_with(id, "    Id: { type: integer }\n"),
        );
        let pet = struct_of(&types, "Pet");
        assert!(!optional(field_type(pet, "id")), "{id}:\n{pet}");
    }
}

/// Whether the type refuses a body that leaves out the key of `field` in the
/// struct `name`. The type of the field is an `Option` whether or not its key
/// may be missing. typify gives an `Option` whose key must be there a
/// `deserialize_with`, which asks serde for the key, and one whose key may be
/// missing none, so serde reads the missing key as `None`. The attributes are
/// the ones `syn` parses, so a `#[serde(..)]` that `rustfmt` wraps over several
/// lines is read whole.
fn requires_the_key(types: &str, name: &str, field: &str) -> bool {
    let file: syn::File = syn::parse_str(types).expect("the generated types parse");
    let found = file.items.iter().find_map(|item| {
        if let syn::Item::Struct(found) = item
            && found.ident == name
        {
            Some(found)
        } else {
            None
        }
    });
    found
        .expect("the struct is generated")
        .fields
        .iter()
        .find(|candidate| candidate.ident.as_ref().is_some_and(|ident| ident == field))
        .expect("the field is generated")
        .attrs
        .iter()
        .any(|attr| {
            attr.path().is_ident("serde")
                && attr
                    .to_token_stream()
                    .to_string()
                    .contains("deserialize_with")
        })
}

/// The ways a property is nullable and the API sets it, one for each of the four
/// of `SPELLINGS`. A `$ref` erases the word beside it, and `allOf` of the one
/// reference is the spelling that keeps it, so the wrapper is where `nullable`
/// and the flag stand together beside a reference.
const NULLABLE_SPELLINGS: [(&str, &str, &str); 4] = [
    (
        "on the property",
        "{ type: integer, readOnly: true, nullable: true }",
        "",
    ),
    (
        "on the schema a `$ref` leads to",
        "{ $ref: '#/components/schemas/Id' }",
        "    Id: { type: integer, readOnly: true, nullable: true }\n",
    ),
    (
        "beside a wrapper of one element",
        "{ allOf: [{ $ref: '#/components/schemas/Id' }], readOnly: true, nullable: true }",
        "    Id: { type: integer }\n",
    ),
    (
        "on the schema inside a wrapper",
        "{ allOf: [{ $ref: '#/components/schemas/Id' }], nullable: true }",
        "    Id: { type: integer, readOnly: true }\n",
    ),
];

/// A property the API sets and that admits `null` is an `Option` the request body
/// does not require, whichever way the document spells it: the flag is gone from
/// the command line exactly where the type stops requiring the key, and `null`
/// does not bring the requirement back.
#[test]
fn a_nullable_property_the_api_sets_is_not_required_of_the_type_a_request_reaches() {
    for (label, id, extra) in NULLABLE_SPELLINGS {
        let doc = loaded(&pet_with(id, extra));
        assert_eq!(
            operation(&doc, "createPet").body().read_only(),
            &["id"],
            "{label}"
        );
        let types = types_of("readonly-nullable", &pet_with(id, extra));
        let pet = struct_of(&types, "Pet");
        assert!(optional(field_type(pet, "id")), "{label}:\n{pet}");
        assert!(!requires_the_key(&types, "Pet", "id"), "{label}:\n{pet}");
    }
}

/// The control of the test above: a property that admits `null` and that the API
/// does not set keeps its flag and is an `Option` whose key must be there, so
/// `requires_the_key` tells a required `Option` from an optional one and the flag
/// is held in both directions.
#[test]
fn a_nullable_property_the_api_does_not_set_keeps_its_flag_and_is_a_key_that_must_be_there() {
    for id in [
        "{ type: integer, nullable: true }",
        "{ allOf: [{ $ref: '#/components/schemas/Id' }], readOnly: false, nullable: true }",
        "{ allOf: [{ $ref: '#/components/schemas/Id' }], nullable: true }",
    ] {
        let text = pet_with(id, "    Id: { type: integer }\n");
        let doc = loaded(&text);
        let body = operation(&doc, "createPet").body();
        assert_eq!(names(body.fields()), ["id", "name"], "{id}");
        assert!(body.read_only().is_empty(), "{id}");
        let types = types_of("readonly-nullable-control", &text);
        let pet = struct_of(&types, "Pet");
        assert!(optional(field_type(pet, "id")), "{id}:\n{pet}");
        assert!(requires_the_key(&types, "Pet", "id"), "{id}:\n{pet}");
    }
}

/// `required` takes effect on a response (OpenAPI 3.0.3), so a type that only
/// responses reach keeps it: sevdesk-cli's five required read-only properties are
/// all of this kind.
#[test]
fn a_type_only_responses_reach_keeps_the_documents_required() {
    let types = types_of(
        "readonly-response",
        &document(
            "  /pets/{id}:
    get:
      operationId: getPet
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses:
        '200':
          description: OK
          content:
            application/json:
              schema: { $ref: '#/components/schemas/Receipt' }
  /pets:
    post:
      operationId: createPet
      requestBody:
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Pet' }
      responses: { '201': { description: Created } }
",
            &format!(
                "{PET}    Receipt:
      type: object
      required: [id, total]
      properties:
        id: {{ type: integer, readOnly: true }}
        total: {{ type: integer }}
"
            ),
        ),
    );
    let receipt = struct_of(&types, "Receipt");
    assert_eq!(field_type(receipt, "id"), "i64", "{receipt}");
    // The type of the request does not require it.
    assert_eq!(field_type(struct_of(&types, "Pet"), "id"), OPTIONAL_ID);
}

/// One type for the request and the response is the case F-42 is about. The
/// request cannot send what the type requires, and the response may lack what the
/// type then does not require, so the type that both reach stops requiring it.
#[test]
fn a_type_both_a_request_and_a_response_reach_does_not_require_it() {
    let types = types_of(
        "readonly-both",
        &document(
            "  /pets:
    post:
      operationId: createPet
      requestBody:
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Pet' }
      responses:
        '201':
          description: Created
          content:
            application/json:
              schema: { $ref: '#/components/schemas/Pet' }
",
            PET,
        ),
    );
    assert_eq!(field_type(struct_of(&types, "Pet"), "id"), OPTIONAL_ID);
}

/// A request reaches a type through the properties, the items and the
/// compositions of another, and every type on the way stops requiring what the
/// API sets. A type that only a response reaches does not, though it stands in
/// the same document.
#[test]
fn a_type_is_reached_through_the_types_a_request_body_names() {
    const SCHEMAS: &str = "    Order:
      type: object
      required: [id, owner, lines, history, extras]
      properties:
        id: { type: integer, readOnly: true }
        owner: { $ref: '#/components/schemas/Owner' }
        lines: { type: array, items: { $ref: '#/components/schemas/Line' } }
        history: { allOf: [{ $ref: '#/components/schemas/History' }] }
        extras: { type: object, additionalProperties: { $ref: '#/components/schemas/Extra' } }
    Owner:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
    Line:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
    History:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
    Extra:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
    Unrelated:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
";
    let doc = creating("{ $ref: '#/components/schemas/Order' }", SCHEMAS);
    let types = types_of("readonly-reach", &doc);
    for reached in ["Order", "Owner", "Line", "History", "Extra"] {
        let body = struct_of(&types, reached);
        assert_eq!(field_type(body, "id"), OPTIONAL_ID, "{reached}:\n{body}");
    }
    let unrelated = struct_of(&types, "Unrelated");
    assert_eq!(field_type(unrelated, "id"), "i64", "{unrelated}");
}

/// A schema stated where the operation uses it is a type like a named one: the
/// request's loses the requirement, and the response's keeps it. What the stated
/// body refers to is reached by the request, and what only the stated response
/// refers to is not.
#[test]
fn an_inline_request_body_stops_requiring_it_and_an_inline_response_does_not() {
    let types = types_of(
        "readonly-inline",
        &document(
            "  /pets:
    post:
      operationId: createPet
      requestBody:
        content:
          application/json:
            schema:
              type: object
              required: [id, name, owner, tag]
              properties:
                id: { type: integer, readOnly: true }
                name: { type: string }
                tag: { $ref: '#/components/schemas/Tag' }
                owner:
                  type: object
                  required: [id]
                  properties:
                    id: { type: integer, readOnly: true }
      responses:
        '201':
          description: Created
          content:
            application/json:
              schema:
                type: object
                required: [id, stamp]
                properties:
                  id: { type: integer, readOnly: true }
                  stamp: { $ref: '#/components/schemas/Stamp' }
",
            "    Tag:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
    Stamp:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
",
        ),
    );
    let request = struct_of(&types, "CreatePetBody");
    assert_eq!(field_type(request, "id"), OPTIONAL_ID, "{request}");
    assert_eq!(field_type(request, "name"), "::std::string::String");
    // An object stated inside the body is part of the body.
    let owner = struct_of(&types, "CreatePetBodyOwner");
    assert_eq!(field_type(owner, "id"), OPTIONAL_ID, "{owner}");
    let tag = struct_of(&types, "Tag");
    assert_eq!(field_type(tag, "id"), OPTIONAL_ID, "{tag}");
    let response = struct_of(&types, "CreatePetResponse");
    assert_eq!(field_type(response, "id"), "i64", "{response}");
    let stamp = struct_of(&types, "Stamp");
    assert_eq!(field_type(stamp, "id"), "i64", "{stamp}");
}

/// Two operations whose bodies are `$ref`s to `#/components/requestBodies/`: one
/// to a component that names `Pet`, which refers to `Owner`, and one to a chain
/// of two that states its schema inline and refers to `Tag`. `Receipt` is only an
/// answer.
const COMPONENT_BODIES: &str = "openapi: 3.0.3
info: { title: t, version: '1' }
servers: [{ url: 'http://localhost:9999' }]
paths:
  /pets:
    post:
      operationId: createPet
      requestBody: { $ref: '#/components/requestBodies/NewPet' }
      responses:
        '201':
          description: Created
          content:
            application/json:
              schema: { $ref: '#/components/schemas/Receipt' }
  /owners:
    post:
      operationId: createOwner
      requestBody: { $ref: '#/components/requestBodies/Alias' }
      responses: { '201': { description: Created } }
components:
  requestBodies:
    NewPet:
      required: true
      content:
        application/json:
          schema: { $ref: '#/components/schemas/Pet' }
    Alias: { $ref: '#/components/requestBodies/Stated' }
    Stated:
      content:
        application/json:
          schema:
            type: object
            required: [id, tag]
            properties:
              id: { type: integer, readOnly: true }
              tag: { $ref: '#/components/schemas/Tag' }
  schemas:
    Pet:
      type: object
      required: [id, owner]
      properties:
        id: { type: integer, readOnly: true }
        owner: { $ref: '#/components/schemas/Owner' }
    Owner:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
    Tag:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
    Receipt:
      type: object
      required: [id]
      properties:
        id: { type: integer, readOnly: true }
";

/// A request body that is a `$ref` to `#/components/requestBodies/` reaches what
/// the component names or states, and everything those refer to, as a body stated
/// on the operation does. A type that only such a body reaches stops requiring the
/// property, and a type that only a response reaches does not.
#[test]
fn a_body_behind_a_component_reaches_the_types_it_names_and_the_types_those_name() {
    let types = types_of("readonly-component-body", COMPONENT_BODIES);
    for reached in ["Pet", "Owner", "Tag", "CreateOwnerBody"] {
        let body = struct_of(&types, reached);
        assert_eq!(field_type(body, "id"), OPTIONAL_ID, "{reached}:\n{body}");
    }
    let receipt = struct_of(&types, "Receipt");
    assert_eq!(field_type(receipt, "id"), "i64", "{receipt}");
}

/// The two operations of the compositions below: one takes `schema` as its
/// request body, and the other returns `response` as its response.
fn composed(request: &str, response: &str, schemas: &str) -> String {
    document(
        &format!(
            "  /pets:
    post:
      operationId: createPet
      requestBody:
        content:
          application/json:
            schema: {request}
      responses: {{ '201': {{ description: Created }} }}
  /pets/{{id}}:
    get:
      operationId: getPet
      parameters:
        - {{ name: id, in: path, required: true, schema: {{ type: integer }} }}
      responses:
        '200':
          description: OK
          content:
            application/json:
              schema: {response}
"
        ),
        schemas,
    )
}

/// The compositions of one schema that `required` is split over: the members that
/// list `id` and the ones that declare it read-only.
fn compositions() -> [(&'static str, &'static str); 4] {
    [
        (
            "an inline member that requires it beside an inline member that declares it",
            "    Pet:
      allOf:
        - { required: [id, name] }
        - type: object
          properties:
            id: { type: integer, readOnly: true }
            name: { type: string }
",
        ),
        (
            "the schema itself requires it, and a member declares it",
            "    Pet:
      required: [id, name]
      allOf:
        - type: object
          properties:
            id: { type: integer, readOnly: true }
            name: { type: string }
",
        ),
        (
            "an inline member that requires it beside a member that is a `$ref`",
            "    Pet:
      allOf:
        - { required: [id, name] }
        - { $ref: '#/components/schemas/Base' }
    Base:
      type: object
      properties:
        id: { type: integer, readOnly: true }
        name: { type: string }
",
        ),
        (
            "an inline member that requires it beside a wrapper of a `$ref` that declares it",
            "    Pet:
      allOf:
        - { required: [id, name] }
        - allOf:
            - { $ref: '#/components/schemas/Base' }
    Base:
      type: object
      properties:
        id: { type: integer, readOnly: true }
        name: { type: string }
",
        ),
    ]
}

/// A schema that is an `allOf` is one type, so a property that any member declares
/// read-only is not required of it, wherever a member lists it in `required`: in
/// the schema itself, in a member stated inline, and in a member that is a
/// `$ref`. A property that no member declares read-only stays required.
#[test]
fn a_required_in_one_member_does_not_keep_a_property_that_another_member_declares_read_only() {
    for (label, schemas) in compositions() {
        let doc = composed(
            "{ $ref: '#/components/schemas/Pet' }",
            "{ type: string }",
            schemas,
        );
        let types = types_of("readonly-allof-request", &doc);
        let pet = struct_of(&types, "Pet");
        assert!(optional(field_type(pet, "id")), "{label}:\n{pet}");
        assert_eq!(
            field_type(pet, "name"),
            "::std::string::String",
            "{label}:\n{pet}"
        );
    }
}

/// The same composition that only a response reaches is the document's: `required`
/// takes effect on a response, so `id` stays required there.
#[test]
fn the_composition_only_a_response_reaches_keeps_its_required() {
    let doc = composed(
        "{ type: object, properties: { name: { type: string } } }",
        "{ $ref: '#/components/schemas/Pet' }",
        "    Pet:
      allOf:
        - { required: [id, name] }
        - type: object
          properties:
            id: { type: integer, readOnly: true }
            name: { type: string }
",
    );
    let types = types_of("readonly-allof-response", &doc);
    let pet = struct_of(&types, "Pet");
    assert_eq!(field_type(pet, "id"), "i64", "{pet}");
}

/// A member that is a `$ref` is a named schema, and one type wherever it is used:
/// its own `required` stays. The composition that another member makes read-only
/// does not reach into it, so a request that names the member alone requires
/// what it declares without `readOnly`. `docs/generating.md` states this as a
/// limit.
#[test]
fn a_member_that_is_a_reference_keeps_its_own_required() {
    let doc = document(
        "  /pets:
    post:
      operationId: createPet
      requestBody:
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Pet' }
      responses: { '201': { description: Created } }
  /tags:
    post:
      operationId: createTag
      requestBody:
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Tag' }
      responses: { '201': { description: Created } }
",
        "    Pet:
      allOf:
        - { $ref: '#/components/schemas/Tag' }
        - type: object
          properties:
            id: { type: integer, readOnly: true }
    Tag:
      type: object
      required: [id]
      properties:
        id: { type: integer }
",
    );
    let types = types_of("readonly-allof-named", &doc);
    let tag = struct_of(&types, "Tag");
    assert_eq!(field_type(tag, "id"), "i64", "{tag}");
}
