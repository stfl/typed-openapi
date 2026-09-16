//! What the runtime crate makes of the vendor's document plus the adopter's
//! Overlay — with no bless step anywhere, because a CLI-only adopter needs
//! none.
//!
//! The fixtures under `tests/fixtures/` are this crate's own, not the example
//! adoption's. `examples/toy/spec/` holds a document with the same content
//! today and a different owner: there it is the vendor's, and the example is
//! free to evolve it. Pointing these tests at that copy would let a change to
//! the example break the library.

#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use typed_openapi::model::Body;
use typed_openapi::tree::Asked;
use typed_openapi::{
    Carrier, Document, Effect, Field, Invocation, Operation, Param, Scalar, Shape, Unsupported,
    Values, render, tree,
};

const TOY: &str = include_str!("fixtures/toy.yaml");
const CORRECTIONS: &str = include_str!("fixtures/corrections.yaml");
const CLI: &str = include_str!("fixtures/cli.yaml");

/// The layers, in the order a bless step applies them.
const OVERLAYS: &[&str] = &[CORRECTIONS, CLI];

fn document() -> Document {
    Document::load(TOY, OVERLAYS).expect("the vendor's document plus the adopter's Overlay")
}

#[test]
fn the_overlay_adds_what_the_vendor_left_out() {
    let doc = document();
    assert!(doc.get("archiveVoucher").is_some(), "the added operation");
    let Body::JsonFields(fields) = doc.get("createVoucher").unwrap().body() else {
        panic!("createVoucher takes a flat JSON body");
    };
    assert!(
        fields.iter().any(|f| f.name() == "internal_ref"),
        "the added field"
    );
}

#[test]
fn the_gate_is_default_closed_and_the_marker_can_only_add_writes() {
    let doc = document();
    let effect = |id: &str| doc.get(id).unwrap_or_else(|| panic!("{id}")).effect();
    assert_eq!(effect("listVouchers"), Effect::Read);
    assert_eq!(effect("getVoucher"), Effect::Read);
    assert_eq!(effect("createVoucher"), Effect::Write);
    assert_eq!(effect("updateVoucher"), Effect::Write);
    assert_eq!(effect("enshrineVoucher"), Effect::Write);
    assert_eq!(effect("archiveVoucher"), Effect::Write);
    // The one fact HTTP cannot carry, and the only thing `x-cli-writes` is for.
    assert_eq!(effect("renderVoucher"), Effect::Write);
}

/// The second half of the gate: the words an operation is held behind beyond
/// the confirmation. They are decided while the document is reduced and read
/// back off both doors, because a shipped binary meets the blob and never the
/// document.
#[test]
fn the_gates_an_operation_names_come_back_off_the_document_and_the_blob() {
    let doc = document();
    let gates = |doc: &Document, id: &str| -> Vec<String> {
        doc.get(id)
            .unwrap_or_else(|| panic!("{id}"))
            .gates()
            .iter()
            .map(|gate| gate.as_str().to_owned())
            .collect()
    };

    assert_eq!(gates(&doc, "enshrineVoucher"), ["enshrine"]);
    assert_eq!(gates(&doc, "sendVoucherByEmail"), ["email"]);
    assert!(
        gates(&doc, "createVoucher").is_empty(),
        "a write the document names no hazard on stands behind --commit alone"
    );
    assert!(
        gates(&doc, "getVoucher").is_empty(),
        "a read is asked nothing"
    );

    let blob = doc.to_blob().expect("the reduction encodes");
    let shipped = Document::from_blob(&blob).expect("and decodes");
    assert_eq!(gates(&shipped, "enshrineVoucher"), ["enshrine"]);
    assert_eq!(gates(&shipped, "sendVoucherByEmail"), ["email"]);
}

/// Which operations stand behind a given word is the document's answer, so a
/// suite with something to say about everything irreversible asks it rather
/// than keeping a list beside it.
#[test]
fn the_document_names_its_gates_and_what_stands_behind_each() {
    let doc = document();
    let named: Vec<&str> = doc.gates().iter().map(|gate| gate.as_str()).collect();
    assert_eq!(named, ["enshrine", "email"]);

    let behind = |gate: &str| -> Vec<&str> { doc.gated_by(gate).map(Operation::id).collect() };
    assert_eq!(behind("enshrine"), ["enshrineVoucher"]);
    assert_eq!(behind("email"), ["sendVoucherByEmail"]);
    assert!(
        behind("commit").is_empty(),
        "the write gate is not one of the named ones"
    );
}

#[test]
fn every_body_is_exactly_one_flag_set() {
    let doc = document();
    let body = |id: &str| doc.get(id).unwrap_or_else(|| panic!("{id}")).body().clone();
    assert!(matches!(body("getVoucher"), Body::None));
    assert!(matches!(body("createVoucher"), Body::JsonFields(_)));
    // `Contact.address` is nested, so there are no per-field flags at all —
    // rather than dead ones beside a required `--json-body`.
    assert!(matches!(
        body("createContact"),
        Body::JsonWhole { required: true, .. }
    ));
    // A media type nothing here assembles: the bytes go through `--raw-body`
    // under the document's own `Content-Type`.
    assert!(matches!(
        body("uploadDocument"),
        Body::Opaque { ref media_type, .. } if media_type == "application/pdf"
    ));
    assert!(matches!(
        body("uploadDocumentMultipart"),
        Body::Multipart { .. }
    ));
}

/// A `required` list is what a body must carry *if the caller sends one*, and
/// whether the caller must send one at all is the other `required` — the
/// boolean on the request body. A property is demanded on the command line only
/// where both say so.
///
/// Reading the list alone would put the two statements at odds: the document
/// says the body may be left out, and the subcommand would then refuse to run
/// without a property of the body that was left out. `createMemo` beside it is
/// the control — the same list under a body the document does demand, where the
/// flag is demanded with it — so this says the reading is conditional rather
/// than merely lenient.
#[test]
fn a_property_is_demanded_only_where_the_document_demands_the_body_holding_it() {
    const EITHER_WAY: &str = r#"  /notes:
    post:
      operationId: createNote
      requestBody:
        required: false
        content:
          application/json:
            schema:
              type: object
              required: [label]
              properties:
                label: { type: string }
      responses: { "201": { description: Created } }
  /memos:
    post:
      operationId: createMemo
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [label]
              properties:
                label: { type: string }
      responses: { "201": { description: Created } }
"#;

    let doc = Document::load(&synthetic(EITHER_WAY), &[]).expect("a document");
    let demanded = |id: &str| {
        let Body::JsonFields(fields) = doc.get(id).expect(id).body() else {
            panic!("`{id}` states a flat body");
        };
        fields
            .iter()
            .find(|field| field.name() == "label")
            .expect("the body declares `label`")
            .required()
    };
    assert!(
        !demanded("createNote"),
        "a body the document does not ask for demands a property of itself"
    );
    assert!(demanded("createMemo"), "and one it does ask for does");

    // The same two answers where a user meets them: a subcommand whose body is
    // optional runs with nothing typed at all.
    let root = || clap::Command::new("toy").subcommands(tree::commands(&doc));
    root()
        .try_get_matches_from(["toy", "notes", "create"])
        .expect("an optional body is a body the caller may leave out");
    let refused = root()
        .try_get_matches_from(["toy", "memos", "create"])
        .expect_err("a required body has to arrive somehow")
        .to_string();
    assert!(refused.contains("--label"), "{refused}");
}

/// The properties of a body are reachable without matching on the body, and
/// every body that offers none says so the same way.
///
/// This is the door anything walking a body a value at a time goes through — a
/// guard, a renderer, a page of documentation — and the point of it being one
/// door is that a caller writes no match: the four bodies with nothing to offer
/// answer with nothing, so a walk is complete without knowing which of them it
/// has, and a body kind added later is covered where it stands.
#[test]
fn every_bodys_properties_are_reachable_without_matching_on_the_body() {
    let doc = document();
    let named = |id: &str| -> Vec<&str> {
        doc.get(id)
            .unwrap_or_else(|| panic!("{id}"))
            .body()
            .fields()
            .iter()
            .map(Field::name)
            .collect()
    };

    assert_eq!(
        named("createVoucher"),
        ["id", "total", "currency", "status", "internal_ref"],
        "a flat JSON body offers its properties in document order"
    );
    for (id, why) in [
        ("getVoucher", "the document asks for no body"),
        ("createContact", "one nested property sends the body whole"),
        ("uploadDocument", "the bytes go out as they arrived"),
        ("uploadDocumentMultipart", "the parts are files and text"),
    ] {
        assert!(named(id).is_empty(), "`{id}` offers properties, and {why}");
    }
}

