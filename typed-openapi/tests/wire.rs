//! The request goes to the path and the host that the document declares.
//!
//! `{id}` never goes on the wire, a path-level parameter that an operation
//! overrides can be invoked, the `Authorization` header never becomes a flag, and
//! no request goes to a host the document did not mean. Each case is the input of
//! a probe of SEV-249 (`p01` to `p06` and `p11d` of `A3a/reduce`) or of a review
//! of the slice. A case that refuses a document asserts the variant beside the
//! words and loads the document with the repair the words name, so they are
//! true.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use clap::error::ErrorKind;
use typed_openapi::{
    Document, Invocation, LoadError, Location, Place, Shape, Unsupported, ValueError, Values, tree,
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

/// `p01`. The path item's entry and the operation's of one name and place are one
/// parameter, so one `--id` is the whole of what the request needs.
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
        &Shape::Unreachable(Unsupported::Shared(Place::Query))
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
        [Ok(Location::Path), Err(Unsupported::Shared(Place::Path))]
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

/// `p02`. One wire name in the path and the query is legal. Only the parameter
/// that comes first has a flag, because every value is filed under the name.
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
        &Shape::Unreachable(Unsupported::Shared(Place::Path))
    );
    assert_eq!(
        Unsupported::Shared(Place::Path).to_string(),
        "sharing its name with the `path` parameter"
    );
}

