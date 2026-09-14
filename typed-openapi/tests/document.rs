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
use typed_openapi::{
    Document, Effect, Invocation, Param, Shape, Unsupported, Values, render, tree,
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
    assert!(matches!(
        body("uploadDocument"),
        Body::Opaque { ref media_type, .. } if media_type == "form-data"
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
    assert_eq!(flag_of(update.param("id").unwrap()), "id");
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
    let selected = tree::select(&doc, &matches).expect("the subcommand names an operation");
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