/// Asking an operation which of its values are of a kind covers both halves of
/// the request, which is what the halves alone cannot promise.
///
/// The parts are public and a caller may take them: `Param::format` over
/// `Operation::params`, `Field::format` over `Body::fields`. What `carrying`
/// adds is that neither is forgotten — a guard over the parameters alone passes
/// on every body field it was written to cover, silently. So the whole is held
/// to the parts here: a `carrying` that stopped reading one half would still
/// answer, and only this says the answer is short.
#[test]
fn asking_for_a_kind_reads_both_halves_of_the_request() {
    let doc = postings();
    let op = doc.get("createPosting").expect("createPosting");

    let by_hand: Vec<&str> = op
        .params()
        .iter()
        .filter(|param| param.format() == Some("ledger-day"))
        .map(Param::name)
        .chain(
            op.body()
                .fields()
                .iter()
                .filter(|field| field.format() == Some("ledger-day"))
                .map(Field::name),
        )
        .collect();
    let asked: Vec<&str> = op.carrying("ledger-day").map(Carrier::name).collect();

    assert_eq!(asked, by_hand, "the whole is not the two halves");
    assert!(
        asked.contains(&"period") && asked.contains(&"booked_on"),
        "the fixture no longer names a kind in both halves, so this test \
         compares nothing: {asked:?}"
    );
}

#[test]
fn a_body_field_moves_aside_for_a_path_parameter_of_the_same_name() {
    let doc = document();
    let update = doc.get("updateVoucher").unwrap();
    assert_eq!(flag_of(update.param("id").unwrap()), "id");
    let Body::JsonFields(fields) = update.body() else {
        panic!("updateVoucher takes a flat JSON body");
    };
    let id = fields.iter().find(|f| f.name() == "id").unwrap();
    assert_eq!(id.flag(), "body-id");
    assert!(id.renamed(), "and it says so in its help line");
}

/// A flag word this CLI spends is spent before the document has a say, so a
/// vendor who declares a field spelled the same way gets a flag of their own
/// rather than the one that stands in front of the CLI's own question.
///
/// The word costs something to reserve — this is that cost, paid at bless time
/// where a reviewer sees the rename, rather than at run time where a user would
/// find `--json-body-template` sending a field.
#[test]
fn a_body_field_spelled_like_the_template_flag_moves_aside() {
    let document = pointing(
        "\x20               json-body-template:\n\
         \x20                 type: string\n",
    );
    let doc = Document::load(&document, &[]).expect("a document");
    let Body::JsonFields(fields) = doc.get("createNote").expect("createNote").body() else {
        panic!("a body of scalars is flat");
    };
    let field = fields
        .iter()
        .find(|f| f.name() == "json-body-template")
        .expect("the document declares it");
    assert_eq!(field.flag(), "body-json-body-template");
    assert!(field.renamed(), "and it says so in its help line");
}

/// The body with no per-field flags is the body nothing on the command line
/// describes, so the reduction writes down what it saw on the way to deciding
/// that: the keys a caller must supply, nested as deep as the document nests
/// them.
///
/// Every rule the template follows is in this one rendering. `name` and
/// `address` are required and are here; `Contact.id` is optional and is not.
/// `address` nests, because the document nests it. `street` is the empty string
/// its type skeletons to, and `city` is the word the document states an
/// `example` for — a value that round-trips, where one built from the type
/// alone is only a shape.
#[test]
fn a_nested_body_carries_the_shape_no_flag_can_state() {
    let doc = document();
    let Body::JsonWhole {
        template: Some(template),
        ..
    } = doc.get("createContact").expect("createContact").body()
    else {
        panic!("a nested body goes whole and carries a template");
    };
    assert_eq!(
        template,
        "{\n  \"name\": \"\",\n  \"address\": {\n    \"street\": \"\",\n    \"city\": \"Vienna\"\n  }\n}"
    );
}