/// A place displays as the word the document spells `in` with.
#[test]
fn a_place_displays_as_the_word_the_document_spells_in_with() {
    let spelled = [Place::Path, Place::Query, Place::Header, Place::Cookie].map(|p| p.to_string());

    assert_eq!(spelled, ["path", "query", "header", "cookie"]);
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

// The repair that `Parameter` names depends on where the refused parameter is
// and on why it has no flag, and each one loads the document.

/// A required parameter that no flag can carry is the document's to mend, so it
/// is refused by name as any other is. Off the path it can stop being required.
#[test]
fn a_required_parameter_off_the_path_is_corrected_or_stops_being_required() {
    const PATHS: &str = "  /things:
    get:
      operationId: listThings
      parameters:
        - { name: ids, in: query, required: true, style: pipeDelimited, schema: { type: array, items: { type: string } } }
      responses: { \"200\": { description: OK } }
";
    let target = "$.paths['/things'].get.parameters[0]";

    let error = refused(PATHS);

    assert!(
        matches!(&error, LoadError::Parameter { op, name, place: Place::Query, why: Unsupported::Style(style) }
            if op == "listThings" && name == "ids" && style == "pipeDelimited"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "listThings: parameter `ids` is declared with `style: pipeDelimited`, which this CLI \
         does not serialise, and the document requires it; correct the parameter in an \
         Overlay, or drop its `required`"
    );
    for update in ["{ style: form }", "{ required: false }"] {
        Document::load(&document(PATHS), &[&updating(target, update)])
            .unwrap_or_else(|error| panic!("the Overlay `{update}` loads the document: {error}"));
    }
}

/// A path parameter that is not `required` is refused too, so dropping the
/// `required` of one is no repair, and the words do not name it.
#[test]
fn a_required_path_parameter_is_corrected_and_never_made_optional() {
    const PATHS: &str = "  /things/{id}:
    get:
      operationId: getThing
      parameters:
        - { name: id, in: path, required: true, style: matrix, schema: { type: string } }
      responses: { \"200\": { description: OK } }
";
    let target = "$.paths['/things/{id}'].get.parameters[0]";

    let error = refused(PATHS);

    assert!(
        matches!(
            &error,
            LoadError::Parameter {
                place: Place::Path,
                why: Unsupported::Style(_),
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "getThing: parameter `id` is declared with `style: matrix`, which this CLI does not \
         serialise, and the document requires it; correct the parameter in an Overlay"
    );
    let corrected = Document::load(&document(PATHS), &[&updating(target, "{ style: simple }")])
        .expect("the Overlay corrects the style");
    assert_eq!(names(&corrected, "getThing"), ["id"]);
    let optional = Document::load(
        &document(PATHS),
        &[&updating(target, "{ required: false }")],
    )
    .expect_err("a path parameter that is not required is refused");
    assert!(
        matches!(optional, LoadError::OptionalPathParameter { .. }),
        "{optional:?}"
    );
}

/// The name of a path parameter never goes on the wire, only its value in place of
/// the variable, and the template names the parameter. So no `update` of the
/// parameter alone mends a name with no spelling, and the Overlay that does
/// renames the parameter and its variable together by moving the path item. The
/// request is the same.
#[test]
fn a_path_parameter_with_no_spelling_is_renamed_with_its_variable() {
    const PATHS: &str = "  /things/{straße}:
    get:
      operationId: getThing
      parameters:
        - { name: straße, in: path, required: true, schema: { type: string } }
      responses: { \"200\": { description: OK } }
";
    const MOVE: &str = "overlay: 1.1.0
info: { title: t, version: \"1\" }
actions:
  - target: $.paths
    update:
      /things/{strasse}:
        get:
          operationId: getThing
          parameters:
            - { name: strasse, in: path, required: true, schema: { type: string } }
          responses: { \"200\": { description: OK } }
  - target: \"$.paths['/things/{straße}']\"
    remove: true
";

    let error = refused(PATHS);

    assert!(
        matches!(&error, LoadError::Parameter { op, name, place: Place::Path, why: Unsupported::Unspellable }
            if op == "getThing" && name == "straße"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "getThing: parameter `straße` is named with a character outside ASCII, or in a way that \
         does not kebab-case into [a-z0-9-], and the document requires it; rename the parameter \
         and its variable in the path together, in an Overlay that writes the path item again \
         under the new path and removes the old one"
    );
    let moved = Document::load(&document(PATHS), &[MOVE]).expect("the Overlay moves the path item");
    assert_eq!(names(&moved, "getThing"), ["strasse"]);
    let op = moved.get("getThing").expect("the operation");
    tree::command(op)
        .try_get_matches_from(["get", "--strasse", "x"])
        .expect("the flag is `--strasse`");
    assert_eq!(
        uri(&moved, "getThing", Values::new().param("strasse", "x")),
        "http://localhost:9999/things/x"
    );
}

/// Off the path every reason offers to drop `required`, whatever the place of the
/// parameter is, and the Overlay that does loads the document.
#[test]
fn a_required_header_or_cookie_parameter_can_stop_being_required() {
    let cases = [
        (
            "{ name: x-tag, in: header, required: true, content: { application/json: { schema: { type: string } } } }",
            Place::Header,
            Unsupported::Encoded,
            "listThings: parameter `x-tag` is described by `content`, which this CLI does not \
             encode, and the document requires it; correct the parameter in an Overlay, or drop \
             its `required`",
        ),
        (
            "{ name: sid, in: cookie, required: true, schema: { type: string } }",
            Place::Cookie,
            Unsupported::Cookie,
            "listThings: parameter `sid` is `in: cookie`, which this CLI does not send, and the \
             document requires it; correct the parameter in an Overlay, or drop its `required`",
        ),
    ];
    for (parameter, in_place, reason, words) in cases {
        let paths = format!(
            "  /things:\n    get:\n      operationId: listThings\n      parameters:\n        - \
             {parameter}\n      responses: {{ \"200\": {{ description: OK }} }}\n"
        );

        let error = refused(&paths);

        assert!(
            matches!(&error, LoadError::Parameter { place, why, .. } if *place == in_place && *why == reason),
            "{error:?}"
        );
        assert_eq!(error.to_string(), words);
        Document::load(
            &document(&paths),
            &[&updating(
                "$.paths['/things'].get.parameters[0]",
                "{ required: false }",
            )],
        )
        .unwrap_or_else(|error| panic!("the Overlay drops the `required` of {in_place}: {error}"));
    }
}

/// Two parameters with one name, of which the later one is required.
struct Namesakes {
    /// The earlier parameter, which holds the name.
    first: &'static str,
    /// The later parameter, which is refused.
    second: &'static str,
    /// The place of the later parameter.
    refused: Place,
    /// The place of the earlier parameter.
    earlier: Place,
    /// Where the later parameter's flag goes once the earlier one is removed.
    flag: Location,
    /// The refusal as it reads.
    words: &'static str,
}

const NAMESAKES: [Namesakes; 3] = [
    Namesakes {
        first: "{ name: q, in: header, schema: { type: string } }",
        second: "{ name: q, in: query, required: true, schema: { type: string } }",
        refused: Place::Query,
        earlier: Place::Header,
        flag: Location::Query,
        words: "listThings: parameter `q` is sharing its name with the `header` parameter, and the \
                document requires it; remove the `header` parameter `q` in an Overlay, or drop \
                its `required`",
    },
    Namesakes {
        first: "{ name: q, in: query, schema: { type: string } }",
        second: "{ name: q, in: header, required: true, schema: { type: string } }",
        refused: Place::Header,
        earlier: Place::Query,
        flag: Location::Header,
        words: "listThings: parameter `q` is sharing its name with the `query` parameter, and the \
                document requires it; remove the `query` parameter `q` in an Overlay, or drop \
                its `required`",
    },
    Namesakes {
        first: "{ name: q, in: cookie, schema: { type: string } }",
        second: "{ name: q, in: query, required: true, schema: { type: string } }",
        refused: Place::Query,
        earlier: Place::Cookie,
        flag: Location::Query,
        words: "listThings: parameter `q` is sharing its name with the `cookie` parameter, and the \
                document requires it; remove the `cookie` parameter `q` in an Overlay, or drop \
                its `required`",
    },
];

/// The parameter that the document lists later is the one that has no flag, so
/// removing the one before it gives it the flag, wherever either of them is, and
/// off the path dropping the `required` of the later one loads as well.
#[test]
fn a_required_namesake_off_the_path_is_given_its_flag_by_removing_the_other() {
    for case in NAMESAKES {
        let paths = format!(
            "  /things:\n    get:\n      operationId: listThings\n      parameters:\n        - \
             {}\n        - {}\n      responses: {{ \"200\": {{ description: OK }} }}\n",
            case.first, case.second
        );

        let error = refused(&paths);

        assert!(
            matches!(&error, LoadError::Parameter { op, name, place, why }
                if op == "listThings" && name == "q" && *place == case.refused
                    && *why == Unsupported::Shared(case.earlier)),
            "{error:?}"
        );
        assert_eq!(error.to_string(), case.words);
        let removed = Document::load(
            &document(&paths),
            &[&removing("$.paths['/things'].get.parameters[0]")],
        )
        .unwrap_or_else(|error| panic!("the Overlay removes the {}: {error}", case.earlier));
        assert!(
            matches!(
                removed
                    .get("listThings")
                    .and_then(|op| op.param("q"))
                    .map(typed_openapi::Param::shape),
                Some(Shape::Flag { location, .. }) if *location == case.flag
            ),
            "{}",
            case.earlier
        );
        Document::load(
            &document(&paths),
            &[&updating(
                "$.paths['/things'].get.parameters[1]",
                "{ required: false }",
            )],
        )
        .unwrap_or_else(|error| panic!("the Overlay drops the `required`: {error}"));
    }
}

/// The template names a path parameter, so the words offer no removal of one:
/// the document would then be refused for the variable. A parameter in the query
/// and one in a header are told the same.
#[test]
fn a_required_namesake_of_a_path_parameter_drops_its_required() {
    for (spelled, in_place) in [("query", Place::Query), ("header", Place::Header)] {
        let paths = format!(
            "  /logs/{{ref}}:
    post:
      operationId: appendLogEntry
      parameters:
        - {{ name: ref, in: path, required: true, schema: {{ type: string }} }}
        - {{ name: ref, in: {spelled}, required: true, schema: {{ type: string }} }}
      responses: {{ \"201\": {{ description: OK }} }}
"
        );

        let error = refused(&paths);

        assert!(
            matches!(&error, LoadError::Parameter { op, name, place, why: Unsupported::Shared(Place::Path) }
                if op == "appendLogEntry" && name == "ref" && *place == in_place),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            "appendLogEntry: parameter `ref` is sharing its name with the `path` parameter, and \
             the document requires it; drop its `required` in an Overlay"
        );
        let mended = Document::load(
            &document(&paths),
            &[&updating(
                "$.paths['/logs/{ref}'].post.parameters[1]",
                "{ required: false }",
            )],
        )
        .expect("the Overlay drops the `required`");
        assert!(mended.get("appendLogEntry").is_some());
        let unbound = Document::load(
            &document(&paths),
            &[&removing("$.paths['/logs/{ref}'].post.parameters[0]")],
        )
        .expect_err("the template needs the path parameter");
        assert!(
            matches!(unbound, LoadError::UnboundVariable { .. }),
            "{unbound:?}"
        );
    }
}

/// Two path entries of one name: the second shares its name with the first, a
/// path parameter has no `required` to drop, and removing the first loads.
#[test]
fn a_second_path_entry_of_one_name_is_given_its_flag_by_removing_the_first() {
    const PATHS: &str = "  /things/{id}:
    get:
      operationId: getThing
      parameters:
        - { name: id, in: path, required: true, schema: { type: string } }
        - { name: id, in: path, required: true, description: \"the second\", schema: { type: string } }
      responses: { \"200\": { description: OK } }
";

    let error = refused(PATHS);

    assert!(
        matches!(&error, LoadError::Parameter { op, name, place: Place::Path, why: Unsupported::Shared(Place::Path) }
            if op == "getThing" && name == "id"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "getThing: parameter `id` is sharing its name with the `path` parameter, and the \
         document requires it; remove the `path` parameter `id` in an Overlay"
    );
    let mended = Document::load(
        &document(PATHS),
        &[&removing("$.paths['/things/{id}'].get.parameters[0]")],
    )
    .expect("the Overlay removes the first entry");
    assert_eq!(names(&mended, "getThing"), ["id"]);
}

const PATH_LEVEL_QUERY: &str = "  /logs/{ref}:
    parameters:
      - { name: ref, in: query, schema: { type: string } }
    post:
      operationId: appendLogEntry
      parameters:
        - { name: ref, in: path, required: true, schema: { type: string } }
      responses: { \"201\": { description: OK } }
";

/// A path parameter has no `required` to drop, so the only repair that loads is
/// the removal of the parameter it shares the name with.
#[test]
fn a_path_parameter_that_shares_its_name_is_given_its_flag_by_removing_the_other() {
    let error = refused(PATH_LEVEL_QUERY);

    assert!(
        matches!(&error, LoadError::Parameter { op, name, place: Place::Path, why: Unsupported::Shared(Place::Query) }
            if op == "appendLogEntry" && name == "ref"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "appendLogEntry: parameter `ref` is sharing its name with the `query` parameter, and the \
         document requires it; remove the `query` parameter `ref` in an Overlay"
    );
    let mended = Document::load(
        &document(PATH_LEVEL_QUERY),
        &[&removing("$.paths['/logs/{ref}'].parameters[0]")],
    )
    .expect("the Overlay removes the query parameter");
    assert!(matches!(
        mended
            .get("appendLogEntry")
            .and_then(|op| op.param("ref"))
            .map(typed_openapi::Param::shape),
        Some(Shape::Flag {
            location: Location::Path,
            ..
        })
    ));
    let optional = Document::load(
        &document(PATH_LEVEL_QUERY),
        &[&updating(
            "$.paths['/logs/{ref}'].post.parameters[0]",
            "{ required: false }",
        )],
    )
    .expect_err("a path parameter that is not required is refused");
    assert!(
        matches!(optional, LoadError::OptionalPathParameter { .. }),
        "{optional:?}"
    );
}

/// "Later" reads the list the reduction builds: the path item's entries in its
/// order, the operation's entry in the slot of the one it replaces, and then the
/// operation's others. So the operation's override of the path item's query `ref`
/// comes before the operation's path `ref` that it lists first, and the path
/// `ref` is the later one.
#[test]
fn an_override_that_takes_the_slot_of_a_path_level_entry_comes_before_the_entries_listed_after_it()
{
    const PATHS: &str = "  /logs/{ref}:
    parameters:
      - { name: ref, in: query, schema: { type: string } }
    post:
      operationId: appendLogEntry
      parameters:
        - { name: ref, in: path, required: true, schema: { type: string } }
        - { name: ref, in: query, description: \"the override\", schema: { type: string } }
      responses: { \"201\": { description: OK } }
";

    let error = refused(PATHS);

    assert!(
        matches!(
            &error,
            LoadError::Parameter {
                place: Place::Path,
                why: Unsupported::Shared(Place::Query),
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "appendLogEntry: parameter `ref` is sharing its name with the `query` parameter, and the \
         document requires it; remove the `query` parameter `ref` in an Overlay"
    );
    let mended = Document::load(
        &document(PATHS),
        &[&removing("$.paths['/logs/{ref}'].parameters[0]")],
    )
    .expect("the Overlay removes the path-level entry");
    let op = mended.get("appendLogEntry").expect("the operation");
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
        &Shape::Unreachable(Unsupported::Shared(Place::Path))
    );
}

// ------------------------------------------------------------------ F-11

/// `p03`. A variable that no path parameter answers is refused, so `{id}` is
/// never in a request.
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
/// The path parameter that the words ask for shares its name with the query one,
/// and that refusal names the repair that finishes the job.
#[test]
fn a_template_variable_is_not_answered_by_a_query_parameter() {
    const PATHS: &str = "  /vouchers/{id}:
    get:
      operationId: getVoucher
      parameters:
        - { name: id, in: query, required: true, schema: { type: string } }
      responses: { \"200\": { description: OK } }
";
    let adding = updating(
        "$.paths['/vouchers/{id}'].get.parameters",
        "{ name: id, in: path, required: true, schema: { type: string } }",
    );

    let error = refused(PATHS);

    assert!(
        matches!(&error, LoadError::UnboundVariable { op, name, .. } if op == "getVoucher" && name == "id"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "getVoucher: `/vouchers/{id}` has the variable `{id}`, and the operation declares no \
         path parameter of that name; add one in an Overlay"
    );
    let added = Document::load(&document(PATHS), &[&adding]).expect_err("the namesake is first");
    assert!(
        matches!(
            &added,
            LoadError::Parameter {
                place: Place::Path,
                why: Unsupported::Shared(Place::Query),
                ..
            }
        ),
        "{added:?}"
    );
    assert!(
        added
            .to_string()
            .ends_with("remove the `query` parameter `id` in an Overlay"),
        "{added}"
    );
    let finished = Document::load(
        &document(PATHS),
        &[
            &adding,
            &removing("$.paths['/vouchers/{id}'].get.parameters[0]"),
        ],
    )
    .expect("the Overlays add the path parameter and remove the query one");
    assert_eq!(names(&finished, "getVoucher"), ["id"]);
}

/// `p04`. A path parameter that no variable names is refused, so no required
/// flag has its value thrown away.
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

/// `p05`. A path parameter that is not `required` is refused, so no flag can be
/// left out and leave `{id}` in the request.
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

/// `ctype.yaml` and `main.yaml`. The request carries the one `content-type` of its
/// body, and no header of the three comes from a parameter of the document.
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

/// The `Authorization` header never becomes a flag, and neither do the two others.
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

/// The `/upload` path item of `p11d`, with the `servers` lists the case puts on
/// the path item and on its `post`.
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
    assert_eq!(
        error.to_string(),
        "upload: `servers` stands on `$.paths['/upload'].servers`, and this crate sends every \
         request to the first server of the document; remove it in an Overlay"
    );
    let mended = Document::load(
        &document(&paths),
        &[&removing("$.paths['/upload'].servers")],
    )
    .expect("the Overlay removes them");
    assert!(mended.get("upload").is_some());
}

/// The probe sets both, and the refusal is of the path item, which the reduction
/// reads first. Its repair leaves the list of the operation, which is refused in
/// turn with the Overlay that removes it.
#[test]
fn servers_on_both_are_refused_at_the_path_item() {
    let paths = upload(&format!("    {SERVERS}"), &format!("      {SERVERS}"));

    let error = refused(&paths);

    assert!(
        matches!(&error, LoadError::Servers { at, .. } if at == "$.paths['/upload'].servers"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "upload: `servers` stands on `$.paths['/upload'].servers`, and this crate sends every \
         request to the first server of the document; remove it in an Overlay"
    );
    let item = removing("$.paths['/upload'].servers");
    let left = Document::load(&document(&paths), &[&item]).expect_err("the operation's remains");
    assert!(
        matches!(&left, LoadError::Servers { at, .. } if at == "$.paths['/upload'].post.servers"),
        "{left:?}"
    );
    let mended = Document::load(
        &document(&paths),
        &[&item, &removing("$.paths['/upload'].post.servers")],
    )
    .expect("the Overlays remove both");
    assert!(mended.get("upload").is_some());
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
