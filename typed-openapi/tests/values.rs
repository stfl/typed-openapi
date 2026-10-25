//! A value the operator types is refused before a request exists, and is never
//! sent changed.
//!
//! The command line and the typed call refuse the same values, because both
//! reach them in `Invocation::new`, and the number that was checked is the number
//! that goes on the wire. Each case is the input of a probe of SEV-249 (`A2/wire`
//! and `A3b/scalars`) or of a review of the slice. A case that refuses a value
//! asserts the variant beside the words, and the words never print the value that
//! was refused where it may be a credential.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::error::Error as _;

use clap::error::ErrorKind;
use clap::{Arg, ArgMatches, Command, value_parser};
use http::Uri;
use typed_openapi::client::{Client, Error as ClientError};
use typed_openapi::scalar::ScalarError;
use typed_openapi::tree::{self, ArgError, Asked, DispatchError};
use typed_openapi::{
    Answers, BaseUrlError, Document, Invocation, LoadError, NoContent, Plan, PlanError, Recorder,
    RequestError, ValueError, Values, render,
};

/// A document with one server and whatever the case puts under `paths`.
fn document(url: &str, paths: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: '{url}' }}]\n\
         paths:\n{paths}"
    )
}

const SERVER: &str = "http://api.example.test/base/";

/// `main.yaml` and `hlist.yaml` of `A2/wire`, and `numbody.yaml` with a number in
/// the query as well: a free-text path parameter on a read and on a write, a
/// header and a header list, a query list, and every kind of number.
const PATHS: &str = "  /things/{name}:
    get:
      operationId: getThing
      parameters:
        - { name: name, in: path, required: true, schema: { type: string } }
        - { name: q, in: query, schema: { type: string } }
        - { name: n, in: query, schema: { type: integer } }
        - { name: sum, in: query, schema: { type: number } }
        - { name: X-Trace, in: header, schema: { type: string } }
        - { name: X-Count, in: header, schema: { type: integer } }
        - { name: X-Tags, in: header, schema: { type: array, items: { type: string } } }
        - { name: tags, in: query, explode: false, schema: { type: array, items: { type: string } } }
        - { name: sort, in: query, schema: { type: string, enum: ['-name', 'name'] } }
      responses: { \"200\": { description: ok } }
    delete:
      operationId: deleteThing
      parameters:
        - { name: name, in: path, required: true, schema: { type: string } }
      responses: { \"204\": { description: ok } }
  /items/{id}:
    get:
      operationId: getItem
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses: { \"200\": { description: ok } }
  /lists/{ids}:
    get:
      operationId: getLists
      parameters:
        - { name: ids, in: path, required: true, schema: { type: array, items: { type: string } } }
      responses: { \"200\": { description: ok } }
  /n:
    post:
      operationId: num
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                sum: { type: number }
                n: { type: integer }
                flag: { type: boolean }
                s: { type: string }
      responses: { \"200\": { description: ok } }
";

fn doc() -> Document {
    Document::load(&document(SERVER, PATHS), &[]).expect("the document loads")
}

/// The refusal of an invocation, from values the caller built.
fn refused(doc: &Document, id: &str, values: Values) -> ValueError {
    let op = doc.get(id).expect("the operation");
    Invocation::new(op, values).expect_err("the values are refused")
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

fn root(doc: &Document) -> Command {
    Command::new("p")
        .subcommand_required(true)
        .subcommands(tree::commands(doc))
}

/// What clap makes of an operator's command line.
fn line(doc: &Document, args: &[&str]) -> Result<ArgMatches, clap::Error> {
    root(doc).try_get_matches_from(std::iter::once("p").chain(args.iter().copied()))
}

/// Everything an operator's command line goes through before a request is sent:
/// clap, the reading of the flags, `Invocation::new` and the gate.
fn planned(doc: &Document, args: &[&str]) -> Result<Plan, DispatchError> {
    let matches = line(doc, args).expect("clap accepts the line");
    match tree::select(doc, &matches)? {
        Asked::Run(selection) => selection.plan(doc.base()),
        Asked::Template(_) => panic!("this command line runs the operation"),
    }
}

/// The request an operator's command line makes.
fn sent(doc: &Document, args: &[&str]) -> http::Request<Vec<u8>> {
    planned(doc, args)
        .expect("the command line is accepted")
        .request()
        .clone()
}

/// Why an operator's command line made no request.
fn stopped(doc: &Document, args: &[&str]) -> DispatchError {
    planned(doc, args).expect_err("the command line is refused")
}

/// Every message down the chain of `error`, from the head.
fn chain(error: &dyn std::error::Error) -> String {
    std::iter::successors(Some(error), |cause| (*cause).source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

// ------------------------------------------------------------------- F-5

/// `A2/wire` `things get --name '..'` is the input. Unrefused, it builds
/// `GET /base/things/..`, which a server or a proxy reads as `/base/`.
#[test]
fn a_path_value_that_is_no_segment_is_refused_and_names_the_parameter() {
    let doc = doc();
    for id in ["getThing", "deleteThing"] {
        for value in ["", ".", ".."] {
            let error = refused(&doc, id, Values::new().param("name", value));
            assert!(
                matches!(&error, ValueError::PathSegment { op, name, value: given }
                    if op == id && name == "name" && given == value),
                "{id} `{value}`: {error:?}"
            );
        }
    }
    for (value, shown) in [("..", "`..`"), (".", "`.`"), ("", "empty")] {
        assert_eq!(
            refused(&doc, "deleteThing", Values::new().param("name", value)).to_string(),
            format!(
                "deleteThing: `name` is {shown}, which cannot be a path segment: a server reads it \
                 as a step along the path and not as a name, so the request would reach another \
                 resource"
            )
        );
    }
}

/// The command line reaches `Invocation::new` and is refused by it, so a dry run
/// is as much a refusal as a confirmed run, and nothing is sent.
#[test]
fn the_command_line_refuses_the_same_path_values() {
    let doc = doc();
    for value in ["..", ".", ""] {
        let error = stopped(&doc, &["things", "delete", "--name", value, "--commit"]);
        assert!(
            matches!(&error, DispatchError::Plan(PlanError::Value(
                ValueError::PathSegment { op, name, value: given }
            )) if op == "deleteThing" && name == "name" && given == value),
            "`{value}`: {error:?}"
        );
        assert!(
            error.to_string().starts_with("deleteThing: `name` is "),
            "`{value}`: {error}"
        );
    }
    let matches = line(&doc, &["things", "delete", "--name", "..", "--commit"])
        .expect("clap accepts the line");
    let client = Recorder::new();
    let Asked::Run(selection) = tree::select(&doc, &matches).expect("an operation") else {
        panic!("this command line runs the operation");
    };
    assert!(selection.send(&client, doc.base()).is_err());
    assert!(client.take().is_empty(), "nothing reached the transport");
}

#[test]
fn a_value_that_only_looks_like_a_dot_segment_is_a_value() {
    let doc = doc();
    for (value, path) in [
        ("...", "/base/things/..."),
        (".a", "/base/things/.a"),
        ("a.", "/base/things/a."),
        ("..a", "/base/things/..a"),
        ("a/..", "/base/things/a%2F.."),
        ("%2e%2e", "/base/things/%252e%252e"),
        (" ", "/base/things/%20"),
    ] {
        let sent = uri(&doc, "getThing", Values::new().param("name", value));
        assert_eq!(sent, format!("http://api.example.test{path}"), "`{value}`");
    }
}

/// The segment is what a list makes of its items, joined by commas, so one item
/// of two is not a segment of its own.
#[test]
fn a_list_is_judged_as_the_segment_it_makes() {
    let doc = doc();
    let error = refused(&doc, "getLists", Values::new().each("ids", [".."]));
    assert!(
        matches!(&error, ValueError::PathSegment { op, name, value }
            if op == "getLists" && name == "ids" && value == ".."),
        "{error:?}"
    );
    assert!(error.to_string().contains("`ids` is `..`"), "{error}");
    assert_eq!(
        uri(&doc, "getLists", Values::new().each("ids", ["..", "a"])),
        "http://api.example.test/base/lists/..,a"
    );
    assert_eq!(
        uri(&doc, "getLists", Values::new().each("ids", [".", "."])),
        "http://api.example.test/base/lists/.,."
    );
}

// ------------------------------------------------------------------ F-28

/// `A2/wire` `things get --name a --x-trace "v<CR><LF>Injected: 1"` is the input.
/// The `http` crate refuses it late, as "failed to parse header value" with no
/// name, so `Invocation::new` refuses it by name, before a request exists.
#[test]
fn a_header_value_the_http_crate_refuses_is_refused_by_name() {
    let doc = doc();
    for value in [
        "v\r\nInjected: 1",
        "v\nx",
        "v\rx",
        "\0",
        "a\u{7f}b",
        "\u{1}",
    ] {
        let error = refused(
            &doc,
            "getThing",
            Values::new().param("name", "a").param("X-Trace", value),
        );
        assert!(
            matches!(&error, ValueError::HeaderValue { op, name }
                if op == "getThing" && name == "X-Trace"),
            "{value:?}: {error:?}"
        );
        assert!(!error.to_string().contains("Injected"), "{error}");
    }
    let error = refused(
        &doc,
        "getThing",
        Values::new()
            .param("name", "a")
            .param("X-Trace", "v\r\nInjected: 1"),
    );
    assert_eq!(
        error.to_string(),
        "getThing: `X-Trace` was given a value that no header can carry: a header value holds \
         no control character but a tab"
    );
}

#[test]
fn the_command_line_refuses_the_header_value_before_a_request_exists() {
    let doc = doc();
    let error = stopped(
        &doc,
        &[
            "things",
            "get",
            "--name",
            "a",
            "--x-trace",
            "v\r\nInjected: 1",
        ],
    );
    assert!(
        matches!(&error, DispatchError::Plan(PlanError::Value(
            ValueError::HeaderValue { op, name }
        )) if op == "getThing" && name == "X-Trace"),
        "{error:?}"
    );
    let shown = error.to_string();
    assert!(
        shown.starts_with("getThing: `X-Trace` was given a value"),
        "{shown}"
    );
    assert!(!shown.contains("Injected"), "{shown}");
}

/// The refusal is `HeaderValue`'s, so what it accepts is what it accepts: a tab,
/// text past ASCII, and a comma in a header that is not a list.
#[test]
fn a_header_value_the_http_crate_accepts_is_sent() {
    let doc = doc();
    let op = doc.get("getThing").expect("the operation");
    for value in ["a\tb", "Grüß", "a, b", "", "x=y; z"] {
        let request = Invocation::new(op, Values::new().param("name", "a").param("X-Trace", value))
            .expect("a value the http crate accepts")
            .request(doc.base())
            .expect("a request");
        assert_eq!(
            request.headers()["x-trace"].as_bytes(),
            value.as_bytes(),
            "{value:?}"
        );
    }
}

/// `A2/wire` `h list --x-tags 'a,b' --x-tags c` is the input. Sent as
/// `x-tags: a,b,c`, a receiver reads three items where the caller gave two. A
/// query list encodes the comma, so there it is a character of the item.
#[test]
fn a_comma_inside_one_item_of_a_header_list_is_refused() {
    let doc = doc();
    let error = refused(
        &doc,
        "getThing",
        Values::new()
            .param("name", "a")
            .each("X-Tags", ["a,b", "c"]),
    );
    assert!(
        matches!(&error, ValueError::HeaderItemComma { op, name }
            if op == "getThing" && name == "X-Tags"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "getThing: `X-Tags` was given an item with a comma in it, and a header list is split \
         at its commas"
    );
    let error = stopped(
        &doc,
        &[
            "things", "get", "--name", "a", "--x-tags", "a,b", "--x-tags", "c",
        ],
    );
    assert!(
        matches!(&error, DispatchError::Plan(PlanError::Value(
            ValueError::HeaderItemComma { name, .. }
        )) if name == "X-Tags"),
        "{error:?}"
    );

    let request = sent(
        &doc,
        &[
            "things", "get", "--name", "a", "--x-tags", "a", "--x-tags", "b", "--tags", "a,b",
            "--tags", "c",
        ],
    );
    assert_eq!(request.headers()["x-tags"], "a,b");
    assert_eq!(request.uri().query(), Some("tags=a%2Cb,c"));
}

#[test]
fn a_comma_in_a_header_that_is_not_a_list_is_a_character_of_the_value() {
    let doc = doc();
    let request = sent(&doc, &["things", "get", "--name", "a", "--x-trace", "a,b"]);
    assert_eq!(request.headers()["x-trace"], "a,b");
}

/// RFC 9110 section 5.6.1.2: a recipient ignores an empty list element. `h list
/// --x-tags '' --x-tags c` would be sent as `x-tags: ,c`, which a receiver reads
/// as the one item `c`, and `--x-tags ''` alone as a header with no item.
#[test]
fn an_empty_item_of_a_header_list_is_refused() {
    let doc = doc();
    for items in [vec![""], vec!["", "c"], vec!["a", ""], vec!["a", "", "c"]] {
        let error = refused(
            &doc,
            "getThing",
            Values::new()
                .param("name", "a")
                .each("X-Tags", items.clone()),
        );
        assert!(
            matches!(&error, ValueError::HeaderItemEmpty { op, name }
                if op == "getThing" && name == "X-Tags"),
            "{items:?}: {error:?}"
        );
    }
    assert_eq!(
        refused(
            &doc,
            "getThing",
            Values::new().param("name", "a").each("X-Tags", ["", "c"])
        )
        .to_string(),
        "getThing: `X-Tags` was given an empty item, and a receiver ignores an empty item of a \
         header list"
    );
    for args in [
        ["--x-tags", "", "--x-tags", "c"],
        ["--x-tags", "a", "--x-tags", ""],
    ] {
        let line: Vec<&str> = ["things", "get", "--name", "a"]
            .into_iter()
            .chain(args)
            .collect();
        let error = stopped(&doc, &line);
        assert!(
            matches!(&error, DispatchError::Plan(PlanError::Value(
                ValueError::HeaderItemEmpty { op, name }
            )) if op == "getThing" && name == "X-Tags"),
            "{args:?}: {error:?}"
        );
    }
}

/// RFC 9110 sections 5.5 and 5.6.1: a field value does not start or end with
/// whitespace, and whitespace around a comma is not part of an item. A receiver
/// strips it, so it reads another value than the caller gave.
#[test]
fn a_space_or_a_tab_at_the_edge_of_a_header_value_or_item_is_refused() {
    let doc = doc();
    for value in [" a", "a ", "\ta", "a\t", " ", "\t", "  a  "] {
        let error = refused(
            &doc,
            "getThing",
            Values::new().param("name", "a").param("X-Trace", value),
        );
        assert!(
            matches!(&error, ValueError::HeaderWhitespace { op, name }
                if op == "getThing" && name == "X-Trace"),
            "{value:?}: {error:?}"
        );
        for items in [["a", value], [value, "b"]] {
            let error = refused(
                &doc,
                "getThing",
                Values::new().param("name", "a").each("X-Tags", items),
            );
            assert!(
                matches!(&error, ValueError::HeaderWhitespace { name, .. } if name == "X-Tags"),
                "{items:?}: {error:?}"
            );
        }
    }
    assert_eq!(
        refused(
            &doc,
            "getThing",
            Values::new().param("name", "a").param("X-Trace", " s3cret")
        )
        .to_string(),
        "getThing: `X-Trace` was given a value that starts or ends with a space or a tab, and a \
         receiver does not read it as part of the value"
    );
    for args in [
        ["--x-trace", " s3cret"],
        ["--x-trace", "s3cret\t"],
        ["--x-tags", " s3cret"],
    ] {
        let line: Vec<&str> = ["things", "get", "--name", "a"]
            .into_iter()
            .chain(args)
            .collect();
        let error = stopped(&doc, &line);
        assert!(
            matches!(&error, DispatchError::Plan(PlanError::Value(
                ValueError::HeaderWhitespace { op, name }
            )) if op == "getThing" && (name == "X-Trace" || name == "X-Tags")),
            "{args:?}: {error:?}"
        );
        assert!(!chain(&error).contains("s3cret"), "{error}");
    }
}

/// Whitespace inside a value, an empty value that is no list, and text past
/// ASCII reach the receiver as the same bytes, so they are sent.
#[test]
fn a_header_value_that_a_receiver_reads_as_it_was_given_is_sent() {
    let doc = doc();
    let op = doc.get("getThing").expect("the operation");
    for value in ["a b", "a\tb", "a  b", "", "\u{a0}a\u{a0}", "a;b=c"] {
        let request = Invocation::new(op, Values::new().param("name", "a").param("X-Trace", value))
            .expect("a value a receiver reads as given")
            .request(doc.base())
            .expect("a request");
        assert_eq!(
            request.headers()["x-trace"].as_bytes(),
            value.as_bytes(),
            "{value:?}"
        );
    }
    let request = Invocation::new(
        op,
        Values::new()
            .param("name", "a")
            .each("X-Tags", ["a b", "c\td"]),
    )
    .expect("items with whitespace inside")
    .request(doc.base())
    .expect("a request");
    assert_eq!(request.headers()["x-tags"], "a b,c\td");
    let request = sent(
        &doc,
        &[
            "things",
            "get",
            "--name",
            "a",
            "--x-trace",
            "",
            "--x-tags",
            "a b",
        ],
    );
    assert_eq!(request.headers()["x-trace"], "");
    assert_eq!(request.headers()["x-tags"], "a b");
}

/// `A2/wire` `badheader list --x-bad v` is the input. A name that no request
/// could carry refuses the document, and the `http` crate does not have to refuse
/// every request that is built from it.
#[test]
fn a_header_parameter_that_is_no_header_name_is_refused_at_load() {
    let bad = "  /bad:
    get:
      operationId: badHeader
      parameters:
        - { name: 'X Bad', in: header, schema: { type: string } }
      responses: { \"200\": { description: ok } }
";
    let error = Document::load(&document(SERVER, bad), &[]).expect_err("the document is refused");
    assert!(
        matches!(&error, LoadError::HeaderName { op, name }
            if op == "badHeader" && name == "X Bad"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "badHeader: the header parameter `X Bad` is not a header name; correct or remove it in \
         an Overlay"
    );

    // The words are true: each repair loads the document.
    let overlay = |action: &str| {
        format!(
            "overlay: 1.1.0\ninfo: {{ title: t, version: \"1\" }}\nactions:\n  - target: \
             \"$.paths['/bad'].get.parameters[0]\"\n    {action}\n"
        )
    };
    for action in ["remove: true", "update: { name: X-Bad }"] {
        Document::load(&document(SERVER, bad), &[&overlay(action)])
            .unwrap_or_else(|error| panic!("`{action}` leaves a document that loads: {error}"));
    }

    // Only a header has a header name.
    for place in ["query", "path"] {
        let elsewhere = bad
            .replace("in: header", &format!("in: {place}"))
            .replace("/bad:", "/bad/{x}:")
            .replace(
                "{ name: 'X Bad'",
                "{ name: 'x', required: true }, { name: 'X Bad'",
            );
        let loaded = Document::load(&document(SERVER, &elsewhere), &[]);
        assert!(
            !matches!(loaded, Err(LoadError::HeaderName { .. })),
            "`in: {place}`: {loaded:?}"
        );
    }
}

// ------------------------------------------------------------ F-31, F-76

fn base(text: &str) -> Uri {
    text.parse().expect("a URI")
}

/// `A2/wire` `toy --base-url 'http://alice:s3cret@127.0.0.1:18473/' raw vouchers
/// render --id 5` is the input. Unrefused, a dry run prints
/// `host: alice:s3cret@127.0.0.1:18473` and a confirmed run sends the credential
/// as a `Basic` header.
#[test]
fn a_base_url_with_userinfo_is_refused_and_the_refusal_does_not_print_it() {
    let doc = doc();
    let op = doc.get("getThing").expect("the operation");
    let invocation = Invocation::new(op, Values::new().param("name", "a")).expect("values");

    for text in [
        "http://alice:s3cret@127.0.0.1:18473/",
        // The `http` crate reads this as a path with no authority. The load reads
        // it as an authority.
        "//alice:s3cret@127.0.0.1:18473/",
    ] {
        let error = invocation
            .request(&base(text))
            .expect_err("userinfo is refused");

        assert!(
            matches!(&error, RequestError::Base(BaseUrlError::Userinfo)),
            "{text}: {error:?}"
        );
        let shown = chain(&error);
        assert!(shown.contains("userinfo"), "{shown}");
        assert!(
            !shown.contains("alice") && !shown.contains("s3cret"),
            "{shown}"
        );
    }
    // The empty user and the empty password are userinfo too.
    for text in [
        "http://@host/",
        "http://:@host/",
        "http://u@host/",
        "//@host/",
        "//u@host/api",
    ] {
        assert!(
            matches!(
                invocation.request(&base(text)),
                Err(RequestError::Base(BaseUrlError::Userinfo))
            ),
            "{text}"
        );
    }
}

/// `A2/wire` `toy --base-url 'http://127.0.0.1:18471/api?key=secret' raw vouchers
/// list --type ''` is the input. Unrefused, the listener receives
/// `GET /api/vouchers?type=`, and the query is gone.
#[test]
fn a_base_url_with_a_query_is_refused_and_the_refusal_does_not_print_it() {
    let doc = doc();
    let op = doc.get("getThing").expect("the operation");
    let invocation = Invocation::new(op, Values::new().param("name", "a")).expect("values");

    for text in [
        "http://127.0.0.1:18471/api?key=secret",
        "http://127.0.0.1:18471/api?",
        "http://127.0.0.1:18471?key=secret",
    ] {
        let error = invocation
            .request(&base(text))
            .expect_err("a query is refused");
        assert!(
            matches!(&error, RequestError::Base(BaseUrlError::Query)),
            "{text}: {error:?}"
        );
        let shown = chain(&error);
        assert!(shown.contains("query"), "{shown}");
        assert!(
            !shown.contains("key") && !shown.contains("secret"),
            "{shown}"
        );
    }
    assert_eq!(
        invocation
            .request(&base("http://127.0.0.1:18471/api/"))
            .expect("a base with a path")
            .uri()
            .to_string(),
        "http://127.0.0.1:18471/api/things/a"
    );
}

/// A base with no authority, or with an `@` that is no userinfo, is a base.
#[test]
fn a_base_whose_at_sign_is_no_userinfo_is_a_base() {
    let doc = doc();
    let op = doc.get("getThing").expect("the operation");
    let invocation = Invocation::new(op, Values::new().param("name", "a")).expect("values");

    for (text, sent) in [
        ("//host/api", "//host/api/things/a"),
        ("/api", "/api/things/a"),
        ("/api/u@h", "/api/u@h/things/a"),
        ("http://host//u@h/", "http://host//u@h/things/a"),
        (
            "http://host/api/http://u@h/",
            "http://host/api/http://u@h/things/a",
        ),
    ] {
        let request = invocation
            .request(&base(text))
            .unwrap_or_else(|error| panic!("{text}: {error:?}"));
        assert_eq!(request.uri().to_string(), sent, "{text}");
    }
}

/// Every road a base URL takes to a request ends in the same refusal: the
/// gate's, a typed call's and the command line's. The `http` crate reads the
/// second base as a path, and the roads refuse it as they refuse the first.
#[test]
fn every_road_refuses_the_base_the_caller_names() {
    for text in [
        "http://alice:s3cret@127.0.0.1:18473/",
        "//alice:s3cret@127.0.0.1:18473/",
    ] {
        refused_on_every_road(&base(text));
    }
}

fn refused_on_every_road(secret: &Uri) {
    let doc = doc();
    let op = doc.get("deleteThing").expect("the operation");

    let built = Plan::build(
        op,
        secret,
        Values::new().param("name", "a"),
        &Answers::new(),
    )
    .expect_err("a dry run builds no request either");
    assert!(
        matches!(
            &built,
            PlanError::Request(RequestError::Base(BaseUrlError::Userinfo))
        ),
        "{built:?}"
    );

    let matches = line(&doc, &["things", "delete", "--name", "a"]).expect("clap accepts the line");
    let Asked::Run(selection) = tree::select(&doc, &matches).expect("an operation") else {
        panic!("this command line runs the operation");
    };
    let planned = selection.plan(secret).expect_err("refused");
    assert!(
        matches!(
            &planned,
            DispatchError::Plan(PlanError::Request(RequestError::Base(
                BaseUrlError::Userinfo
            )))
        ),
        "{planned:?}"
    );

    let client = Client::over(doc.clone())
        .expect("a client")
        .with_base(secret.clone());
    let call = client
        .call::<NoContent>(op, Values::new().param("name", "a"))
        .expect("the values are fine");
    let typed = call.request().expect_err("refused");
    assert!(
        matches!(
            &typed,
            ClientError::Request(RequestError::Base(BaseUrlError::Userinfo))
        ),
        "{typed:?}"
    );
    for error in [&built as &dyn std::error::Error, &planned, &typed] {
        let shown = chain(error);
        assert!(
            !shown.contains("alice") && !shown.contains("s3cret"),
            "{shown}"
        );
    }
}

/// A request that someone else built, or a `Plan` an adopter made by hand, still
/// shows no credential: the host line is the host.
#[test]
fn a_dry_run_never_prints_userinfo() {
    let request = http::Request::builder()
        .method("DELETE")
        .uri("http://alice:s3cret@127.0.0.1:18473/base/things/a")
        .body(Vec::new())
        .expect("a request");

    let shown = render(&request);

    assert_eq!(
        shown,
        "DELETE /base/things/a HTTP/1.1\nhost: 127.0.0.1:18473\n"
    );
    let v6 = http::Request::builder()
        .uri("http://u:p@[::1]:8080/x")
        .body(Vec::new())
        .expect("a request");
    assert_eq!(render(&v6), "GET /x HTTP/1.1\nhost: [::1]:8080\n");
    let plain = http::Request::builder()
        .uri("http://127.0.0.1:18473/x")
        .body(Vec::new())
        .expect("a request");
    assert_eq!(render(&plain), "GET /x HTTP/1.1\nhost: 127.0.0.1:18473\n");
    // The `http` crate reads this as a path, and the credential stands in it.
    let path = http::Request::builder()
        .method("DELETE")
        .uri("//alice:s3cret@127.0.0.1:18473/base/things/a")
        .body(Vec::new())
        .expect("a request");
    assert_eq!(
        render(&path),
        "DELETE //127.0.0.1:18473/base/things/a HTTP/1.1\n"
    );
}

/// The request line of a dry run is the path that is sent. When the URI has a
/// host, a `//` at the start of its path is part of the path and not an
/// authority, so an `@` after it stays in the line: `http://host//u@h/` is a
/// base, and so is a document path that starts with `//u@h/`.
#[test]
fn a_dry_run_shows_the_path_that_is_sent() {
    let doc = doc();
    let op = doc.get("getThing").expect("the operation");
    let invocation = Invocation::new(op, Values::new().param("name", "a")).expect("values");

    for (text, shown) in [
        (
            "http://host//u@h/",
            "GET //u@h/things/a HTTP/1.1\nhost: host\n",
        ),
        (
            "http://host:9//u@h/api",
            "GET //u@h/api/things/a HTTP/1.1\nhost: host:9\n",
        ),
    ] {
        let request = invocation
            .request(&base(text))
            .unwrap_or_else(|error| panic!("{text}: {error:?}"));
        assert_eq!(render(&request), shown, "{text}");
    }

    let odd = Document::load(
        &document(
            "http://api.example.test",
            "  //u@h/x:
    get:
      operationId: oddPath
      x-cli-group: odd
      responses: { \"200\": { description: ok } }
",
        ),
        &[],
    )
    .expect("the document loads");
    let request = Invocation::new(odd.get("oddPath").expect("the operation"), Values::new())
        .expect("values")
        .request(odd.base())
        .expect("a request");
    assert_eq!(
        render(&request),
        "GET //u@h/x HTTP/1.1\nhost: api.example.test\n"
    );
}

/// A `Debug` goes to logs, and the base of a client is the caller's. Its query
/// is left out as well, because a key goes there (`?api_key=…`) and a request
/// from such a base is refused.
#[test]
fn the_debug_of_a_client_holds_no_userinfo_and_no_query() {
    let doc = doc();
    for (text, shown) in [
        (
            "http://alice:s3cret@127.0.0.1:18473/api?key=k3y",
            "base: http://127.0.0.1:18473/api",
        ),
        ("http://u:p@[::1]:8080/x", "base: http://[::1]:8080/x"),
        ("http://@host/", "base: http://host/"),
        (
            "//alice:s3cret@127.0.0.1:18473/api?key=k3y",
            "base: //127.0.0.1:18473/api",
        ),
        (
            "http://127.0.0.1:18473/api",
            "base: http://127.0.0.1:18473/api",
        ),
    ] {
        let client = Client::over(doc.clone())
            .expect("a client")
            .with_base(base(text));
        let debug = format!("{client:?}");
        assert!(debug.contains(shown), "{text}: {debug}");
        assert!(
            !debug.contains("alice")
                && !debug.contains("s3cret")
                && !debug.contains("u:p")
                && !debug.contains("k3y"),
            "{text}: {debug}"
        );
    }
}

/// A blob is read with no load, and `Document::from_blob` does not repeat the
/// refusal of a server URL, so a document can hold a base with userinfo and a
/// query. Its `Debug`, and the `Debug` of a client over it, leave both out, and
/// a request from that base is refused all the same. The `http` crate reads a
/// base that starts with `//` as a path, and its userinfo is left out as well.
#[test]
fn the_debug_of_a_document_read_from_a_blob_holds_no_userinfo_and_no_query() {
    for (text, shown) in [
        (
            "http://alice:s3cret@127.0.0.1:18473/api?key=k3y",
            "base: http://127.0.0.1:18473/api,",
        ),
        (
            "//alice:s3cret@127.0.0.1:18473/api?key=k3y",
            "base: //127.0.0.1:18473/api,",
        ),
    ] {
        let mut value = serde_json::to_value(doc()).expect("a document is a value");
        value
            .as_object_mut()
            .expect("a document is an object")
            .insert("base".to_owned(), text.into());
        let edited: Document = serde_json::from_value(value).expect("the edit reads back");
        let document = Document::from_blob(&edited.to_blob().expect("the edit encodes"))
            .expect("a blob is not refused for its base");
        let client = Client::over(document.clone()).expect("a client");

        for (what, debug) in [
            ("document", format!("{document:?}")),
            ("document, pretty", format!("{document:#?}")),
            ("client", format!("{client:?}")),
        ] {
            assert!(debug.contains(shown), "{text}, {what}: {debug}");
            for secret in ["alice", "s3cret", "k3y"] {
                assert!(
                    !debug.contains(secret),
                    "{text}, {what}: `{secret}` in {debug}"
                );
            }
        }
        let op = document.get("getThing").expect("the operation");
        let invocation = Invocation::new(op, Values::new().param("name", "a")).expect("values");
        assert!(
            matches!(
                invocation.request(document.base()),
                Err(RequestError::Base(BaseUrlError::Userinfo))
            ),
            "{text}"
        );
    }
}

/// A document with a `servers` entry the caller could not have sent to without a
/// credential or a query in it is a document refused, whichever of the three it
/// holds, and whichever part of the URL is a variable.
#[test]
fn a_server_with_userinfo_a_query_or_a_fragment_is_refused_at_load() {
    for (url, part) in [
        (
            "http://alice:s3cret@127.0.0.1:18473/api",
            BaseUrlError::Userinfo,
        ),
        ("http://127.0.0.1:18471/api?key=secret", BaseUrlError::Query),
        ("http://127.0.0.1:18471/api?", BaseUrlError::Query),
        ("http://127.0.0.1:18471/api#secret", BaseUrlError::Fragment),
        ("http://127.0.0.1:18471/api#", BaseUrlError::Fragment),
        // The credential that is worst to print comes first.
        (
            "http://alice:s3cret@host/api?key=secret#top",
            BaseUrlError::Userinfo,
        ),
        ("http://host/api?key=secret#top", BaseUrlError::Query),
    ] {
        let error = Document::load(&document(url, PATHS), &[]).expect_err(url);
        assert!(
            matches!(&error, LoadError::ServerBase { source } if *source == part),
            "{url}: {error:?}"
        );
        let shown = chain(&error);
        for secret in ["alice", "s3cret", "key=", "secret", "#top", "127.0.0.1"] {
            assert!(!shown.contains(secret), "{url}: `{secret}` in {shown}");
        }
    }
}

/// `http://alice:s3cret@host/a b` is no URL, and `ServerUrl` prints the URL it
/// refuses, so the userinfo is read from the text first and the refusal is
/// `ServerBase`. No `LoadError` holds the text, in a field or in a message.
///
/// The authority is read after the first `/`, `?` or `#` of the text when that
/// character starts a `//`, and at the start of the text. What stands before the
/// `//` is whatever the document wrote: a variable that `variables` does not
/// declare, a space, a tab or a byte order mark in front, a scheme with a
/// character that no scheme holds or with no colon, or a userinfo that stands
/// before a `://`.
#[test]
fn a_server_url_with_userinfo_that_is_no_url_is_refused_for_the_userinfo() {
    for url in [
        "http://alice:s3cret@host/a b",
        "http://alice:s3cret@host:port/",
        "alice:s3cret@host/a b",
        "//alice:s3cret@host/a b",
        "http://alice:s3cret@{undeclared}/",
        "http://alice:s3cret@host?q=a b",
        "{scheme}://alice:s3cret@host/",
        " http://alice:s3cret@host/",
        "\thttp://alice:s3cret@host/",
        "htt p://alice:s3cret@host/",
        "ht_tp://alice:s3cret@host/",
        "1http://alice:s3cret@host/a b",
        "alice:s3cret@host://x",
        " //alice:s3cret@host/",
        "\t//alice:s3cret@host/",
        "\u{feff}//alice:s3cret@host/",
        "{proto}//alice:s3cret@host/",
        "http//alice:s3cret@host/a b",
        "http: //alice:s3cret@host/a b",
    ] {
        let error = Document::load(&document(url, PATHS), &[]).expect_err(url);
        assert!(
            matches!(
                &error,
                LoadError::ServerBase {
                    source: BaseUrlError::Userinfo
                }
            ),
            "{url}: {error:?}"
        );
        for secret in ["alice", "s3cret"] {
            assert!(!chain(&error).contains(secret), "{url}: {error}");
            assert!(!format!("{error:?}").contains(secret), "{url}: {error:?}");
        }
    }
}

/// An `@` that is not in the authority is no userinfo, and a URL that is no URL
/// and holds none is still `ServerUrl`, which prints it.
#[test]
fn an_at_sign_past_the_authority_is_no_userinfo() {
    for url in [
        "http://host/a b",
        "http://host/a@b c",
        "http://host?u=a@b c",
        // No `@` stands where an authority is read.
        "http:/u@h/a b",
        "http:/\\u@h/a b",
        "http:///u@h/a b",
        // A `/` stands before the `//`, so no authority is read there.
        "{a/b}://u@h/ x",
        "/redirect?to=http://u@h/ x",
        "host/path?u=a://b@c x",
    ] {
        let error = Document::load(&document(url, PATHS), &[]).expect_err(url);
        assert!(
            matches!(&error, LoadError::ServerUrl { url: given, .. } if given == url),
            "{url}: {error:?}"
        );
        assert!(chain(&error).contains(url), "{url}: {error}");
    }
    for url in [
        "http://host/a@b",
        "/redirect?to=http://u@h/",
        "http://host/x#u@h",
        "http://host#u@h",
    ] {
        let loaded = Document::load(&document(url, PATHS), &[]);
        assert!(
            !matches!(
                loaded,
                Err(LoadError::ServerBase {
                    source: BaseUrlError::Userinfo
                })
            ),
            "{url}: {loaded:?}"
        );
    }
}

/// A document with one server whose URL has one variable.
fn with_variable(url: &str, name: &str, default: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers:\n  - url: '{url}'\n    variables:\n      {name}: {{ default: '{default}' }}\n\
         paths: {{}}\n"
    )
}

/// `ServerUrl` prints the URL as the document spells it beside the one with the
/// defaults in it. A default that holds a `/` moves the `@` out of the authority
/// of the filled text and leaves it in the authority of the spelled one, so the
/// spelling is read too, for a URL that does not parse. One that parses is a
/// base, and `http://x/alice:s3cret@host/` holds no userinfo.
#[test]
fn the_spelling_of_a_server_url_is_read_for_userinfo_when_the_url_is_refused() {
    for (url, name, default) in [
        ("{p}://alice:s3cret@host/a b", "p", "a/b"),
        ("http://{h}alice:s3cret@host/a b", "h", "x/"),
    ] {
        let error = Document::load(&with_variable(url, name, default), &[]).expect_err(url);
        assert!(
            matches!(
                &error,
                LoadError::ServerBase {
                    source: BaseUrlError::Userinfo
                }
            ),
            "{url}: {error:?}"
        );
        for secret in ["alice", "s3cret"] {
            assert!(!chain(&error).contains(secret), "{url}: {error}");
            assert!(!format!("{error:?}").contains(secret), "{url}: {error:?}");
        }
    }
    let loaded = Document::load(
        &with_variable("http://{h}alice:s3cret@host/", "h", "x/"),
        &[],
    )
    .expect("the filled text is a URL whose authority is `x`");
    assert_eq!(loaded.base().to_string(), "http://x/alice:s3cret@host/");
}

/// The default of a variable puts userinfo in the URL, and the spelling holds
/// none: the filled text is what a request goes to, and what `ServerUrl` prints
/// when it does not parse.
#[test]
fn a_server_variable_that_makes_the_url_carry_one_is_refused_too() {
    for url in [
        "http://{credentials}host/api",
        "http://{credentials}host/a b",
    ] {
        let document = with_variable(url, "credentials", "alice:s3cret@");
        let error = Document::load(&document, &[]).expect_err("the filled URL holds userinfo");
        assert!(
            matches!(
                &error,
                LoadError::ServerBase {
                    source: BaseUrlError::Userinfo
                }
            ),
            "{url}: {error:?}"
        );
        assert!(
            !chain(&error).contains("s3cret"),
            "{url}: {}",
            chain(&error)
        );
        assert!(!format!("{error:?}").contains("s3cret"), "{url}: {error:?}");
    }
}

#[test]
fn the_words_of_a_server_refusal_name_the_overlay_and_the_repair_loads() {
    let url = "http://alice:s3cret@127.0.0.1:18473/api";
    let error = Document::load(&document(url, PATHS), &[]).expect_err("refused");
    assert!(
        matches!(
            &error,
            LoadError::ServerBase {
                source: BaseUrlError::Userinfo
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "`servers[0].url` cannot be the base URL of a request; correct it in an Overlay"
    );
    assert_eq!(
        error.source().map(ToString::to_string).as_deref(),
        Some(
            "the base URL holds userinfo (`user:password@`), which a request would send as a \
             credential and a dry run would print"
        )
    );

    let overlay = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - target: \
                   \"$.servers[0]\"\n    update: { url: 'http://127.0.0.1:18473/api' }\n";
    let doc = Document::load(&document(url, PATHS), &[overlay]).expect("the repair loads");
    assert_eq!(doc.base().to_string(), "http://127.0.0.1:18473/api");
}

/// A server that is a plain base loads: a port, a path, a trailing slash, a base
/// that is relative.
#[test]
fn a_server_that_is_a_plain_base_loads() {
    for url in [
        "http://127.0.0.1:18473",
        "https://api.example.test:8443/v1/",
        "/",
        "/api",
    ] {
        Document::load(&document(url, PATHS), &[]).unwrap_or_else(|error| panic!("{url}: {error}"));
    }
}

// ------------------------------------------------------------------ F-35

/// `A2/wire` `n create --n -5 --sum -1.5` is the input. clap reads `-5` as a flag
/// of its own unless the flag is told what it is for ("unexpected argument '-5'
/// found"), and only `--n=-5` works.
#[test]
fn a_negative_number_is_a_separate_word() {
    let doc = doc();
    let request = sent(
        &doc,
        &["things", "get", "--name", "a", "--n", "-5", "--sum", "-1.5"],
    );
    assert_eq!(request.uri().query(), Some("n=-5&sum=-1.5"));
    let joined = sent(
        &doc,
        &["things", "get", "--name", "a", "--n=-5", "--sum=-1.5"],
    );
    assert_eq!(joined.uri().query(), request.uri().query());

    let body = sent(&doc, &["n", "create", "--n", "-5", "--sum", "-1.5"]);
    assert_eq!(body.body().as_slice(), br#"{"sum":-1.5,"n":-5}"#);
    // clap's own test of a negative number takes no leading point and no sign
    // after the exponent, so these words reach the value parser only because
    // the flag takes any word. The joined and the separate word send the same
    // query.
    for (typed, sent_as) in [("-.5", "-0.5"), ("-1e-3", "-0.001"), ("-1e+3", "-1000")] {
        let separate = sent(&doc, &["things", "get", "--name", "a", "--sum", typed]);
        let joined = sent(
            &doc,
            &["things", "get", "--name", "a", &format!("--sum={typed}")],
        );
        assert_eq!(
            separate.uri().query(),
            Some(&*format!("sum={sent_as}")),
            "{typed}"
        );
        assert_eq!(joined.uri().query(), separate.uri().query(), "{typed}");
    }
    // A path value too.
    let path = sent(&doc, &["items", "get", "--id", "-5"]);
    assert_eq!(path.uri().path(), "/base/items/-5");
}

/// A word that starts with a dash and is no number is the value of the flag
/// before it, and the flag's parser refuses it as a number.
#[test]
fn a_dash_word_is_not_an_integer() {
    let doc = doc();
    for (value, kind) in [
        ("-abc", ErrorKind::ValueValidation),
        ("-5.5", ErrorKind::ValueValidation),
        ("-", ErrorKind::ValueValidation),
        ("--sum", ErrorKind::ValueValidation),
    ] {
        let error = line(&doc, &["things", "get", "--name", "a", "--n", value])
            .expect_err("the value is refused");
        assert_eq!(error.kind(), kind, "`{value}`: {error}");
    }
    let error = line(&doc, &["things", "get", "--name", "a", "--n", "-5.5"])
        .expect_err("the value is refused");
    assert!(error.to_string().contains("is not an integer"), "{error}");
}

#[test]
fn a_text_flag_takes_a_value_that_starts_with_a_dash() {
    let doc = doc();
    let request = sent(&doc, &["things", "get", "--name", "-x", "--q", "-abc"]);
    assert_eq!(request.uri().path(), "/base/things/-x");
    assert_eq!(request.uri().query(), Some("q=-abc"));
    // A word that looks like a flag is a value when it follows a flag that takes
    // one, and the flag it looks like is not given.
    let request = sent(&doc, &["things", "get", "--name", "a", "--q", "--sum"]);
    assert_eq!(request.uri().query(), Some("q=--sum"));
    let request = sent(&doc, &["things", "get", "--name", "a", "--q", "-"]);
    assert_eq!(request.uri().query(), Some("q=-"));
    // And a value from an `enum` too.
    let request = sent(&doc, &["things", "get", "--name", "a", "--sort", "-name"]);
    assert_eq!(request.uri().query(), Some("sort=-name"));
    let refused = line(&doc, &["things", "get", "--name", "a", "--sort", "-other"])
        .expect_err("not one of the values");
    assert_eq!(refused.kind(), ErrorKind::InvalidValue);
}

// ------------------------------------------------------------------ F-36

/// `A7/reduce` is the input: a hand-written verb that declares `--id` with another
/// parser than the one the document types it with. Read with `get_one`, `values`
/// panics with "Mismatch between definition and access of `id`".
#[test]
fn a_flag_of_another_type_than_the_operation_reads_is_an_arg_error_that_names_it() {
    let doc = doc();
    let op = doc.get("getThing").expect("the operation");
    let matches = Command::new("get")
        .arg(
            Arg::new("name")
                .long("name")
                .value_parser(value_parser!(i64)),
        )
        .get_matches_from(["get", "--name", "5"]);

    let error = tree::values(op, &matches).expect_err("the flag holds an integer");

    assert!(
        matches!(&error, ArgError::FlagType { flag, .. } if flag == "name"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "`--name` holds a value of another type than the one the operation reads"
    );
    assert!(error.source().is_some(), "the cause is clap's");
}

/// Every flag `values` reads is read the same way: a parameter, a field of a flat
/// body, the file of a whole body and of a raw one, and the two parts of a
/// multipart one.
#[test]
fn every_flag_values_reads_is_read_without_a_panic() {
    let doc = Document::load(
        include_str!("fixtures/toy.yaml"),
        &[
            include_str!("fixtures/corrections.yaml"),
            include_str!("fixtures/cli.yaml"),
        ],
    )
    .expect("the toy document");
    for (id, flag) in [
        ("getVoucher", "id"),
        ("createVoucher", "total"),
        ("createVoucher", "json-body"),
        ("createContact", "json-body"),
        ("uploadDocument", "raw-body"),
        ("uploadDocumentMultipart", "file"),
        ("uploadDocumentMultipart", "field"),
    ] {
        let op = doc.get(id).expect("the operation");
        let matches = Command::new("verb")
            .arg(
                Arg::new(flag)
                    .long(flag)
                    .action(clap::ArgAction::Append)
                    .value_parser(value_parser!(i64)),
            )
            .get_matches_from(["verb", &format!("--{flag}"), "5"]);

        let error = tree::values(op, &matches).expect_err(&format!("`--{flag}` of {id}"));

        assert!(
            matches!(&error, ArgError::FlagType { flag: named, .. } if named == flag),
            "{id} `--{flag}`: {error:?}"
        );
        assert!(
            error.to_string().starts_with(&format!("`--{flag}` holds ")),
            "{id} `--{flag}`: {error}"
        );
    }
}

/// `tree::answers` reads a flag the command never declared as unanswered, and
/// `values` reads it as not given, so both are safe to point at a command this
/// crate did not build.
#[test]
fn a_command_that_declares_no_flag_gives_no_value_and_no_panic() {
    let doc = Document::load(
        include_str!("fixtures/toy.yaml"),
        &[
            include_str!("fixtures/corrections.yaml"),
            include_str!("fixtures/cli.yaml"),
        ],
    )
    .expect("the toy document");
    let bare = Command::new("verb").get_matches_from(["verb"]);
    for op in &doc {
        let values = tree::values(op, &bare).unwrap_or_else(|error| panic!("{}: {error}", op.id()));
        assert!(values.params().is_empty(), "{}", op.id());
    }
}

// ------------------------------------------------------------------ F-74

/// A verb of the adopter's own may declare a field's flag with no parser of the
/// document's rule. `Invocation::new` holds a body to its kind only, so the
/// rule is the reading's, and a value that breaks it is refused with the rule.
#[test]
fn a_field_the_verb_read_with_no_rule_is_refused_with_the_documents_rule() {
    let doc = Document::load(
        include_str!("fixtures/toy.yaml"),
        &[
            include_str!("fixtures/corrections.yaml"),
            include_str!("fixtures/cli.yaml"),
        ],
    )
    .expect("the toy document");
    let op = doc.get("createVoucher").expect("the operation");
    let matches = Command::new("create")
        .arg(Arg::new("total").long("total"))
        .get_matches_from(["create", "--total", "1,50"]);

    let error = tree::values(op, &matches).expect_err("a comma is not a decimal point");

    assert!(
        matches!(&error, ArgError::FieldValue { flag, source: ScalarError::Rule { .. } }
            if flag == "total"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "`--total` was given a value the document refuses"
    );
    assert_eq!(
        error.source().map(ToString::to_string).as_deref(),
        Some(r"`1,50` does not match ^-?[0-9]+(\.[0-9]{1,2})?$")
    );
}

// ------------------------------------------------------------------ F-75

/// `A2/wire` `numbody.yaml` is the input. A parameter is checked by
/// `Scalar::parse`, and unless the parsed value is what is kept, its raw text goes
/// on the wire: `+5` as `%2B5` and `007` as written, where the typed call sends
/// `5`.
#[test]
fn a_parameter_is_sent_as_it_was_parsed() {
    let doc = doc();
    for (raw, sent) in [("+5", "5"), ("007", "7"), ("-0", "0"), ("-007", "-7")] {
        assert_eq!(
            uri(&doc, "getItem", Values::new().param("id", raw)),
            format!("http://api.example.test/base/items/{sent}"),
            "path {raw}"
        );
        assert_eq!(
            uri(
                &doc,
                "getThing",
                Values::new().param("name", "a").param("n", raw)
            ),
            format!("http://api.example.test/base/things/a?n={sent}"),
            "query {raw}"
        );
        let op = doc.get("getThing").expect("the operation");
        let request = Invocation::new(op, Values::new().param("name", "a").param("X-Count", raw))
            .expect("an integer")
            .request(doc.base())
            .expect("a request");
        assert_eq!(request.headers()["x-count"], sent, "header {raw}");
    }
    for (raw, sent) in [
        ("1e3", "1000"),
        ("+0.50", "0.5"),
        ("007.25", "7.25"),
        ("1.0", "1"),
    ] {
        assert_eq!(
            uri(
                &doc,
                "getThing",
                Values::new().param("name", "a").param("sum", raw)
            ),
            format!("http://api.example.test/base/things/a?sum={sent}"),
            "{raw}"
        );
    }
}

/// The command line and the typed call send one request for one number, however
/// the number is written.
#[test]
fn the_command_line_and_the_typed_call_send_the_same_number() {
    let doc = doc();
    let from_line = sent(
        &doc,
        &[
            "things", "get", "--name", "a", "--n", "+5", "--sum", "007.50",
        ],
    );
    let typed = Values::new()
        .param("name", "a")
        .param("n", 5)
        .param("sum", 7.5);
    let from_call = doc.get("getThing").map(|op| {
        Invocation::new(op, typed)
            .expect("values")
            .request(doc.base())
            .expect("a request")
    });
    assert_eq!(
        Some(from_line.uri()),
        from_call.as_ref().map(http::Request::uri)
    );
    assert_eq!(from_line.uri().query(), Some("n=5&sum=7.5"));
}

/// `A3b/scalars` `integer` and `number` are the input. `9007199254740993` goes out
/// as `9007199254740992.0` and `1e-400` as `0.0` unless it is refused, and an
/// integer past `i64` is a number and not "not an integer".
#[test]
fn a_number_that_does_not_survive_the_round_trip_is_refused_on_both_roads() {
    let doc = doc();
    for raw in ["9007199254740993", "1e-400", "12345678901234567"] {
        let error = refused(
            &doc,
            "getThing",
            Values::new().param("name", "a").param("sum", raw),
        );
        assert!(
            matches!(&error, ValueError::BadValue { name, source: ScalarError::Rule { rule, .. }, .. }
                if name == "sum" && rule.starts_with("does not survive the round trip")),
            "{raw}: {error:?}"
        );
        let flag = line(&doc, &["things", "get", "--name", "a", "--sum", raw])
            .expect_err("the flag refuses it");
        assert_eq!(flag.kind(), ErrorKind::ValueValidation, "{raw}");
        let field = line(&doc, &["n", "create", "--sum", raw]).expect_err("the field refuses it");
        assert_eq!(field.kind(), ErrorKind::ValueValidation, "{raw}");
        assert!(
            field
                .to_string()
                .contains("does not survive the round trip"),
            "{field}"
        );
    }
}

/// Zero is zero whatever its exponent says, so a mantissa of zeros is read before
/// the exponent is, and an exponent past `i64` does not make it a number that
/// would be sent as another one.
#[test]
fn a_zero_with_an_exponent_past_i64_is_zero_on_both_roads() {
    let doc = doc();
    for raw in ["0e99999999999999999999", "0.00e-99999999999999999999"] {
        assert_eq!(
            uri(
                &doc,
                "getThing",
                Values::new().param("name", "a").param("sum", raw)
            ),
            "http://api.example.test/base/things/a?sum=0",
            "{raw}"
        );
        let request = sent(&doc, &["things", "get", "--name", "a", "--sum", raw]);
        assert_eq!(request.uri().query(), Some("sum=0"), "{raw}");
        let body = sent(&doc, &["n", "create", "--sum", raw]);
        assert_eq!(body.body().as_slice(), br#"{"sum":0.0}"#, "{raw}");
    }
    // A number that is not zero, with an exponent no `i64` holds, is still refused.
    let error = refused(
        &doc,
        "getThing",
        Values::new()
            .param("name", "a")
            .param("sum", "1e-99999999999999999999"),
    );
    assert!(
        matches!(&error, ValueError::BadValue { source: ScalarError::Rule { rule, .. }, .. }
            if rule.starts_with("does not survive the round trip")),
        "{error:?}"
    );
}

#[test]
fn an_integer_past_i64_is_out_of_range_on_both_roads() {
    let doc = doc();
    let raw = "9223372036854775808";
    let error = refused(
        &doc,
        "getThing",
        Values::new().param("name", "a").param("n", raw),
    );
    assert!(
        matches!(&error, ValueError::BadValue { source: ScalarError::Rule { rule, .. }, .. }
            if rule.starts_with("is out of range")),
        "{error:?}"
    );
    let flag =
        line(&doc, &["things", "get", "--name", "a", "--n", raw]).expect_err("the flag refuses it");
    assert_eq!(flag.kind(), ErrorKind::ValueValidation);
    assert!(flag.to_string().contains("is out of range"), "{flag}");
    assert!(!flag.to_string().contains("is not an integer"), "{flag}");
}

/// The body is the parsed value too: `007` is the number 7, and a number that is
/// the shortest text of its float is sent as the float.
#[test]
fn a_body_flag_is_sent_as_it_was_parsed() {
    let doc = doc();
    let request = sent(
        &doc,
        &[
            "n", "create", "--n", "007", "--sum", "1e3", "--flag", "true",
        ],
    );
    assert_eq!(
        request.body().as_slice(),
        br#"{"sum":1000.0,"n":7,"flag":true}"#
    );
    // The largest integer a float holds is still a number, however it is typed.
    let request = sent(&doc, &["n", "create", "--sum", "9007199254740992"]);
    assert_eq!(request.body().as_slice(), br#"{"sum":9007199254740992.0}"#);
}
