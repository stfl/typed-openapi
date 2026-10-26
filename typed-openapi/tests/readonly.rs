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

/// The probe's finding: `--id` was demanded, so a caller had to invent an id that
/// the API says not to send.
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
/// a scalar.
#[test]
fn a_read_only_property_does_not_send_the_body_whole() {
    let properties = |meta: &str| {
        creating(
            "{ $ref: '#/components/schemas/Pet' }",
            &format!(
                "    Pet:\n      type: object\n      required: [name]\n      properties:\n        \
                 name: {{ type: string }}\n        meta: {meta}\n        \
                 audit: {{ type: array, items: {{ type: string }}, readOnly: true }}\n"
            ),
        )
    };
    let nested = "{ type: object, properties: { a: { type: string } } }";

    let doc = loaded(&properties(nested));
    let body = operation(&doc, "createPet").body();
    assert!(matches!(body, Body::JsonWhole { .. }), "{body:?}");
    // Whole, and `audit` is named all the same.
    assert_eq!(body.read_only(), &["audit"]);

    let doc = loaded(&properties(
        "{ type: object, readOnly: true, properties: { a: { type: string } } }",
    ));
    let body = operation(&doc, "createPet").body();
    assert!(matches!(body, Body::JsonFields { .. }), "{body:?}");
    assert_eq!(names(body.fields()), ["name"]);
    assert_eq!(body.read_only(), &["meta", "audit"]);
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
    // The body's own object is what the help can name.
    assert_eq!(read_only, &["id", "stamp"]);
    let (_, help) = command_of(op);
    assert!(
        help.contains("`id` is `readOnly`: the API sets it, so the body should go out without it."),
        "{help}"
    );
    assert!(
        help.contains(
            "`stamp` is `readOnly`: the API sets it, so the body should go out without it."
        ),
        "{help}"
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

/// The pattern of a read-only property is a rule of the document that a generated
/// type runs on a response, so it is still refused where no engine runs it. The
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

/// The probe's other half: typify required `id` of the type, so a body that
/// leaves it out could not be built.
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
    // The type of the request still stops requiring it.
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
