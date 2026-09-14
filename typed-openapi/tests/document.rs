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
use typed_openapi::{Document, Effect, Invocation, Operation, Values, render, tree};

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
        Body::JsonWhole { required: true }
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

#[test]
fn a_body_field_moves_aside_for_a_path_parameter_of_the_same_name() {
    let doc = document();
    let update = doc.get("updateVoucher").unwrap();
    assert_eq!(update.param("id").unwrap().flag(), "id");
    let Body::JsonFields(fields) = update.body() else {
        panic!("updateVoucher takes a flat JSON body");
    };
    let id = fields.iter().find(|f| f.name() == "id").unwrap();
    assert_eq!(id.flag(), "body-id");
    assert!(id.renamed(), "and it says so in its help line");
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
            Body::JsonWhole { required: true }
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
