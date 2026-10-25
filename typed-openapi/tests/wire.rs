//! The request goes to the path and the host that the document declares.
//!
//! `{id}` never goes on the wire, a path-level parameter that an operation
//! overrides can be invoked, a credential never becomes a flag, and no request
//! goes to a host the document did not mean. Each case is the input of a probe
//! of SEV-249 (`p01` to `p06` and `p11d` of `A3a/reduce`). A case that refuses a
//! document asserts the variant beside the words, and where the words name a
//! repair the case loads the document with that repair, so they are true.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use clap::error::ErrorKind;
use typed_openapi::{
    Document, Invocation, LoadError, Location, Shape, Unsupported, ValueError, Values, tree,
};

/// A document with one server and whatever the case puts under `paths`.
fn document(paths: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         paths:\n{paths}"
    )
}

fn loaded(paths: &str) -> Document {
    Document::load(&document(paths), &[]).expect("the document loads")
}

fn refused(paths: &str) -> LoadError {
    Document::load(&document(paths), &[]).expect_err("the document is refused")
}

/// An Overlay with one action that removes `target`.
fn removing(target: &str) -> String {
    format!(
        "overlay: 1.1.0\ninfo: {{ title: t, version: \"1\" }}\nactions:\n  - target: \"{target}\"\n    \
         remove: true\n"
    )
}

/// An Overlay with one action that updates `target` with `update`.
fn updating(target: &str, update: &str) -> String {
    format!(
        "overlay: 1.1.0\ninfo: {{ title: t, version: \"1\" }}\nactions:\n  - target: \"{target}\"\n    \
         update: {update}\n"
    )
}

/// The URI of the request that `values` make of `id`.
fn uri(doc: &Document, id: &str, values: Values) -> String {
    let op = doc.get(id).expect("the operation");
    Invocation::new(op, values)
        .expect("the values satisfy the operation")
        .request(doc.base())
        .expect("a request")
        .uri()
        .to_string()
}

fn names(doc: &Document, id: &str) -> Vec<String> {
    let op = doc.get(id).expect("the operation");
    op.params().iter().map(|p| p.name().to_owned()).collect()
}

// ------------------------------------------------------------------ F-10