/// The template one document's `createNote` body renders to.
///
/// `body` is the request body's schema, indented to sit under `schema:`, and
/// `schemas` is whatever `components.schemas` it points into.
fn templated(body: &str, schemas: &str) -> Option<String> {
    let components = if schemas.is_empty() {
        String::new()
    } else {
        format!("components:\n\x20 schemas:\n{schemas}")
    };
    let document = format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         paths:\n\
         \x20 /notes:\n\
         \x20   post:\n\
         \x20     operationId: createNote\n\
         \x20     requestBody:\n\
         \x20       required: true\n\
         \x20       content:\n\
         \x20         application/json:\n\
         {body}\
         \x20     responses: {{ \"201\": {{ description: Created }} }}\n\
         {components}"
    );
    let doc = Document::load(&document, &[]).expect("a document");
    let Body::JsonWhole { template, .. } = doc.get("createNote").expect("createNote").body() else {
        panic!("a body with per-field flags is not the case a template is for");
    };
    template.clone()
}

/// A body of one nested object, whose properties are handed in.
fn nested(properties: &str) -> String {
    format!(
        "\x20           schema:\n\
         \x20             type: object\n\
         \x20             required: [inner]\n\
         \x20             properties:\n\
         \x20               inner:\n\
         \x20                 type: object\n\
         {properties}"
    )
}

/// A template is a skeleton, so it carries values a server refuses: `""` where
/// the document states a `pattern`, `0` where it states a `minimum`, `false`
/// where it asks for a decision. A template a user could send unmodified by
/// accident would be a worse artefact than none — and there is no empty member
/// of an enumeration, so that one kind shows the first value the document
/// lists rather than a value the document does not have.
#[test]
fn a_template_is_empty_where_a_value_can_be_and_names_an_enums_own_value() {
    let template = templated(
        &nested(
            "\x20                 required: [ref, count, rate, paid, status]\n\
             \x20                 properties:\n\
             \x20                   ref: { type: string, pattern: '^[A-Z]{3}$' }\n\
             \x20                   count: { type: integer, minimum: 10 }\n\
             \x20                   rate: { type: number }\n\
             \x20                   paid: { type: boolean }\n\
             \x20                   status: { type: string, enum: [draft, open, paid] }\n",
        ),
        "",
    )
    .expect("a nested body renders a template");

    assert_eq!(
        template,
        "{\n  \"inner\": {\n    \"ref\": \"\",\n    \"count\": 0,\n    \"rate\": 0.0,\n    \
         \"paid\": false,\n    \"status\": \"draft\"\n  }\n}"
    );
}

/// The keys a caller must supply, and no others. An optional key carrying an
/// empty value would be a key nobody asked to send — on a `PUT`, an empty
/// string written over a field somebody meant to leave alone — and JSON has no
/// comment to mark it as a suggestion with.
#[test]
fn only_the_properties_the_document_requires_reach_a_template() {
    let template = templated(
        &nested(
            "\x20                 required: [kept]\n\
             \x20                 properties:\n\
             \x20                   kept: { type: string }\n\
             \x20                   dropped: { type: string }\n",
        ),
        "",
    )
    .expect("a nested body renders a template");

    assert!(template.contains("\"kept\""), "{template}");
    assert!(
        !template.contains("\"dropped\""),
        "an optional key is not a key the caller asked for: {template}"
    );
}

/// The document's own `example` is the better source, so it wins over the
/// skeleton a type alone would give — a value the document states round-trips.
///
/// And it is read as *data*: a vendor whose example spells out a form using the
/// words `required` and `properties` is writing a value that happens to use
/// them, so the walk takes it whole and never descends into it looking for a
/// schema.
#[test]
fn an_example_the_document_states_wins_and_is_read_as_a_value() {
    let template = templated(
        &nested(
            "\x20                 required: [amount]\n\
             \x20                 properties:\n\
             \x20                   amount: { type: string }\n\
             \x20                 example:\n\
             \x20                   required: [not a key]\n\
             \x20                   properties: not a schema\n",
        ),
        "",
    )
    .expect("a nested body renders a template");

    assert_eq!(
        template,
        "{\n  \"inner\": {\n    \"required\": [\n      \"not a key\"\n    ],\n    \
         \"properties\": \"not a schema\"\n  }\n}"
    );
}

/// A property pointing at a named schema and stating an `example` of its own
/// describes one value twice, and the property's is what the template shows.
///
/// The reading a description gets, for the reason a description gets it: both
/// are written *about* something, and what a property writes is about that
/// property, where the named schema's is about every field sharing the rule. A
/// `City` whose example is `Vienna` says what a city looks like; a `city`
/// property that says `Graz` is telling its own caller something else, and a
/// template showing `Vienna` there shows a value nobody wrote about this key.
///
/// `region` beside it is the control: a bare `$ref` states nothing of its own,
/// so what it inherits is all there is and both readings reach it.
#[test]
fn a_propertys_own_example_wins_over_the_one_it_points_at() {
    let template = templated(
        &nested(
            "\x20                 required: [city, region]\n\
             \x20                 properties:\n\
             \x20                   city:\n\
             \x20                     allOf: [{ $ref: '#/components/schemas/City' }]\n\
             \x20                     example: Graz\n\
             \x20                   region: { $ref: '#/components/schemas/City' }\n",
        ),
        "\x20   City:\n\
         \x20     type: string\n\
         \x20     example: Vienna\n",
    )
    .expect("a nested body renders a template");

    assert_eq!(
        template,
        "{\n  \"inner\": {\n    \"city\": \"Graz\",\n    \"region\": \"Vienna\"\n  }\n}"
    );
}

/// One element rather than none. An empty list is a body a server accepts and
/// a user learns nothing from, and what goes *in* the list is what they came
/// here to find out.
#[test]
fn an_array_shows_one_element_rather_than_none() {
    let template = templated(
        &nested(
            "\x20                 required: [lines]\n\
             \x20                 properties:\n\
             \x20                   lines:\n\
             \x20                     type: array\n\
             \x20                     items:\n\
             \x20                       type: object\n\
             \x20                       required: [account]\n\
             \x20                       properties:\n\
             \x20                         account: { type: string }\n",
        ),
        "",
    )
    .expect("a nested body renders a template");

    assert_eq!(
        template,
        "{\n  \"inner\": {\n    \"lines\": [\n      {\n        \"account\": \"\"\n      }\n    \
         ]\n  }\n}"
    );
}

/// A property that points back at the schema holding it describes a value of no
/// finite depth, so the walk stops at a floor and writes the empty object
/// there. What a user gets is the shape down to that depth rather than a
/// template that never ends — and, more to the point, rather than a bless step
/// that never returns.
#[test]
fn a_schema_that_points_back_at_itself_still_renders_a_finite_template() {
    let template = templated(
        "\x20           schema: { $ref: '#/components/schemas/Node' }\n",
        "\x20   Node:\n\
         \x20     type: object\n\
         \x20     required: [label, child]\n\
         \x20     properties:\n\
         \x20       label: { type: string }\n\
         \x20       child: { $ref: '#/components/schemas/Node' }\n",
    )
    .expect("a cycle still renders");

    assert_eq!(
        template.matches("\"child\"").count(),
        8,
        "the walk stops at its own floor: {template}"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(&template).is_ok(),
        "and what it stops with is JSON: {template}"
    );
}

/// A flat body carries no template, because its per-field flags already say
/// what goes in it — one per property, each with the rules its schema states. A
/// second rendering of the same facts in a second notation would be the one
/// place the two could come to disagree.
#[test]
fn a_flat_body_has_its_flags_to_say_what_it_wants_and_carries_no_template() {
    let doc = document();
    assert!(matches!(
        doc.get("createVoucher").expect("createVoucher").body(),
        Body::JsonFields(_)
    ));
}

/// A body this crate has no reading for describes no shape, so there is nothing
/// to write down and the absence travels — which is what keeps a subcommand from
/// growing a flag that prints nothing, the flag progenitor ships.
///
/// Two ways to say nothing: a body with no schema at all, and a composition,
/// where the document says a value is one of several things and nothing here
/// picks which. A skeleton for either would be a shape this crate invented.
#[test]
fn a_body_the_document_describes_no_shape_for_carries_no_template() {
    assert_eq!(templated("\x20           {}\n", ""), None);
    assert_eq!(templated(&composed(), ""), None);
}

/// A body whose schema is a `oneOf` of two objects: a real composition, and one
/// with no single shape to write down.
fn composed() -> String {
    "\x20           schema:\n\
     \x20             oneOf:\n\
     \x20               - { type: object, required: [ledger], properties: { ledger: { type: string } } }\n\
     \x20               - { type: object, required: [period], properties: { period: { type: string } } }\n"
        .to_owned()
}

/// A template is decided while the document is reduced and travels in the blob,
/// like both command names: a shipped binary prints it and has no schema walk
/// compiled into it to have derived it with.
///
/// The bytes are the reason it is text rather than a `serde_json::Value`.
/// postcard is not self-describing and a `Value` deserialises through
/// `deserialize_any`, which postcard answers with `WontImplement` — a `Value`
/// in the blob would not come back at all.
#[test]
fn a_template_comes_back_off_the_blob_the_bless_step_writes() {
    let doc = document();
    let blob = doc.to_blob().expect("the reduction encodes");
    let read = Document::from_blob(&blob).expect("and decodes");

    let template = |doc: &Document| {
        let Body::JsonWhole { template, .. } =
            doc.get("createContact").expect("createContact").body()
        else {
            panic!("a nested body goes whole");
        };
        template.clone()
    };
    let carried = template(&read);
    assert!(carried.is_some(), "the blob carries the template");
    assert_eq!(carried, template(&doc));
}

/// The closest thing a runtime-built tree has to a compile-time check, and the
/// one that catches the next flag collision before a user does.
#[test]
fn the_whole_mounted_tree_is_a_valid_clap_command() {
    let doc = document();
    clap::Command::new("toy")
        .subcommand(clap::Command::new("raw").subcommands(tree::commands(&doc)))
        .debug_assert();
}

#[test]
fn a_dry_run_prints_the_request_that_commit_would_send() {
    let doc = document();
    let op = doc.get("updateVoucher").unwrap();
    let values = Values::new()
        .param("id", 5)
        .json(serde_json::json!({"total": "12.50"}));
    let request = Invocation::new(op, values)
        .expect("the values satisfy the operation")
        .request(doc.base())
        .expect("the base URL is a URL");
    assert_eq!(
        render(&request),
        "PUT /vouchers/5 HTTP/1.1\n\
         host: localhost:9999\n\
         content-type: application/json\n\
         \n\
         {\"total\":\"12.50\"}\n"
    );
}

#[test]
fn the_document_rejects_values_it_does_not_describe() {
    let doc = document();
    let op = doc.get("getVoucher").unwrap();
    assert!(
        Invocation::new(op, Values::new()).is_err(),
        "`id` is required"
    );
    assert!(
        Invocation::new(op, Values::new().param("nope", 1)).is_err(),
        "there is no `nope` parameter"
    );
    assert!(
        Invocation::new(op, Values::new().param("id", "five")).is_err(),
        "`id` is an integer"
    );
    assert!(
        Invocation::new(op, Values::new().param("id", 5).json(serde_json::json!({}))).is_err(),
        "getVoucher takes no body"
    );
}

/// Two defences, in this order: the document's own type rejects the value, and
/// anything that does get through is percent-encoded rather than interpolated.
#[test]
fn a_path_value_cannot_smuggle_a_segment_into_the_url() {
    let doc = document();
    let op = doc.get("archiveVoucher").unwrap();
    let refused = Invocation::new(op, Values::new().param("id", "1/../../etc"))
        .expect_err("`id` is `type: integer` in the document");
    assert!(
        refused.to_string().contains("is not an integer"),
        "{refused}"
    );

    // The same value under a parameter the document types as a string.
    let doc = Document::load(
        &TOY.replace(
            "        schema:\n          type: integer\n          format: int64",
            "        schema:\n          type: string",
        ),
        OVERLAYS,
    )
    .expect("a document whose ids are strings");
    let op = doc.get("getVoucher").unwrap();
    let request = Invocation::new(op, Values::new().param("id", "1/../../etc"))
        .unwrap()
        .request(doc.base())
        .unwrap();
    assert_eq!(request.uri().path(), "/vouchers/1%2F..%2F..%2Fetc");
}

/// A property pointed at a named schema carries that schema's rules onto the
/// flag. Only following the `$ref` while the document is reduced can put them
/// there: the rule is a hop away from the property that has to obey it.
#[test]
fn a_rule_a_named_schema_states_reaches_the_property_pointing_at_it() {
    let doc = document();
    let Body::JsonFields(fields) = doc.get("updateVoucher").unwrap().body() else {
        panic!("updateVoucher takes a flat JSON body");
    };
    let total = fields.iter().find(|f| f.name() == "total").unwrap();
    assert_eq!(
        total.scalar().note().as_deref(),
        Some(r"matches ^-?[0-9]+(\.[0-9]{1,2})?$"),
        "the rule the `Money` schema states did not reach `total`"
    );
    assert_eq!(
        total
            .scalar()
            .parse("1,50")
            .expect_err("a comma is not a decimal point")
            .to_string(),
        r"`1,50` does not match ^-?[0-9]+(\.[0-9]{1,2})?$"
    );
    assert!(total.scalar().parse("12.50").is_ok());
}

/// A parameter never passes through a generated body type, so what the document
/// says about one is the only thing that can ever check it. All of it is
/// checked, and every refusal carries the document's own number.
#[test]
fn every_rule_a_parameter_states_is_checked_because_nothing_else_can_check_it() {
    const PARAMETERS: &str = "  /vouchers:\n\
         \x20   get:\n\
         \x20     operationId: listVouchers\n\
         \x20     parameters:\n\
         \x20       - { name: since, in: query, schema: { type: string, pattern: '^[0-9]{4}-[0-9]{2}$' } }\n\
         \x20       - { name: code, in: query, schema: { type: string, minLength: 3, maxLength: 3 } }\n\
         \x20       - { name: limit, in: query, schema: { type: integer, minimum: 1, maximum: 100, multipleOf: 5 } }\n\
         \x20     responses: { \"200\": { description: OK } }\n";

    let doc = Document::load(&synthetic(PARAMETERS), &[]).expect("a document");
    let op = doc.get("listVouchers").unwrap();
    let refused = |name: &str, value: &str| {
        Invocation::new(op, Values::new().param(name, value))
            .expect_err("the document refuses it")
            .to_string()
    };

    assert_eq!(
        refused("since", "2026-9"),
        "listVouchers: `since`: `2026-9` does not match ^[0-9]{4}-[0-9]{2}$"
    );
    assert_eq!(
        refused("code", "EU"),
        "listVouchers: `code`: `EU` is shorter than 3 characters"
    );
    assert_eq!(
        refused("code", "EURO"),
        "listVouchers: `code`: `EURO` is longer than 3 characters"
    );
    assert_eq!(
        refused("limit", "0"),
        "listVouchers: `limit`: `0` is not at least 1"
    );
    assert_eq!(
        refused("limit", "105"),
        "listVouchers: `limit`: `105` is not at most 100"
    );
    assert_eq!(
        refused("limit", "7"),
        "listVouchers: `limit`: `7` is not a multiple of 5"
    );
    assert!(
        Invocation::new(
            op,
            Values::new()
                .param("since", "2026-09")
                .param("code", "EUR")
                .param("limit", "25"),
        )
        .is_ok()
    );
}

/// A `pattern` the engine cannot read would refuse every value at the flag,
/// which is a command line nothing can satisfy. The document is refused while
/// it is reduced instead, naming the operation and the value it was stated
/// about.
#[test]
fn a_pattern_no_engine_can_read_is_refused_while_the_document_is_reduced() {
    let error = Document::load(
        &synthetic(
            "  /vouchers:\n\
             \x20   get:\n\
             \x20     operationId: listVouchers\n\
             \x20     parameters:\n\
             \x20       - { name: since, in: query, schema: { type: string, pattern: '[unterminated' } }\n\
             \x20     responses: { \"200\": { description: OK } }\n",
        ),
        &[],
    )
    .expect_err("`[unterminated` is not a regular expression");
    assert_eq!(
        error.to_string(),
        "listVouchers: `since`: `[unterminated` is not a regular expression: Unbalanced bracket"
    );
}

/// One operation with a body, under whatever `content` key is handed in.
fn upload(content_key: &str) -> String {
    synthetic(&format!(
        "  /documents:\n\
         \x20   post:\n\
         \x20     operationId: uploadDocument\n\
         \x20     requestBody:\n\
         \x20       required: true\n\
         \x20       content:\n\
         \x20         '{content_key}':\n\
         \x20           schema: {{ type: string, format: binary }}\n\
         \x20     responses: {{ \"201\": {{ description: Created }} }}\n"
    ))
}

/// A `content` key with no `/` in it names no media type, so there is nothing
/// to send the body under: carrying it would put a `Content-Type` on the wire
/// that no server parses. Which type the vendor meant is a judgement, and the
/// refusal is what leaves that judgement to an adopter writing an Overlay a
/// reviewer can read.
#[test]
fn a_content_key_that_is_not_a_media_type_is_refused_while_the_document_is_reduced() {
    // The way out the message names: `remove` takes the key out, `update` puts
    // the one the vendor meant in its place.
    const REPAIR: &str = "overlay: 1.1.0\n\
         info: { title: t, version: \"1\" }\n\
         actions:\n\
         \x20 - target: \"$.paths['/documents'].post.requestBody.content['form-data']\"\n\
         \x20   description: The vendor means `multipart/form-data`.\n\
         \x20   remove: true\n\
         \x20 - target: $.paths['/documents'].post.requestBody.content\n\
         \x20   description: Say it the way the wire spells it.\n\
         \x20   update:\n\
         \x20     multipart/form-data:\n\
         \x20       schema: { type: object, properties: { file: { type: string } } }\n";

    let error =
        Document::load(&upload("form-data"), &[]).expect_err("`form-data` names no media type");
    assert_eq!(
        error.to_string(),
        "uploadDocument: `form-data` is not a media type; \
         an Overlay is where a document's content type is corrected"
    );

    let repaired = Document::load(&upload("form-data"), &[REPAIR]).expect("the Overlay repairs it");
    assert!(matches!(
        repaired
            .get("uploadDocument")
            .expect("the operation")
            .body(),
        Body::Multipart { .. }
    ));
}

/// What `Opaque` is for, and what the refusal above must not swallow: a media
/// type this crate cannot assemble is still a media type, so the body goes
/// through `--raw-body` under the document's own spelling of it.
#[test]
fn a_media_type_this_crate_cannot_assemble_is_carried_rather_than_refused() {
    let body = |key: &str| {
        Document::load(&upload(key), &[])
            .unwrap_or_else(|error| panic!("{key}: {error}"))
            .get("uploadDocument")
            .expect("the operation")
            .body()
            .clone()
    };
    assert!(matches!(
        body("application/pdf"),
        Body::Opaque { ref media_type, .. } if media_type == "application/pdf"
    ));
    // The parameters travel too: they are part of the `Content-Type` the
    // document asks for.
    assert!(matches!(
        body("text/csv; charset=utf-8"),
        Body::Opaque { ref media_type, .. } if media_type == "text/csv; charset=utf-8"
    ));
    assert!(matches!(
        body("application/x-www-form-urlencoded"),
        Body::Opaque { .. }
    ));
}

/// One JSON body whose properties are handed in, over a named schema stating a
/// rule: the route `docs/overlay.md` recommends, written out for both spellings
/// OpenAPI 3.0 offers. Every property line is indented to sit under
/// `properties:`.
fn pointing(properties: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         paths:\n\
         \x20 /notes:\n\
         \x20   post:\n\
         \x20     operationId: createNote\n\
         \x20     requestBody:\n\
         \x20       required: true\n\
         \x20       content:\n\
         \x20         application/json:\n\
         \x20           schema:\n\
         \x20             type: object\n\
         \x20             properties:\n\
         {properties}\
         \x20     responses: {{ \"201\": {{ description: Created }} }}\n\
         components:\n\
         \x20 schemas:\n\
         \x20   Day:\n\
         \x20     type: string\n\
         \x20     description: A calendar day.\n\
         \x20     pattern: '^[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}$'\n"
    )
}

/// A `$ref` erases everything written beside it, so the only way a 3.0 document
/// names a rule *and* keeps a sentence about the field pointing at it is to
/// wrap the reference in an `allOf` of one element. The wrapper composes
/// nothing, so it is read as the element it wraps: the rule reaches the flag,
/// and the field keeps its own words.
#[test]
fn a_single_element_all_of_is_read_as_the_schema_it_wraps() {
    let doc = Document::load(
        &pointing(
            "\x20               booked:\n\
             \x20                 allOf: [{ $ref: '#/components/schemas/Day' }]\n\
             \x20                 description: The day this note is booked under.\n\
             \x20               due:\n\
             \x20                 allOf: [{ $ref: '#/components/schemas/Day' }]\n",
        ),
        &[],
    )
    .expect("a document");
    let Body::JsonFields(fields) = doc.get("createNote").unwrap().body() else {
        panic!("both properties are scalars, so both are flags");
    };
    let field = |name: &str| {
        fields
            .iter()
            .find(|f| f.name() == name)
            .unwrap_or_else(|| panic!("{name}"))
    };

    // The rule the named schema states arrives through the wrapper.
    assert_eq!(
        field("booked").scalar().note().as_deref(),
        Some(r"matches ^[0-9]{4}-[0-9]{2}-[0-9]{2}$")
    );
    assert!(field("booked").scalar().parse("2026-09-14").is_ok());
    assert!(field("booked").scalar().parse("14.09.2026").is_err());

    // The sentence about this field, which a bare `$ref` would have erased.
    assert_eq!(
        field("booked").description(),
        Some("The day this note is booked under.")
    );
    // And the named schema's, for the field that says nothing of its own.
    assert_eq!(field("due").description(), Some("A calendar day."));
}

/// An `allOf` that is more than a wrapper composes schemas, and a composition
/// is not one value: the whole body goes through `--json-body` rather than a
/// flag standing for something the request builder would have to invent. Two
/// schemas is one way to be more than a wrapper; a keyword of the node's own
/// beside the `allOf` is the other.
#[test]
fn an_all_of_that_is_more_than_a_wrapper_is_not_a_scalar() {
    let whole = |properties: &str| {
        let doc = Document::load(&pointing(properties), &[]).expect("a document");
        matches!(
            doc.get("createNote").expect("createNote").body(),
            Body::JsonWhole { required: true, .. }
        )
    };
    assert!(whole(
        "\x20               booked:\n\
         \x20                 allOf:\n\
         \x20                   - { $ref: '#/components/schemas/Day' }\n\
         \x20                   - { type: string, minLength: 1 }\n"
    ));
    assert!(whole(
        "\x20               booked:\n\
         \x20                 type: string\n\
         \x20                 allOf: [{ $ref: '#/components/schemas/Day' }]\n"
    ));
}

/// One operation over a named schema that states both halves of a rule: the
/// `pattern` every consumer of the document can run, and the `format` naming
/// the part no document can state — what a day *is* to a ledger that closes
/// periods. Five values are of that kind, each arriving at it by a different
/// road, and three are not.
const POSTINGS: &str = r"openapi: 3.0.3
info: { title: t, version: '1' }
servers: [{ url: 'http://localhost:9999' }]
paths:
  /postings:
    post:
      operationId: createPosting
      parameters:
        - name: period
          in: query
          schema: { $ref: '#/components/schemas/LedgerDay' }
        - name: opened
          in: query
          schema: { type: array, items: { $ref: '#/components/schemas/LedgerDay' } }
        - name: session
          in: cookie
          schema: { $ref: '#/components/schemas/LedgerDay' }
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                booked_on: { $ref: '#/components/schemas/LedgerDay' }
                paid_on:
                  allOf: [{ $ref: '#/components/schemas/LedgerDay' }]
                  description: The day the money arrived.
                due_on:
                  type: string
                  format: ledger-day
                  pattern: '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
                amount: { type: integer, format: int64 }
                memo: { type: string }
      responses: { '201': { description: Created } }
  /positions:
    post:
      operationId: createPosition
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                booked_on: { $ref: '#/components/schemas/LedgerDay' }
                line:
                  type: object
                  properties:
                    account: { type: string }
      responses: { '201': { description: Created } }
components:
  schemas:
    LedgerDay:
      type: string
      format: ledger-day
      pattern: '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      description: A day in the ledger.
";

fn postings() -> Document {
    Document::load(POSTINGS, &[]).expect("a document naming the kind of its days")
}

/// Every value of one operation that is of a named kind, as the half of the
/// request it travels in and the name the document gives it.
fn carried(op: &Operation, format: &str) -> Vec<String> {
    op.carrying(format)
        .map(|carrier| match carrier {
            Carrier::Param(_) => format!("param {}", carrier.name()),
            Carrier::Field(_) => format!("field {}", carrier.name()),
        })
        .collect()
}

/// The question a guard asks: which of the values this operation sends are of
/// a kind I have something to say about. A `pattern` cannot state a calendar,
/// so the document names the kind and the adopter supplies the meaning — and
/// what makes that general rather than a list of field names kept by hand is
/// that both halves of the operation answer, whichever road the kind arrived
/// by: a `$ref` at the named schema, a wrapper around that reference, the items
/// of a list, or the keyword written on the property itself.
#[test]
fn an_operation_names_the_values_of_a_kind_the_document_declares() {
    let doc = postings();
    let op = doc.get("createPosting").expect("createPosting");

    assert_eq!(
        carried(op, "ledger-day"),
        [
            "param period",
            "param opened",
            "field booked_on",
            "field paid_on",
            "field due_on",
        ]
    );
    // A format OpenAPI names itself is a kind like any other, and comes back
    // in the document's own spelling rather than in this crate's.
    assert_eq!(carried(op, "int64"), ["field amount"]);
    // A kind nothing declares names nothing — not everything that declares no
    // kind at all.
    assert!(
        carried(op, "money").is_empty(),
        "no schema here says an amount is anything"
    );
    assert!(
        carried(op, "").is_empty(),
        "a document that declares no kind declares no kind"
    );
}

/// A sentence is about the field that carries a value; a kind is about the
/// value itself. So the two follow opposite preferences, and `paid_on` carries
/// both: its own words about why this day matters here, over a reference whose
/// schema says what kind of thing every field pointing at it holds. Reading
/// the kind off the schema that states it is also what leaves an adoption one
/// vocabulary — it is the node a generator hands typify, so the Rust type the
/// kind stands for and the kind reported here come off the same words.
#[test]
fn the_sentence_is_the_fields_own_and_the_kind_is_the_named_schemas() {
    let doc = postings();
    let Body::JsonFields(fields) = doc.get("createPosting").unwrap().body() else {
        panic!("every property of the body is a scalar");
    };
    let field = |name: &str| {
        fields
            .iter()
            .find(|field| field.name() == name)
            .unwrap_or_else(|| panic!("{name}"))
    };

    assert_eq!(
        field("paid_on").description(),
        Some("The day the money arrived.")
    );
    assert_eq!(field("paid_on").format(), Some("ledger-day"));
    assert_eq!(
        field("booked_on").description(),
        Some("A day in the ledger.")
    );
    assert_eq!(field("booked_on").format(), Some("ledger-day"));
    assert_eq!(field("memo").format(), None);
}

/// A kind is about a value, so a shape that carries no value names none. Both
/// of these are worth knowing before a guard is written over what the answer
/// names: a parameter this CLI cannot spell gets no flag and no place in the
/// request, and a body with one nested property goes out whole and has no
/// fields at all — so the kinds its properties declare are not reachable, and
/// a guard over such a body is a guard the adopter writes over the JSON.
#[test]
fn a_shape_that_carries_no_value_names_no_kind() {
    let doc = postings();
    let op = doc.get("createPosting").expect("createPosting");
    let session = op.param("session").expect("it is in the reduction");
    assert!(matches!(
        session.shape(),
        Shape::Unreachable(Unsupported::Cookie)
    ));
    assert_eq!(
        session.format(),
        None,
        "the document names the kind; the parameter carries no value"
    );

    let nested = doc.get("createPosition").expect("createPosition");
    assert!(matches!(nested.body(), Body::JsonWhole { .. }));
    assert!(
        carried(nested, "ledger-day").is_empty(),
        "a body that goes out whole has no fields to name"
    );
}

/// One parameter under a rule, with `kind` standing where a `format` would go:
/// the same document, reduced with and without a kind named beside the rule.
fn period(kind: &str) -> Document {
    let document = synthetic(&format!(
        "  /postings:\n\
         \x20   get:\n\
         \x20     operationId: listPostings\n\
         \x20     parameters:\n\
         \x20       - name: period\n\
         \x20         in: query\n\
         \x20         schema: {{ type: string, {kind}pattern: '^[0-9-]+$', minLength: 7 }}\n\
         \x20     responses: {{ \"200\": {{ description: OK }} }}\n"
    ));
    Document::load(&document, &[]).expect("a document")
}

/// A kind names a rule and is not one, so naming it changes nothing a value is
/// held to. The same value gets the same verdict and the same sentence with
/// the kind and without it — which is what keeps every rule in one place, and
/// what a kind that reached `Scalar` would break on its first value.
#[test]
fn a_kind_named_beside_a_rule_changes_no_value_the_parser_admits_or_refuses() {
    let plain = period("");
    let tagged = period("format: ledger-period, ");
    let param = |doc: &Document| {
        doc.get("listPostings")
            .expect("listPostings")
            .param("period")
            .expect("period")
            .clone()
    };
    let (plain, tagged) = (param(&plain), param(&tagged));
    assert_eq!(tagged.format(), Some("ledger-period"));
    assert_eq!(plain.format(), None);

    let (plain, tagged) = (scalar_of(&plain), scalar_of(&tagged));
    // The rules are the same rules, down to the value.
    assert_eq!(plain, tagged);
    assert_eq!(plain.note(), tagged.note());
    for raw in ["2026-09", "2026-09-14", "2026", "", "x", "not-a-day"] {
        assert_eq!(plain.parse(raw), tagged.parse(raw), "{raw}");
    }
}

/// The help line carries rules: every note beside a flag is something a value
/// can be refused for, and the refusal is the same rendering. A kind is
/// nothing a value can be refused for, so a kind on that line would be a
/// promise this crate leaves the server to keep.
#[test]
fn a_kind_reaches_no_flags_help_line() {
    let doc = postings();
    let op = doc.get("createPosting").expect("createPosting");
    let rendered = tree::command(op).render_long_help().to_string();
    assert!(
        rendered.contains("matches ^[0-9]"),
        "the rule is on the line: {rendered}"
    );
    for kind in ["ledger-day", "int64"] {
        assert!(
            !rendered.contains(kind),
            "`{kind}` is on a help line:\n{rendered}"
        );
    }
}

/// A kind travels in the blob, because the binary that has something to say
/// about a kind is the binary that never reads a document.
#[test]
fn a_kind_survives_the_reduction_the_bless_step_writes() {
    let doc = postings();
    let blob = doc.to_blob().expect("the reduction encodes");
    let shipped = Document::from_blob(&blob).expect("and decodes");
    let named: Vec<&str> = shipped
        .get("createPosting")
        .expect("createPosting")
        .carrying("ledger-day")
        .map(Carrier::name)
        .collect();
    assert_eq!(
        named,
        ["period", "opened", "booked_on", "paid_on", "due_on"]
    );
}

/// One query parameter that is a list of strings. `explode` is the one line
/// that decides how its values reach the wire.
const LISTED: &str = r"  /vouchers:
    get:
      operationId: listVouchers
      parameters:
        - name: tag
          in: query
          explode: true
          schema: { type: array, items: { type: string } }
      responses: { '200': { description: OK } }
";

/// One operation declaring three shapes at once — a list this CLI spells, an
/// object it cannot, and a plain integer — beside a second operation that
/// declares none of them.
const SHAPES: &str = r"  /vouchers:
    get:
      operationId: listVouchers
      parameters:
        - name: tag
          in: query
          schema: { type: array, items: { type: string } }
        - name: filter
          in: query
          required: false
          schema:
            type: object
            properties:
              opened:
                type: object
                properties:
                  from: { type: string }
        - name: limit
          in: query
          schema: { type: integer }
      responses: { '200': { description: OK } }
  /contacts:
    post:
      operationId: createContact
      responses: { '201': { description: OK } }
";

/// The request one document builds from one set of values, as a whole URL.
fn sent(document: &str, id: &str, values: Values) -> String {
    let doc = Document::load(document, &[]).expect("a document");
    let op = doc.get(id).unwrap_or_else(|| panic!("{id}"));
    Invocation::new(op, values)
        .expect("the values satisfy the operation")
        .request(doc.base())
        .expect("the base URL is a URL")
        .uri()
        .to_string()
}

/// A list is one flag given more than once, and what becomes of the repeats is
/// the document's `explode` rather than this crate's preference. `form` with
/// `explode: true` is what OpenAPI defaults a query parameter to.
#[test]
fn a_list_parameter_reaches_the_query_the_way_the_document_explodes_it() {
    let both = |document: &str| {
        sent(
            document,
            "listVouchers",
            Values::new().each("tag", ["a", "b"]),
        )
    };

    assert_eq!(
        both(&synthetic(LISTED)),
        "http://localhost:9999/vouchers?tag=a&tag=b"
    );
    // The default is the same rendering, written down.
    assert_eq!(
        both(&synthetic(&LISTED.replace("          explode: true\n", ""))),
        "http://localhost:9999/vouchers?tag=a&tag=b"
    );
    assert_eq!(
        both(&synthetic(
            &LISTED.replace("explode: true", "explode: false")
        )),
        "http://localhost:9999/vouchers?tag=a,b"
    );
}

/// Percent-encoding runs before the comma is written, so the comma *between*
/// two values and a comma *inside* one value are not the same character on the
/// wire, and a server reading the field gets the two values that were given.
#[test]
fn a_comma_inside_a_value_is_not_the_comma_between_two_values() {
    assert_eq!(
        sent(
            &synthetic(&LISTED.replace("explode: true", "explode: false")),
            "listVouchers",
            Values::new().each("tag", ["a,b", "c"]),
        ),
        "http://localhost:9999/vouchers?tag=a%2Cb,c"
    );
}

/// The regression this shape exists for. One parameter no flag can carry is an
/// operation's problem; it used to be the whole document's, which made every
/// other operation in it unreachable as well.
#[test]
fn a_parameter_no_flag_can_carry_leaves_every_other_operation_standing() {
    let doc = Document::load(&synthetic(SHAPES), &[])
        .expect("an unreachable parameter does not stop the document reducing");
    let list = doc.get("listVouchers").expect("listVouchers");

    assert!(
        matches!(
            list.param("filter")
                .expect("it is in the reduction")
                .shape(),
            Shape::Unreachable(Unsupported::Structured)
        ),
        "an object parameter is carried, not dropped and not refused"
    );
    // The operation that declares it is mounted, and its other parameters work.
    assert_eq!(flag_of(list.param("limit").expect("limit")), "limit");
    assert_eq!(
        sent(
            &synthetic(SHAPES),
            "listVouchers",
            Values::new().each("tag", ["a"]).param("limit", 5),
        ),
        "http://localhost:9999/vouchers?tag=a&limit=5"
    );
    // And so is every operation that never mentioned it.
    assert!(doc.get("createContact").is_some(), "the other operation");

    // A value for it is refused rather than dropped: a request quietly missing
    // the filter it was given is worse than one that was never built.
    let refused = Invocation::new(list, Values::new().param("filter", "{}"))
        .expect_err("there is nowhere to put it");
    assert_eq!(
        refused.to_string(),
        "listVouchers: `filter` is neither a value nor a list of values, \
         so there is nowhere in the request to put a value for it"
    );
}

/// An operation whose caller *must* send what this CLI cannot spell could never
/// be invoked correctly, so it is named while the document is reduced rather
/// than mounted as a subcommand guaranteed to build the wrong request.
#[test]
fn a_required_parameter_no_flag_can_carry_names_itself_and_the_way_out() {
    let error = Document::load(
        &synthetic(&SHAPES.replace("required: false", "required: true")),
        &[],
    )
    .expect_err("a required parameter with no flag");
    assert_eq!(
        error.to_string(),
        "listVouchers: parameter `filter` is neither a value nor a list of values, \
         and the document requires it; correct the parameter in an Overlay, \
         or drop its `required`"
    );
}

/// `in: cookie` and a parameter described by `content` are the same shape as an
/// object: something one operation asks for that this CLI has no spelling for.
/// One rule covers all three, so none of them costs the document anything.
#[test]
fn a_cookie_and_a_content_parameter_are_carried_the_way_an_object_is() {
    const NEIGHBOURS: &str = r"  /vouchers:
    get:
      operationId: listVouchers
      parameters:
        - name: session
          in: cookie
          schema: { type: string }
        - name: window
          in: query
          content:
            application/json:
              schema: { type: object }
      responses: { '200': { description: OK } }
";

    let doc = Document::load(&synthetic(NEIGHBOURS), &[]).expect("the document still reduces");
    let op = doc.get("listVouchers").expect("listVouchers");
    let why = |name: &str| match op.param(name).expect("it is in the reduction").shape() {
        Shape::Unreachable(why) => why.clone(),
        Shape::Flag { .. } => panic!("`{name}` has no command-line spelling"),
    };
    assert_eq!(why("session"), Unsupported::Cookie);
    assert_eq!(why("window"), Unsupported::Encoded);

    // Neither grows a flag, and the long help says why rather than leaving a
    // reader of `--help` to wonder where the parameter went.
    let command = tree::command(op);
    let longs: Vec<&str> = command
        .get_arguments()
        .filter_map(clap::Arg::get_long)
        .collect();
    assert!(
        !longs.contains(&"session") && !longs.contains(&"window"),
        "{longs:?}"
    );
    let long_about = command.get_long_about().expect("a long help").to_string();
    assert!(
        long_about
            .contains("`session` has no flag: it is `in: cookie`, which this CLI does not send."),
        "{long_about}"
    );
    assert!(
        long_about.contains(
            "`window` has no flag: it is described by `content`, which this CLI does not encode."
        ),
        "{long_about}"
    );
}

/// A serialisation this crate does not write is named on the parameter that
/// declares it. Writing it as `form` instead would put the values on the wire
/// in a shape the server does not read, which is a request that looks sent.
#[test]
fn a_style_this_crate_does_not_serialise_names_itself() {
    const STYLED: &str = r"  /vouchers:
    get:
      operationId: listVouchers
      parameters:
        - name: tag
          in: query
          required: false
          style: pipeDelimited
          schema: { type: array, items: { type: string } }
      responses: { '200': { description: OK } }
";

    /// The same list in a path segment, where a parameter has styles of its own
    /// and is required by definition.
    const SEGMENTED: &str = r"  /vouchers/{ids}:
    get:
      operationId: getVouchers
      parameters:
        - name: ids
          in: path
          required: true
          style: matrix
          schema: { type: array, items: { type: integer } }
      responses: { '200': { description: OK } }
";

    for style in ["spaceDelimited", "pipeDelimited", "deepObject"] {
        let document = synthetic(&STYLED.replace("pipeDelimited", style));
        let doc = Document::load(&document, &[]).expect("the document still reduces");
        let op = doc.get("listVouchers").expect("listVouchers");
        let Shape::Unreachable(why) = op.param("tag").expect("tag").shape() else {
            panic!("`{style}` is not a serialisation this crate writes");
        };
        assert_eq!(
            why.to_string(),
            format!("declared with `style: {style}`, which this CLI does not serialise")
        );

        // Required, the same parameter is the document's problem, and the
        // refusal carries the style's own spelling.
        let error = Document::load(&document.replace("required: false", "required: true"), &[])
            .expect_err("a required parameter with no flag");
        assert!(
            error.to_string().contains(&format!("`style: {style}`")),
            "{error}"
        );
    }

    // A path has styles of its own, and they are not only about delimiters:
    // `matrix` puts a `;ids=` in front of one value as surely as in front of a
    // list, so both schemas answer the same way. A path parameter is required
    // by definition, so both are the document's problem.
    for style in ["matrix", "label"] {
        for schema in [
            "{ type: integer }",
            "{ type: array, items: { type: integer } }",
        ] {
            let document = synthetic(
                &SEGMENTED
                    .replace("matrix", style)
                    .replace("{ type: array, items: { type: integer } }", schema),
            );
            let error =
                Document::load(&document, &[]).expect_err("a path style this crate does not write");
            assert!(
                error.to_string().contains(&format!("`style: {style}`")),
                "{schema}: {error}"
            );
        }
    }
}

/// A path segment and a header are `style: simple`, which comma-separates a list
/// however it explodes. A path parameter given twice is one segment, not a
/// second value silently dropped.
#[test]
fn a_list_in_a_path_segment_or_a_header_is_comma_separated() {
    const SEGMENTED: &str = r"  /vouchers/{ids}:
    get:
      operationId: getVouchers
      parameters:
        - name: ids
          in: path
          required: true
          schema: { type: array, items: { type: integer } }
        - name: X-Trace
          in: header
          schema: { type: array, items: { type: string } }
      responses: { '200': { description: OK } }
";

    let doc = Document::load(&synthetic(SEGMENTED), &[]).expect("a document");
    let op = doc.get("getVouchers").expect("getVouchers");
    let request = Invocation::new(
        op,
        Values::new()
            .each("ids", [3, 4, 5])
            .each("X-Trace", ["one", "two"]),
    )
    .expect("the values satisfy the operation")
    .request(doc.base())
    .expect("the base URL is a URL");

    assert_eq!(request.uri().path(), "/vouchers/3,4,5");
    assert_eq!(
        request.headers().get("x-trace").expect("the header"),
        "one,two"
    );
}

/// A parameter the document declares one value for, given two, is refused: the
/// list the caller meant is not a list the document describes.
#[test]
fn a_parameter_that_is_not_a_list_is_refused_a_second_value() {
    let doc = Document::load(&synthetic(SHAPES), &[]).expect("a document");
    let op = doc.get("listVouchers").expect("listVouchers");
    let refused = Invocation::new(op, Values::new().each("limit", [5, 6]))
        .expect_err("`limit` is one integer");
    assert_eq!(
        refused.to_string(),
        "listVouchers: `limit` takes one value, and was given 2"
    );
}

/// The command line's half of the same facts: the flag is repeatable, its help
/// line says what the repeats become, and what the tree reads back builds the
/// request the document describes.
#[test]
fn a_repeatable_flag_says_what_it_does_and_reaches_the_request_builder_repeated() {
    let doc = Document::load(&synthetic(SHAPES), &[]).expect("a document");
    let op = doc.get("listVouchers").expect("listVouchers");
    let command = tree::command(op);
    let tag = command
        .get_arguments()
        .find(|arg| arg.get_long() == Some("tag"))
        .expect("--tag");

    assert!(matches!(tag.get_action(), clap::ArgAction::Append));
    let help = tag.get_help().expect("a help line").to_string();
    assert!(
        help.contains("repeatable; each value is sent as its own field"),
        "{help}"
    );

    let matches = clap::Command::new("toy")
        .subcommands(tree::commands(&doc))
        .get_matches_from(["toy", "vouchers", "list", "--tag", "a", "--tag", "b"]);
    let Asked::Run(selected) =
        tree::select(&doc, &matches).expect("the subcommand names an operation")
    else {
        panic!("this command line runs the operation");
    };
    let request = Invocation::new(selected.operation(), selected.values().clone())
        .expect("the flags satisfy the operation")
        .request(doc.base())
        .expect("the base URL is a URL");

    assert_eq!(request.uri().query(), Some("tag=a&tag=b"));
}

/// The two doors onto one reduction. A bless step writes the blob, a binary
/// reads it, and nothing between them may change what the document said —
/// including the flag renames, which are decided while reducing and would be a
/// different command line if they were decided again on the way back.
#[test]
fn a_reduction_survives_the_round_trip_the_bless_step_makes() {
    let doc = document();
    let blob = doc.to_blob().expect("the reduction encodes");
    assert_eq!(Document::from_blob(&blob).expect("and decodes"), doc);
}

/// A blob that is not one is a named error, not a panic and not a CLI that
/// starts with half an API.
#[test]
fn a_blob_that_is_not_a_reduction_is_refused_by_name() {
    let error = Document::from_blob(b"not a reduction").expect_err("not a reduction");
    assert!(error.to_string().contains("reduced model"), "{error}");
}

/// The flag a parameter grows, for a test that is about the name rather than
/// about the shape.
fn flag_of(param: &Param) -> &str {
    match param.shape() {
        Shape::Flag { flag, .. } => flag,
        Shape::Unreachable(why) => panic!("`{}` has no flag: it is {why}", param.name()),
    }
}

/// The rules a parameter's values are held to, for a test that is about the
/// rules rather than about the flag.
fn scalar_of(param: &Param) -> &Scalar {
    match param.shape() {
        Shape::Flag { scalar, .. } => scalar,
        Shape::Unreachable(why) => panic!("`{}` takes no value: it is {why}", param.name()),
    }
}

/// A document of this test's own, `paths` and nothing else — for the naming
/// rules, which are about shapes the toy fixture does not have.
fn synthetic(paths: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         paths:\n{paths}"
    )
}

/// What the tree calls every operation, in document order.
fn placements(document: &str) -> Vec<String> {
    Document::load(document, &[])
        .expect("a document")
        .iter()
        .map(|op| format!("{} {}", op.group(), op.command()))
        .collect()
}

/// A segment every path shares tells nothing apart, so it is not the group: a
/// document served entirely under `/v1` must not collapse into one group
/// named `v1`.
#[test]
fn a_prefix_every_path_shares_is_not_the_group() {
    let placed = placements(&synthetic(
        "  /v1/vouchers:\n\
         \x20   get: { operationId: listVouchers, responses: { \"200\": { description: OK } } }\n\
         \x20 /v1/contacts:\n\
         \x20   post: { operationId: createContact, responses: { \"201\": { description: OK } } }\n",
    ));
    assert_eq!(placed, ["vouchers list", "contacts create"]);
}

/// Two operations under one name would silently shadow each other. The
/// document is refused instead, naming both and the way out.
#[test]
fn two_operations_under_one_name_are_refused_by_both_ids() {
    const COLLIDING: &str = "  /vouchers/{id}/render:\n\
         \x20   get: { operationId: renderVoucher, responses: { \"200\": { description: OK } } }\n\
         \x20 /vouchers/{id}/pdf/render:\n\
         \x20   get: { operationId: renderVoucherPdf, responses: { \"200\": { description: OK } } }\n";

    let error = Document::load(&synthetic(COLLIDING), &[]).expect_err("both are `vouchers render`");

    assert_eq!(
        error.to_string(),
        "`renderVoucher` and `renderVoucherPdf` are both `vouchers render` on the \
         command line; give one of them an `x-cli-command`"
    );

    // And the way out the message names is the way out.
    let resolved = COLLIDING.replace(
        "operationId: renderVoucherPdf",
        "operationId: renderVoucherPdf, x-cli-command: render-pdf",
    );
    assert_eq!(
        placements(&synthetic(&resolved)),
        ["vouchers render", "vouchers render-pdf"]
    );
}

/// The path is where a name comes from; the document is where it is overruled.
/// `x-cli-group` and `x-cli-command` are the adopter's say, written in the
/// same Overlay as every other correction.
#[test]
fn the_document_may_name_its_own_group_and_command() {
    let placed = placements(&synthetic(
        "  /vouchers/{id}/render:\n\
         \x20   get:\n\
         \x20     operationId: renderVoucher\n\
         \x20     x-cli-group: reports\n\
         \x20     x-cli-command: pdf\n\
         \x20     responses: { \"200\": { description: OK } }\n\
         \x20 /contacts:\n\
         \x20   post: { operationId: createContact, responses: { \"201\": { description: OK } } }\n",
    ));
    assert_eq!(placed, ["reports pdf", "contacts create"]);
}

/// A marker that is there and is not a name is the document saying something
/// this crate has no reading for — refused, rather than passed over in favour
/// of the name it was meant to override.
#[test]
fn a_marker_that_is_not_a_name_is_refused() {
    let error = Document::load(
        &synthetic(
            "  /vouchers:\n\
             \x20   get:\n\
             \x20     operationId: listVouchers\n\
             \x20     x-cli-command: [a, b]\n\
             \x20     responses: { \"200\": { description: OK } }\n",
        ),
        &[],
    )
    .expect_err("a list is not a name");
    assert_eq!(
        error.to_string(),
        "listVouchers: `x-cli-command` is not a string"
    );
}

/// Every way a gate can be wrong, refused while the document is reduced and
/// naming the operation and the word.
///
/// A gate that reached a shipped binary would be a flag somebody is about to
/// type, so all of this is the adopter's failure at bless time rather than a
/// user's at the prompt.
#[test]
fn a_gate_the_command_line_cannot_offer_is_refused_by_name() {
    let refused = |method: &str, marker: &str| {
        let paths = format!(
            "  /vouchers/{{id}}/enshrine:\n\
             \x20   {method}:\n\
             \x20     operationId: enshrineVoucher\n\
             \x20     x-cli-gates: {marker}\n\
             \x20     responses: {{ \"200\": {{ description: OK }} }}\n"
        );
        Document::load(&synthetic(&paths), &[])
            .expect_err("the document names a gate it cannot offer")
            .to_string()
    };

    // One word where a list goes would otherwise be no gate at all, which is
    // the one outcome a default-closed gate must never reach by accident.
    assert_eq!(
        refused("post", "enshrine"),
        "enshrineVoucher: `x-cli-gates` is not a list of names"
    );
    assert_eq!(
        refused("post", "[3]"),
        "enshrineVoucher: `x-cli-gates` is not a list of names"
    );
    assert_eq!(
        refused("post", "[\"???\"]"),
        "the x-cli-gates `???` does not kebab-case into [a-z0-9-]"
    );
    assert_eq!(
        refused("post", "[commit]"),
        "enshrineVoucher: the gate `commit` is one of the flags every subcommand \
         already spends"
    );
    assert_eq!(
        refused("post", "[enshrine, enshrine]"),
        "enshrineVoucher: the gate `enshrine` is named twice"
    );
    // A read runs on sight, so there is nothing for a gate to hold back: the
    // document is saying two things at once and does not say which it meant.
    assert_eq!(
        refused("get", "[enshrine]"),
        "enshrineVoucher: a read stands behind no gate, and this one names \
         `enshrine`; mark the operation `x-cli-writes: true` or drop the gate"
    );
}

/// A gate's flag is claimed before the document's own names are, so a body
/// field the vendor happens to spell like one moves aside instead of shadowing
/// the word standing in front of the hazard.
#[test]
fn a_body_field_that_collides_with_a_gate_moves_aside() {
    let doc = Document::load(
        &synthetic(
            "  /vouchers/{id}/enshrine:\n\
             \x20   post:\n\
             \x20     operationId: enshrineVoucher\n\
             \x20     x-cli-gates: [enshrine]\n\
             \x20     requestBody:\n\
             \x20       content:\n\
             \x20         application/json:\n\
             \x20           schema:\n\
             \x20             type: object\n\
             \x20             properties:\n\
             \x20               enshrine: { type: string }\n\
             \x20     responses: { \"200\": { description: OK } }\n",
        ),
        &[],
    )
    .expect("a document whose body field is spelled like its gate");

    let op = doc.get("enshrineVoucher").unwrap();
    let Body::JsonFields(fields) = op.body() else {
        panic!("enshrineVoucher takes a flat JSON body");
    };
    let field = fields.iter().find(|f| f.name() == "enshrine").unwrap();
    assert_eq!(field.flag(), "body-enshrine");
    assert!(field.renamed(), "and it says so in its help line");
    // Both flags are on the subcommand, which they could not be if one had
    // shadowed the other: clap panics on a duplicate name, and this is where.
    tree::command(op).debug_assert();
}

/// An override that will not reduce to a command name is rejected rather than
/// mangled, and the message says which of the document's own words to look at.
#[test]
fn an_override_that_is_not_spellable_names_itself() {
    let error = Document::load(
        &synthetic(
            "  /vouchers:\n\
             \x20   get:\n\
             \x20     operationId: listVouchers\n\
             \x20     x-cli-group: \"???\"\n\
             \x20     responses: { \"200\": { description: OK } }\n",
        ),
        &[],
    )
    .expect_err("`???` is not a name");
    assert_eq!(
        error.to_string(),
        "the x-cli-group `???` does not kebab-case into [a-z0-9-]"
    );
}