const OVERRIDDEN: &str = "  /vouchers/{id}:
    parameters:
      - { name: id, in: path, required: true, schema: { type: string } }
    get:
      operationId: getVoucher
      parameters:
        - { name: id, in: path, required: true, description: \"The numeric id\", schema: { type: integer, minimum: 1 } }
      responses: { \"200\": { description: OK } }
";

/// `p01`. The operation gets two required flags, `--id` and `--param-id`, and
/// neither alone nor both together can be sent.
#[test]
fn a_path_level_parameter_that_the_operation_overrides_is_one_parameter() {
    let doc = loaded(OVERRIDDEN);
    let op = doc.get("getVoucher").expect("the operation");

    assert_eq!(names(&doc, "getVoucher"), ["id"]);
    let [id] = op.params() else {
        panic!("one parameter: {:?}", op.params())
    };
    assert_eq!(
        id.description(),
        Some("The numeric id"),
        "the entry of the operation wins"
    );
    assert_eq!(
        uri(&doc, "getVoucher", Values::new().param("id", 5)),
        "http://localhost:9999/vouchers/5"
    );
}

/// The path-level entry says `string` and the operation's says an integer from
/// 1, so a value that only the operation's entry refuses shows which one won.
#[test]
fn the_entry_of_the_operation_is_the_one_that_holds_the_value_to_its_rules() {
    let doc = loaded(OVERRIDDEN);
    let op = doc.get("getVoucher").expect("the operation");

    let error = Invocation::new(op, Values::new().param("id", "0")).expect_err("below minimum");

    assert!(
        matches!(&error, ValueError::BadValue { op, name, .. } if op == "getVoucher" && name == "id"),
        "{error:?}"
    );
}

/// The operation's entry takes the place of the one it replaces, so what the
/// path item lists keeps its order, and the operation's other parameters follow.
#[test]
fn an_override_keeps_the_place_of_the_entry_it_replaces() {
    let doc = loaded(
        "  /things/{id}:
    parameters:
      - { name: id, in: path, required: true, schema: { type: string } }
      - { name: a, in: query, schema: { type: string } }
    get:
      operationId: getThing
      parameters:
        - { name: b, in: query, schema: { type: string } }
        - { name: id, in: path, required: true, description: \"the override\", schema: { type: integer } }
      responses: { \"200\": { description: OK } }
",
    );
    let op = doc.get("getThing").expect("the operation");

    assert_eq!(names(&doc, "getThing"), ["id", "a", "b"]);
    assert_eq!(op.params()[0].description(), Some("the override"));
}

/// Two entries of the operation with one name and place are two parameters, and
/// not one that replaced the other: the second has no flag, as any namesake.
#[test]
fn a_second_entry_of_the_operation_with_the_same_name_and_place_replaces_nothing() {
    let doc = loaded(
        "  /things:
    parameters:
      - { name: limit, in: query, schema: { type: string } }
    get:
      operationId: listThings
      parameters:
        - { name: limit, in: query, schema: { type: integer } }
        - { name: limit, in: query, schema: { type: integer } }
      responses: { \"200\": { description: OK } }
",
    );
    let op = doc.get("listThings").expect("the operation");

    let [first, second] = op.params() else {
        panic!("two parameters: {:?}", op.params())
    };
    assert!(matches!(first.shape(), Shape::Flag { .. }));
    assert_eq!(
        second.shape(),
        &Shape::Unreachable(Unsupported::Shared("query".to_owned()))
    );
}

/// A parameter is identified by its name and its location. The path item's
/// `ref` is in the path and the operation's is in the query, so they are two
/// parameters, and the operation's does not take the place of the path item's.
#[test]
fn a_parameter_in_another_location_overrides_nothing() {
    let doc = loaded(
        "  /logs/{ref}:
    parameters:
      - { name: ref, in: path, required: true, schema: { type: string } }
    post:
      operationId: appendLogEntry
      parameters:
        - { name: ref, in: query, schema: { type: string } }
      responses: { \"201\": { description: OK } }
",
    );
    let op = doc.get("appendLogEntry").expect("the operation");

    let places: Vec<_> = op
        .params()
        .iter()
        .map(|p| match p.shape() {
            Shape::Flag { location, .. } => Ok(*location),
            Shape::Unreachable(why) => Err(why.clone()),
        })
        .collect();
    assert_eq!(
        places,
        [
            Ok(Location::Path),
            Err(Unsupported::Shared("path".to_owned()))
        ]
    );
}

const SHARED: &str = "  /logs/{ref}:
    post:
      operationId: appendLogEntry
      parameters:
        - { name: ref, in: path, required: true, schema: { type: string } }
        - { name: ref, in: query, schema: { type: string } }
      responses: { \"201\": { description: OK } }
";

/// `p02`. One wire name in the path and the query is legal, and the flag for the
/// query's could never send, because every value was filed under the path's.
#[test]
fn of_two_parameters_with_one_name_the_one_the_document_lists_later_is_unreachable() {
    let doc = loaded(SHARED);
    let op = doc.get("appendLogEntry").expect("the operation");

    let [path, query] = op.params() else {
        panic!("two parameters: {:?}", op.params())
    };
    assert!(matches!(
        path.shape(),
        Shape::Flag {
            location: Location::Path,
            ..
        }
    ));
    assert_eq!(
        query.shape(),
        &Shape::Unreachable(Unsupported::Shared("path".to_owned()))
    );
    assert_eq!(
        Unsupported::Shared("path".to_owned()).to_string(),
        "sharing its name with the `path` parameter"
    );
}

/// `Values` is keyed by name, so the value reaches the parameter that holds the
/// name and the query string stays empty.
#[test]
fn a_value_for_a_shared_name_reaches_the_parameter_that_holds_it() {
    let doc = loaded(SHARED);

    assert_eq!(
        uri(&doc, "appendLogEntry", Values::new().param("ref", "a")),
        "http://localhost:9999/logs/a"
    );
}

/// A list that holds the name is given more than once, and the namesake that
/// has no flag is not the parameter that was given the values.
#[test]
fn the_values_of_a_list_are_not_counted_against_a_namesake() {
    let doc = loaded(
        "  /things:
    get:
      operationId: listThings
      parameters:
        - { name: tag, in: query, schema: { type: array, items: { type: string } } }
        - { name: tag, in: header, schema: { type: string } }
      responses: { \"200\": { description: OK } }
",
    );

    assert_eq!(
        uri(&doc, "listThings", Values::new().each("tag", ["a", "b"])),
        "http://localhost:9999/things?tag=a&tag=b"
    );
}

/// A required parameter that no flag can carry is the document's to mend, so it
/// is refused by name as any other is.
#[test]
fn a_required_parameter_that_shares_a_name_refuses_the_document_by_name() {
    let paths = "  /logs/{ref}:
    post:
      operationId: appendLogEntry
      parameters:
        - { name: ref, in: path, required: true, schema: { type: string } }
        - { name: ref, in: query, required: true, schema: { type: string } }
      responses: { \"201\": { description: OK } }
";

    let error = refused(paths);

    assert!(
        matches!(&error, LoadError::Parameter { op, name, why }
            if op == "appendLogEntry" && name == "ref"
                && *why == Unsupported::Shared("path".to_owned())),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "appendLogEntry: parameter `ref` is sharing its name with the `path` parameter, and the \
         document requires it; correct the parameter in an Overlay, or drop its `required`"
    );
    let mended = Document::load(
        &document(paths),
        &[&updating(
            "$.paths['/logs/{ref}'].post.parameters[1]",
            "{ required: false }",
        )],
    )
    .expect("the Overlay drops the `required`");
    assert!(mended.get("appendLogEntry").is_some());
}

// ------------------------------------------------------------------ F-11

/// `p03`. The request went to `/vouchers/{id}` with the braces in it.
#[test]
fn a_template_variable_with_no_path_parameter_is_refused() {
    const PATHS: &str = "  /vouchers/{id}:
    get:
      operationId: getVoucher
      responses: { \"200\": { description: OK } }
";

    let error = refused(PATHS);

    assert!(
        matches!(&error, LoadError::UnboundVariable { op, path, name }
            if op == "getVoucher" && path == "/vouchers/{id}" && name == "id"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "getVoucher: `/vouchers/{id}` has the variable `{id}`, and the operation declares no \
         path parameter of that name; add one in an Overlay"
    );
    let mended = Document::load(
        &document(PATHS),
        &[&updating(
            "$.paths['/vouchers/{id}'].get",
            "{ parameters: [{ name: id, in: path, required: true, schema: { type: string } }] }",
        )],
    )
    .expect("the Overlay adds the parameter");
    assert_eq!(names(&mended, "getVoucher"), ["id"]);
}

/// A parameter of another location with the variable's name does not answer it.
#[test]
fn a_template_variable_is_not_answered_by_a_query_parameter() {
    let error = refused(
        "  /vouchers/{id}:
    get:
      operationId: getVoucher
      parameters:
        - { name: id, in: query, required: true, schema: { type: string } }
      responses: { \"200\": { description: OK } }
",
    );

    assert!(
        matches!(&error, LoadError::UnboundVariable { name, .. } if name == "id"),
        "{error:?}"
    );
}

/// `p04`. The flag was required and its value was thrown away.
#[test]
fn a_path_parameter_with_no_template_variable_is_refused() {
    const PATHS: &str = "  /vouchers:
    get:
      operationId: listVouchers
      parameters:
        - { name: id, in: path, required: true, schema: { type: string } }
      responses: { \"200\": { description: OK } }
";

    let error = refused(PATHS);

    assert!(
        matches!(&error, LoadError::UnusedPathParameter { op, path, name }
            if op == "listVouchers" && path == "/vouchers" && name == "id"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "listVouchers: the path parameter `id` has no `{id}` in `/vouchers`; remove the \
         parameter in an Overlay"
    );
    let mended = Document::load(
        &document(PATHS),
        &[&removing("$.paths['/vouchers'].get.parameters[0]")],
    )
    .expect("the Overlay removes the parameter");
    assert_eq!(names(&mended, "listVouchers"), Vec::<String>::new());
}

/// `p05`. The flag was optional, and the request then sent `{id}`.
#[test]
fn a_path_parameter_that_is_not_required_is_refused() {
    const PATHS: &str = "  /vouchers/{id}:
    get:
      operationId: getVoucher
      parameters:
        - { name: id, in: path, schema: { type: string } }
      responses: { \"200\": { description: OK } }
";

    let error = refused(PATHS);

    assert!(
        matches!(&error, LoadError::OptionalPathParameter { op, name }
            if op == "getVoucher" && name == "id"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "getVoucher: the path parameter `id` is not `required`, and OpenAPI says a path \
         parameter is; set `required: true` in an Overlay"
    );
    let mended = Document::load(
        &document(PATHS),
        &[&updating(
            "$.paths['/vouchers/{id}'].get.parameters[0]",
            "{ required: true }",
        )],
    )
    .expect("the Overlay requires it");
    assert!(mended.get("getVoucher").is_some());
}

/// A variable that the path repeats is one variable, and a path-level
/// parameter answers it for every operation under the path item.
#[test]
fn a_repeated_variable_and_a_path_level_parameter_are_answered() {
    let doc = loaded(
        "  /a/{id}/b/{id}:
    parameters:
      - { name: id, in: path, required: true, schema: { type: string } }
    get:
      operationId: getBoth
      responses: { \"200\": { description: OK } }
",
    );

    assert_eq!(
        uri(&doc, "getBoth", Values::new().param("id", 7)),
        "http://localhost:9999/a/7/b/7"
    );
}

// ------------------------------------------------------------------ F-14

const CREDENTIALS: &str = "  /vouchers:
    post:
      operationId: createVoucher
      parameters:
        - { name: Accept, in: header, schema: { type: string } }
        - { name: Content-Type, in: header, required: true, schema: { type: string } }
        - { name: Authorization, in: header, required: true, schema: { type: string } }
        - { name: X-Request-Id, in: header, schema: { type: string } }
      requestBody:
        required: true
        content:
          application/json:
            schema: { type: object, properties: { total: { type: string } } }
      responses: { \"201\": { description: OK } }
";

/// `p06`. A required `Authorization` is not `Unreachable`, because that would
/// refuse the document, and it is not a parameter at all.
#[test]
fn the_three_header_names_are_dropped_and_the_document_loads() {
    let doc = loaded(CREDENTIALS);

    assert_eq!(names(&doc, "createVoucher"), ["X-Request-Id"]);
}

/// `ctype.yaml` sent two `content-type` headers, and `main.yaml` put a bearer
/// token in argv and in the dry run.
#[test]
fn a_request_carries_the_documents_content_type_and_no_credential_of_the_document() {
    let doc = loaded(CREDENTIALS);
    let op = doc.get("createVoucher").expect("the operation");
    let values = Values::new()
        .param("X-Request-Id", "r1")
        .json(serde_json::json!({ "total": "5" }));

    let request = Invocation::new(op, values)
        .expect("the values satisfy the operation")
        .request(doc.base())
        .expect("a request");

    let mut sent: Vec<_> = request
        .headers()
        .keys()
        .map(|k| k.as_str().to_owned())
        .collect();
    sent.sort();
    assert_eq!(sent, ["content-type", "x-request-id"]);
    assert_eq!(request.headers()["content-type"], "application/json");
}

#[test]
fn a_value_for_a_dropped_header_is_for_a_parameter_the_operation_does_not_have() {
    let doc = loaded(CREDENTIALS);
    let op = doc.get("createVoucher").expect("the operation");
    let values = Values::new()
        .param("Authorization", "Bearer s3cr3t")
        .json(serde_json::json!({ "total": "5" }));

    let error = Invocation::new(op, values).expect_err("nowhere to put it");

    assert!(
        matches!(&error, ValueError::UnknownParam { op, name }
            if op == "createVoucher" && name == "Authorization"),
        "{error:?}"
    );
}

/// HTTP header names are not case sensitive, and the specification names them in
/// one spelling.
#[test]
fn the_names_are_compared_without_case_and_only_in_a_header() {
    let doc = loaded(
        "  /things:
    parameters:
      - { name: AUTHORIZATION, in: header, required: true, schema: { type: string } }
    get:
      operationId: listThings
      parameters:
        - { name: accept, in: header, schema: { type: string } }
        - { name: CONTENT-TYPE, in: header, required: true, schema: { type: string } }
        - { name: Authorization, in: query, schema: { type: string } }
        - { name: Accept-Language, in: header, schema: { type: string } }
      responses: { \"200\": { description: OK } }
",
    );

    assert_eq!(
        names(&doc, "listThings"),
        ["Authorization", "Accept-Language"]
    );
}

/// A credential never becomes a flag.
#[test]
fn no_flag_is_grown_for_the_dropped_headers() {
    let doc = loaded(CREDENTIALS);
    let op = doc.get("createVoucher").expect("the operation");

    for flag in ["--authorization", "--content-type", "--accept"] {
        let error = tree::command(op)
            .try_get_matches_from(["create", flag, "x", "--commit"])
            .expect_err(flag);
        assert_eq!(error.kind(), ErrorKind::UnknownArgument, "{flag}");
    }
}

// ------------------------------------------------------------------ F-20

/// `p11d`. The request went to the document's first server, and the upload
/// server was named in the document.
fn upload(item_servers: &str, operation_servers: &str) -> String {
    format!(
        "  /upload:\n{item_servers}    post:\n      operationId: upload\n{operation_servers}      \
         responses: {{ \"200\": {{ description: OK }} }}\n"
    )
}

const SERVERS: &str = "servers: [{ url: 'https://uploads.example.com' }]\n";

#[test]
fn servers_on_an_operation_are_refused_with_the_overlay_that_removes_them() {
    let paths = upload("", &format!("      {SERVERS}"));

    let error = refused(&paths);

    assert!(
        matches!(&error, LoadError::Servers { op, at }
            if op == "upload" && at == "$.paths['/upload'].post.servers"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "upload: `servers` stands on `$.paths['/upload'].post.servers`, and this crate sends \
         every request to the first server of the document; remove it in an Overlay"
    );
    let mended = Document::load(
        &document(&paths),
        &[&removing("$.paths['/upload'].post.servers")],
    )
    .expect("the Overlay removes them");
    assert!(mended.get("upload").is_some());
}

#[test]
fn servers_on_a_path_item_are_refused_and_name_an_operation_under_it() {
    let paths = upload(&format!("    {SERVERS}"), "");

    let error = refused(&paths);

    assert!(
        matches!(&error, LoadError::Servers { op, at }
            if op == "upload" && at == "$.paths['/upload'].servers"),
        "{error:?}"
    );
    let mended = Document::load(
        &document(&paths),
        &[&removing("$.paths['/upload'].servers")],
    )
    .expect("the Overlay removes them");
    assert!(mended.get("upload").is_some());
}

/// The probe sets both, and the refusal is of the path item, which it reads
/// first.
#[test]
fn servers_on_both_are_refused_at_the_path_item() {
    let error = refused(&upload(
        &format!("    {SERVERS}"),
        &format!("      {SERVERS}"),
    ));

    assert!(
        matches!(&error, LoadError::Servers { at, .. } if at == "$.paths['/upload'].servers"),
        "{error:?}"
    );
}

/// `servers` with no entry is no server of its own, and a path item that no
/// operation sends through sends nothing anywhere.
#[test]
fn an_empty_list_and_a_path_item_with_no_operation_are_not_refused() {
    let doc = loaded(&format!(
        "{}  /unused:\n    {SERVERS}",
        upload("    servers: []\n", "      servers: []\n")
    ));

    assert!(doc.get("upload").is_some());
}
