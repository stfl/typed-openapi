//! A document is read as what it says, or refused by name.
//!
//! The reduction never loads a document with a rule dropped, and a refusal says
//! what to change. Each case here is the input of a probe of SEV-249 or of a
//! finding of a review, and asserts the variant beside the words: the variant is
//! what a caller matches, and the message is what a person reads. Where a
//! message names a repair, the case also loads the document with that repair, so
//! the words are true.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::fmt::Write as _;

use http::Uri;
use typed_openapi::names::NameError;
use typed_openapi::overlay::OverlayError;
use typed_openapi::{
    Body, ConfirmationError, Document, LoadError, Loading, RefError, Shape, Unsupported,
};

/// A document at `version` with one server, and whatever the case puts after it.
fn at(version: &str, rest: &str) -> String {
    format!(
        "openapi: {version}\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         {rest}"
    )
}

/// What loading `rest` as an OpenAPI 3.0.3 document refuses with.
fn refused(rest: &str) -> LoadError {
    Document::load(&at("3.0.3", rest), &[]).expect_err("the document is refused")
}

/// The document `rest` makes, as an OpenAPI 3.0.3 document.
fn loaded(rest: &str) -> Document {
    Document::load(&at("3.0.3", rest), &[]).expect("the document loads")
}

/// One read operation, with the given parameters and nothing else.
fn read_with(parameters: &str) -> String {
    format!(
        "paths:\n  /items:\n    get:\n      operationId: listItems\n      parameters:\n{parameters}      \
         responses: {{ '200': {{ description: ok }} }}\n"
    )
}

// ------------------------------------------------------------------ F-13

/// What a 3.1 document says that a 3.0 reading drops in silence, refuses with
/// no place, or reads as another thing, and two documents that say nothing of
/// the kind. Every one is a 3.1 document, and that is why it is refused.
const THREE_ONE: [(&str, &str); 6] = [
    (
        "a numeric exclusiveMinimum",
        "paths:\n  /items:\n    get:\n      operationId: listItems\n      parameters:\n        - { \
         name: n, in: query, schema: { type: integer, exclusiveMinimum: 5 } }\n      responses: \
         { '200': { description: ok } }\n",
    ),
    (
        "a type list",
        "paths:\n  /items:\n    get:\n      operationId: listItems\n      parameters:\n        - { \
         name: n, in: query, schema: { type: [integer, 'null'] } }\n      responses: { '200': { \
         description: ok } }\n",
    ),
    (
        "a sibling of a $ref",
        "paths:\n  /items:\n    get:\n      operationId: listItems\n      parameters:\n        - { \
         name: n, in: query, schema: { $ref: '#/components/schemas/Count', maximum: 10 } }\n      \
         responses: { '200': { description: ok } }\ncomponents:\n  schemas:\n    Count: { type: \
         integer, minimum: 0 }\n",
    ),
    (
        "const",
        "paths:\n  /items:\n    get:\n      operationId: listItems\n      parameters:\n        - { \
         name: n, in: query, schema: { type: string, const: fixed } }\n      responses: { '200': \
         { description: ok } }\n",
    ),
    ("webhooks and no paths", "webhooks: {}\n"),
    (
        "nothing that 3.0 lacks",
        "paths: {}\ncomponents:\n  schemas:\n    Entry:\n      type: object\n      required: \
         [account]\n      properties:\n        account: { type: string }\n",
    ),
];

/// The version is the first thing the document is asked, so a 3.1 document is
/// refused for what it is and not for the first 3.1 spelling the model trips on.
#[test]
fn a_document_that_is_not_openapi_3_0_is_refused_with_its_version_named() {
    for (label, rest) in THREE_ONE {
        let error = Document::load(&at("3.1.0", rest), &[]).expect_err(label);

        assert!(
            matches!(&error, LoadError::Version { version } if version == "3.1.0"),
            "{label}: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "the document says `openapi: 3.1.0`, and this crate reads OpenAPI 3.0 only",
            "{label}"
        );
    }
}

/// The control of the case above: what a 3.0 document says in the same words is
/// read as 3.0 says it, so the version was the whole reason.
#[test]
fn the_same_words_under_openapi_3_0_3_are_not_refused_for_a_version() {
    for (label, rest) in THREE_ONE {
        let outcome = Document::load(&at("3.0.3", rest), &[]);

        assert!(
            !matches!(&outcome, Err(LoadError::Version { .. })),
            "{label}: {outcome:?}"
        );
    }
    assert!(
        Document::load(&at("3.0.3", "paths: {}\n"), &[]).is_ok(),
        "a 3.0.3 document with nothing in it loads"
    );
}

#[test]
fn every_version_of_openapi_3_0_loads_and_no_other_does() {
    for version in [
        "3.0",
        "3.0.0",
        "3.0.1",
        "3.0.2",
        "3.0.3",
        "3.0.4",
        "3.0.0-rc1",
    ] {
        assert!(
            Document::load(&at(&format!("'{version}'"), "paths: {}\n"), &[]).is_ok(),
            "{version}"
        );
    }
    for version in [
        "3.1.0", "3.1.1", "3.2.0", "3.10.0", "3.01.0", "4.0.0", "2.0", "3", "3.", "", "3.0.",
        "3.0.x", "3.0.foo", "3.0.1.2", "3.0.-rc1", "3.0.0-", "3.00", "3.0 ",
    ] {
        let error =
            Document::load(&at(&format!("'{version}'"), "paths: {}\n"), &[]).expect_err(version);

        assert!(
            matches!(&error, LoadError::Version { version: found } if found == version),
            "{version}: {error:?}"
        );
    }
}

/// SEV-118's document: a name that `required` asks for and nothing declares, with
/// a keyword that only 3.1 has standing where the declaration would be. The
/// version is what it is refused for, and the phantom check does not run.
#[test]
fn a_3_1_document_is_refused_for_its_version_before_the_phantom_check_reads_it() {
    const PHANTOM: &str = "paths: {}\ncomponents:\n  schemas:\n    Entry:\n      type: object\n      \
         required: [account, iban]\n      properties:\n        account: { type: string }\n      \
         dependentSchemas:\n        account:\n          properties:\n            iban: { type: string }\n";

    let error = Document::load(&at("3.1.0", PHANTOM), &[]).expect_err("a 3.1 document");

    assert!(
        matches!(&error, LoadError::Version { version } if version == "3.1.0"),
        "{error:?}"
    );
    assert!(
        matches!(refused(PHANTOM), LoadError::Phantom(_)),
        "under 3.0.3 the same words are the phantom check's to refuse"
    );
}

/// The model reads `openapi` as text, and a YAML `3.0` is a number. That is the
/// model's refusal, at the key, and it has no version to name.
#[test]
fn an_openapi_key_that_is_not_text_is_the_models_to_refuse() {
    let error = Document::load(&at("3.0", "paths: {}\n"), &[]).expect_err("a number");

    assert!(
        matches!(&error, LoadError::Shape { at, .. } if at == "openapi"),
        "{error:?}"
    );
}

/// An Overlay with one action that updates the root of the document with
/// `update`, written as the lines of a YAML mapping at the indent of `update:`.
fn updating_the_root(update: &str) -> String {
    format!(
        "overlay: 1.1.0\ninfo: {{ title: t, version: \"1\" }}\nactions:\n  - target: $\n    \
         description: the root\n    update:\n{update}"
    )
}

/// An Overlay that replaces the whole document with a list, which leaves no
/// `openapi` key. An action cannot remove the key by itself: a target that is a
/// string is refused.
const LEAVES_NO_VERSION: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  \
     - target: $\n    description: no document\n    update: [1]\n";

/// The version is the vendor's, and no Overlay holds a document to 3.0. A 3.1
/// document with an Overlay that says `3.0.3` is refused for the 3.1 it says on
/// its first line, because only this crate knows which rules the label would
/// drop.
#[test]
fn an_overlay_cannot_hold_a_3_1_document_to_3_0() {
    let relabels = updating_the_root("      openapi: 3.0.3\n");

    let error = Document::load(&at("3.1.0", "paths: {}\n"), &[&relabels])
        .expect_err("the vendor's document says 3.1");

    assert!(
        matches!(&error, LoadError::Version { version } if version == "3.1.0"),
        "{error:?}"
    );
}

/// The vendor's version is asked before the first layer, so a 3.1 document is
/// refused for its version when a layer would also have refused it.
#[test]
fn the_version_is_asked_before_any_layer_is_applied() {
    const NEVER_APPLIES: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - \
         target: $.nothing\n    description: a target that is not there\n    update:\n      x: 1\n";

    let error =
        Document::load(&at("3.1.0", "paths: {}\n"), &[NEVER_APPLIES]).expect_err("a 3.1 document");

    assert!(
        matches!(&error, LoadError::Version { version } if version == "3.1.0"),
        "{error:?}"
    );
    assert!(
        matches!(
            Document::load(&at("3.0.3", "paths: {}\n"), &[NEVER_APPLIES]),
            Err(LoadError::Layer { layer: 0, .. })
        ),
        "the layer is refused alone, so the version is what came first above"
    );
}

/// A layer changes the version by another version, another patch of 3.0, or no
/// key, and each is refused at the layer that does it.
#[test]
fn a_layer_that_changes_openapi_is_refused_at_its_layer() {
    let to_3_1 = updating_the_root("      openapi: 3.1.0\n");
    let to_3_0_0 = updating_the_root("      openapi: 3.0.0\n");
    let to_a_number = updating_the_root("      openapi: 3\n");
    let to_null = updating_the_root("      openapi: null\n");
    let harmless = updating_the_root("      x-note: kept\n");

    for (label, layers, layer, before, after) in [
        (
            "another version",
            vec![&to_3_1],
            0,
            Some("3.0.3"),
            Some("3.1.0"),
        ),
        (
            "another patch of 3.0",
            vec![&to_3_0_0],
            0,
            Some("3.0.3"),
            Some("3.0.0"),
        ),
        (
            "the second layer",
            vec![&harmless, &to_3_1],
            1,
            Some("3.0.3"),
            Some("3.1.0"),
        ),
        (
            "a value that is not text",
            vec![&to_a_number],
            0,
            Some("3.0.3"),
            Some("3"),
        ),
        ("null", vec![&to_null], 0, Some("3.0.3"), Some("null")),
    ] {
        let layers: Vec<&str> = layers.into_iter().map(String::as_str).collect();

        let error = Document::load(&at("3.0.3", "paths: {}\n"), &layers).expect_err(label);

        assert!(
            matches!(&error, LoadError::Layer { layer: found, source: OverlayError::OpenApiVersion { before: was, after: now } }
                if *found == layer
                    && was.as_deref() == before
                    && now.as_deref() == after),
            "{label}: {error:?}"
        );
    }
}

/// The message names the version before the layer and the version after it, and
/// the error says which layer and not that again.
#[test]
fn the_refusal_of_a_changed_version_names_both_versions_and_the_layer() {
    let to_3_1 = updating_the_root("      openapi: 3.1.0\n");

    let error = Document::load(&at("3.0.3", "paths: {}\n"), &[&to_3_1]).expect_err("a relabel");

    assert_eq!(error.to_string(), "cannot use `overlays[0]`");
    let cause = std::error::Error::source(&error).expect("the layer has a cause");
    assert_eq!(
        cause.to_string(),
        "the Overlay changes `openapi` from `3.0.3` to `3.1.0`; the version is the vendor's, and \
         no Overlay changes it"
    );
}

/// A document with no `openapi` key, and one server.
const WITHOUT: &str = "info: { title: t, version: \"1\" }\nservers: [{ url: 'http://localhost:9999' }]\npaths: \
                       {}\n";

/// A layer that leaves no key, and a layer that adds one to a document with none,
/// are changes too: no Overlay gives a document a version, so a document with no
/// `openapi` stays the model's refusal.
#[test]
fn a_layer_cannot_remove_the_version_or_give_a_document_one() {
    let left_out = Document::load(&at("3.0.3", "paths: {}\n"), &[LEAVES_NO_VERSION])
        .expect_err("no key is left");

    assert!(
        matches!(&left_out, LoadError::Layer { layer: 0, source: OverlayError::OpenApiVersion { before, after } }
            if before.as_deref() == Some("3.0.3") && after.is_none()),
        "{left_out:?}"
    );
    let cause = std::error::Error::source(&left_out).expect("the layer has a cause");
    assert_eq!(
        cause.to_string(),
        "the Overlay changes `openapi` from `3.0.3` to no `openapi` key; the version is the \
         vendor's, and no Overlay changes it"
    );

    let to_3_0 = updating_the_root("      openapi: 3.0.3\n");
    let given = Document::load(WITHOUT, &[&to_3_0]).expect_err("the key is added");

    assert!(
        matches!(&given, LoadError::Layer { layer: 0, source: OverlayError::OpenApiVersion { before, after } }
            if before.is_none() && after.as_deref() == Some("3.0.3")),
        "{given:?}"
    );
    assert!(
        matches!(Document::load(WITHOUT, &[]), Err(LoadError::Shape { at, .. }) if at == "."),
        "with no layer, the model refuses the missing key at the document"
    );
}

/// The control of the cases above: a layer that leaves `openapi` as it was, with
/// the same text written again, changes nothing.
#[test]
fn a_layer_that_writes_the_version_the_document_already_has_changes_nothing() {
    let same = updating_the_root("      openapi: 3.0.3\n");

    let loaded = Document::load(&at("3.0.3", "paths: {}\n"), &[&same]);

    assert!(loaded.is_ok(), "{loaded:?}");
}

// ------------------------------------------------------------------ F-39

const ONE_READ: &str = "paths:\n  /a:\n    get: { operationId: a, responses: { '200': { \
                        description: ok } } }\n";

/// A document with the `servers` the case gives, and one operation.
fn served_by(servers: &str) -> String {
    format!("openapi: 3.0.3\ninfo: {{ title: t, version: \"1\" }}\n{servers}{ONE_READ}")
}

#[test]
fn a_server_url_with_variables_uses_the_default_of_each() {
    for (url, variables, base) in [
        (
            "https://{tenant}.example.com/v1",
            "{ tenant: { default: demo } }",
            "https://demo.example.com/v1",
        ),
        (
            "https://{host}:{port}/{version}",
            "{ host: { default: api.example.com }, port: { default: '8443', enum: ['8443', \
             '443'] }, version: { default: v2 } }",
            "https://api.example.com:8443/v2",
        ),
        (
            "https://{x}.example.com/{x}",
            "{ x: { default: a } }",
            "https://a.example.com/a",
        ),
        (
            "https://{tenant}.example.com",
            "{ tenant: { default: demo }, region: { default: eu } }",
            "https://demo.example.com/",
        ),
    ] {
        let document = Document::load(
            &served_by(&format!(
                "servers: [{{ url: '{url}', variables: {variables} }}]\n"
            )),
            &[],
        )
        .expect("the defaults make a URL");

        assert_eq!(
            document.base(),
            &base.parse::<Uri>().expect("a URI"),
            "{url}"
        );
    }
}

/// A variable the document does not declare has no default to use, so the URL
/// stays a template and is not a URL. An unclosed brace is no variable, and
/// stays in the URL as it is written, so `https://{a.example.com` is refused as
/// well, though `a` is declared.
#[test]
fn a_server_variable_that_nothing_declares_leaves_a_url_that_is_refused() {
    let error = Document::load(
        &served_by("servers: [{ url: 'https://{tenant}.example.com' }]\n"),
        &[],
    )
    .expect_err("`tenant` has no default");

    assert!(
        matches!(&error, LoadError::ServerUrl { url, filled, .. }
            if url == "https://{tenant}.example.com" && filled == url),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "`servers[0].url` is not a URL: `https://{tenant}.example.com`"
    );

    let unclosed = Document::load(
        &served_by(
            "servers: [{ url: 'https://{a.example.com', variables: { a: { default: x } } }]\n",
        ),
        &[],
    )
    .expect_err("an unclosed brace is not a variable");

    assert!(
        matches!(&unclosed, LoadError::ServerUrl { url, filled, .. }
            if url == "https://{a.example.com" && filled == url),
        "{unclosed:?}"
    );
}

/// A default can make the URL bad, and then the refusal shows the URL as it was
/// refused, beside the URL as the document spells it, so a reader looks at the
/// default and not at the braces.
#[test]
fn a_default_that_makes_the_url_bad_is_shown_in_the_refusal() {
    let error = Document::load(
        &served_by(
            "servers: [{ url: 'https://{tenant}.example.com', variables: { tenant: { default: \
             my tenant } } }]\n",
        ),
        &[],
    )
    .expect_err("a space is no part of a host");

    assert!(
        matches!(&error, LoadError::ServerUrl { url, filled, .. }
            if url == "https://{tenant}.example.com" && filled == "https://my tenant.example.com"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "`servers[0].url` is not a URL: `https://{tenant}.example.com`, which is \
         `https://my tenant.example.com` with the default of each variable"
    );
}

const ADDS_A_SERVER: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - \
     target: $\n    description: where requests go\n    update:\n      servers:\n        - url: \
     'http://localhost:9999'\n";

/// OpenAPI defaults a missing `servers` to `/`, and a CLI has no host to send a
/// request to under `/`, so the document is refused. The Overlay that the
/// refusal names mends it.
#[test]
fn a_document_with_no_servers_is_refused_and_an_overlay_that_adds_one_mends_it() {
    for servers in ["", "servers: []\n"] {
        let document = served_by(servers);

        let error = Document::load(&document, &[]).expect_err("nowhere to send a request");

        assert!(
            matches!(&error, LoadError::NoServer),
            "{servers}: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "the document lists no `servers` entry to send requests to; add one in an Overlay",
            "{servers}"
        );
        let mended = Document::load(&document, &[ADDS_A_SERVER]).expect("the Overlay adds one");
        assert_eq!(
            mended.base(),
            &"http://localhost:9999/".parse::<Uri>().expect("a URI")
        );
    }
}

/// The operation is named by its method and its path, because it has no
/// `operationId` to name it by, and the Overlay that the refusal names mends it.
#[test]
fn an_operation_with_no_operation_id_is_refused_and_an_overlay_that_adds_one_mends_it() {
    const NAMELESS: &str = "paths:\n  /vouchers:\n    get:\n      responses: { '200': { \
                            description: OK } }\n";
    const ADDS_AN_ID: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - \
         target: $.paths['/vouchers'].get\n    description: the name of the operation\n    \
         update:\n      operationId: listVouchers\n";

    let error = refused(NAMELESS);

    assert!(
        matches!(&error, LoadError::NoOperationId { method, path }
            if method == "get" && path == "/vouchers"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "get /vouchers has no operationId; add one in an Overlay"
    );
    let mended =
        Document::load(&at("3.0.3", NAMELESS), &[ADDS_AN_ID]).expect("the Overlay names it");
    assert!(mended.get("listVouchers").is_some());
}

// ------------------------------------------------------------------ F-40

const CONTACTS: &str = "paths:\n  /kontakte/änderungen:\n    get:\n      operationId: \
     listÄnderungen\n      responses: { '200': { description: ok } }\n  \
     /kontakte/{id}/übersicht:\n    get:\n      operationId: getÜbersicht\n      parameters:\n        \
     - { name: id, in: path, required: true, schema: { type: string } }\n      responses: \
     { '200': { description: ok } }\n";

/// A segment with a letter outside ASCII would be the group `kontakte nderungen`
/// if kebab-casing alone read it: a name that is not the vendor's, in a place the
/// vendor would not look for it.
#[test]
fn a_path_segment_with_a_letter_outside_ascii_needs_the_markers_that_name_the_repair() {
    const MARKERS: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - \
         target: $.paths['/kontakte/änderungen'].get\n    update:\n      x-cli-command: \
         list-changes\n  - target: $.paths['/kontakte/{id}/übersicht'].get\n    update:\n      \
         x-cli-command: overview\n";

    let error = refused(CONTACTS);

    assert!(
        matches!(&error, LoadError::Name { op, source }
            if op == "listÄnderungen"
                && *source == NameError { origin: "path segment", raw: "änderungen".to_owned() }),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "listÄnderungen: no command name can be read off this operation; \
         name it with `x-cli-group` and `x-cli-command` in an Overlay"
    );
    let mended = Document::load(&at("3.0.3", CONTACTS), &[MARKERS]).expect("the markers name them");
    assert_eq!(
        mended.get("listÄnderungen").map(|op| op.command().as_str()),
        Some("list-changes")
    );
}

/// A letter written as a base and a combining mark shows as one letter, so it is
/// refused as the single character is, and not mangled into `a-nderungen`. Nor
/// does a character that nobody sees pass: it would be a separator that nobody
/// sees.
#[test]
fn a_letter_in_two_code_points_and_an_invisible_character_are_refused_like_any_other() {
    let segments = [
        "a\u{308}nderungen",
        "re\u{301}sume\u{301}",
        "fo\u{ad}o",
        "fo\u{200b}o",
    ];

    for segment in segments {
        let paths = format!(
            "paths:\n  /kontakte/{segment}:\n    get:\n      operationId: listChanges\n      \
             responses: {{ '200': {{ description: ok }} }}\n"
        );

        let error = refused(&paths);

        assert!(
            matches!(&error, LoadError::Name { op, source }
                if op == "listChanges"
                    && *source == NameError { origin: "path segment", raw: segment.to_owned() }),
            "{segment:?}: {error:?}"
        );
    }

    let names = ["cafe\u{301}", "gro\u{308}sse", "fo\u{ad}o", "fo\u{200b}o"];
    let mut parameters = String::new();
    for name in names {
        writeln!(
            parameters,
            "        - {{ name: \"{name}\", in: query, schema: {{ type: string }} }}"
        )
        .expect("a string takes a write");
    }
    let document = loaded(&read_with(&parameters));
    let op = document.get("listItems").expect("listItems");
    for name in names {
        assert_eq!(
            op.param(name).expect(name).shape(),
            &Shape::Unreachable(Unsupported::Unspellable),
            "{name:?} is carried with no flag, and not as `cafe` or `gro-sse`"
        );
    }
}

/// An Overlay that drops the `required` of the first parameter of `listItems`.
const DROPS_REQUIRED: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - \
         target: $.paths['/items'].get.parameters[0]\n    description: not required\n    update:\n      \
         required: false\n";

#[test]
fn a_parameter_with_a_letter_outside_ascii_has_no_flag_and_a_required_one_refuses_the_document() {
    const NAMES: &str = "        - { name: größe, in: query, schema: { type: string } }\n        - \
                         { name: straße, in: query, schema: { type: string } }\n";

    let document = loaded(&read_with(NAMES));
    let op = document.get("listItems").expect("listItems");

    for name in ["größe", "straße"] {
        let param = op.param(name).expect(name);
        assert_eq!(
            param.shape(),
            &Shape::Unreachable(Unsupported::Unspellable),
            "{name} is carried with no flag, and not as `gr-e` or `stra-e`"
        );
    }

    let required = read_with(
        "        - { name: straße, in: query, required: true, schema: { type: string } }\n",
    );
    let error = refused(&required);
    assert!(
        matches!(&error, LoadError::Parameter { op, name, why }
            if op == "listItems" && name == "straße" && *why == Unsupported::Unspellable),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "listItems: parameter `straße` is named in a way that does not kebab-case into \
         [a-z0-9-], and the document requires it; correct the parameter in an Overlay, or \
         drop its `required`"
    );
    let mended =
        Document::load(&at("3.0.3", &required), &[DROPS_REQUIRED]).expect("the Overlay drops it");
    assert_eq!(
        mended
            .get("listItems")
            .and_then(|op| op.param("straße"))
            .map(typed_openapi::Param::shape),
        Some(&Shape::Unreachable(Unsupported::Unspellable))
    );
}

/// A body property with such a name has no flag, and a body whose property has
/// no flag goes whole through `--json-body`.
#[test]
fn a_body_property_with_a_letter_outside_ascii_sends_the_body_whole() {
    const BODY: &str = "paths:\n  /items:\n    post:\n      operationId: createItem\n      \
         requestBody:\n        content:\n          application/json:\n            schema:\n              \
         type: object\n              properties:\n                größe: { type: string }\n      \
         responses: { '201': { description: ok } }\n";

    let document = loaded(BODY);

    assert!(
        matches!(
            document.get("createItem").expect("createItem").body(),
            Body::JsonWhole { .. }
        ),
        "the property is no flag, and is not the flag `--gr-e`"
    );
}

/// An Overlay that writes the gate of `createVoucher` again, with an ASCII word.
const SPELLS_THE_GATE: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - \
         target: $.paths['/vouchers'].post['x-cli-gates']\n    description: drop the word\n    \
         remove: true\n  - target: $.paths['/vouchers'].post\n    description: an ASCII word\n    \
         update:\n      x-cli-gates: [delete]\n";

#[test]
fn a_gate_and_a_confirmation_with_a_letter_outside_ascii_are_refused() {
    const GATED: &str = "paths:\n  /vouchers:\n    post:\n      operationId: createVoucher\n      \
         x-cli-gates: [löschen]\n      responses: { '201': { description: ok } }\n";

    let error = refused(GATED);

    assert!(
        matches!(&error, LoadError::GateName { op, source }
            if op == "createVoucher"
                && *source == NameError { origin: "x-cli-gates", raw: "löschen".to_owned() }),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "createVoucher: a gate in `x-cli-gates` is not a word that can be a flag; correct it in \
         an Overlay"
    );
    let mended =
        Document::load(&at("3.0.3", GATED), &[SPELLS_THE_GATE]).expect("the gate is ASCII");
    let gates = mended.get("createVoucher").expect("createVoucher").gates();
    assert_eq!(
        gates
            .iter()
            .map(typed_openapi::Gate::as_str)
            .collect::<Vec<_>>(),
        ["delete"]
    );

    let word = Loading::new()
        .commit("löschen")
        .expect_err("a word outside ASCII");
    assert!(
        matches!(&word, ConfirmationError::Spelling(source)
            if *source == NameError { origin: "confirmation", raw: "löschen".to_owned() }),
        "{word:?}"
    );
    assert_eq!(
        word.to_string(),
        "the confirmation `löschen` does not kebab-case into [a-z0-9-]"
    );
    assert!(
        Loading::new().commit("loeschen").is_ok(),
        "an ASCII word is the repair"
    );
}

// ------------------------------------------------------------------ F-41

/// `listVouchers` reads a collection. A `{tenant}` above the group says which
/// tenant, and not that the path addresses one voucher.
#[test]
fn a_template_above_the_group_does_not_make_a_path_address_one_resource() {
    const TENANT: &str =
        "        - { name: tenant, in: path, required: true, schema: { type: string } }\n";
    let paths = format!(
        "paths:\n  /{{tenant}}/vouchers:\n    get:\n      operationId: listVouchers\n      \
         parameters:\n{TENANT}      responses: {{ '200': {{ description: ok }} }}\n  \
         /{{tenant}}/vouchers/{{id}}:\n    get:\n      operationId: getVoucher\n      \
         parameters:\n{TENANT}        - {{ name: id, in: path, required: true, schema: {{ type: \
         string }} }}\n      responses: {{ '200': {{ description: ok }} }}\n  \
         /{{tenant}}/contacts:\n    get:\n      operationId: listContacts\n      parameters:\n{TENANT}      \
         responses: {{ '200': {{ description: ok }} }}\n"
    );

    let document = loaded(&paths);

    let placed = |id: &str| {
        let op = document.get(id).expect(id);
        (
            op.group().as_str().to_owned(),
            op.command().as_str().to_owned(),
        )
    };
    assert_eq!(
        placed("listVouchers"),
        ("vouchers".to_owned(), "list".to_owned())
    );
    assert_eq!(
        placed("getVoucher"),
        ("vouchers".to_owned(), "get".to_owned())
    );
    assert_eq!(
        placed("listContacts"),
        ("contacts".to_owned(), "list".to_owned())
    );
}

// ------------------------------------------------------------------ F-78

/// The probe `deepref`: the reference is a valid JSON Pointer to the `code` of
/// `Holder`, and this crate follows a component and goes no further in. The
/// refusal says which references it follows, and does not say the pointer is
/// missing.
#[test]
fn a_ref_into_the_inside_of_a_schema_is_refused_with_what_is_followed() {
    const HOLDER: &str = "components:\n  schemas:\n    Holder:\n      type: object\n      properties:\n        \
         code: { type: string, pattern: '^[A-Z]{3}$' }\n";
    let inside = format!(
        "{}{HOLDER}",
        read_with(
            "        - { name: code, in: query, schema: { $ref: \
             '#/components/schemas/Holder/properties/code' } }\n"
        )
    );

    let error = refused(&inside);

    assert!(
        matches!(&error, LoadError::Unresolvable { op, name, source: RefError::Inside { reference } }
            if op == "listItems"
                && name == "code"
                && reference == "#/components/schemas/Holder/properties/code"),
        "{error:?}"
    );
    let cause = std::error::Error::source(&error).expect("the layer has a cause");
    assert_eq!(
        cause.to_string(),
        "`#/components/schemas/Holder/properties/code` points inside a component, and this crate \
         follows only `#/components/<section>/<name>`"
    );
    let followed = format!(
        "{}{HOLDER}    Code: {{ type: string, pattern: '^[A-Z]{{3}}$' }}\n",
        read_with(
            "        - { name: code, in: query, schema: { $ref: '#/components/schemas/Code' } }\n"
        )
    );
    assert!(
        Document::load(&at("3.0.3", &followed), &[]).is_ok(),
        "a reference to the component is the one that is followed"
    );
}

/// A reference that names nothing is still a reference that does not resolve.
#[test]
fn a_ref_to_nothing_still_does_not_resolve() {
    let missing = read_with(
        "        - { name: code, in: query, schema: { $ref: '#/components/schemas/Nope' } }\n",
    );

    let error = refused(&missing);

    assert!(
        matches!(&error, LoadError::Unresolvable { source: RefError::Missing { reference }, .. }
            if reference == "#/components/schemas/Nope"),
        "{error:?}"
    );
    let cause = std::error::Error::source(&error).expect("the layer has a cause");
    assert_eq!(
        cause.to_string(),
        "`#/components/schemas/Nope` does not resolve"
    );
}

/// A pointer that goes inside a name that no component has does not point inside
/// a component: there is none, and the name is what to look for.
#[test]
fn a_ref_inside_a_component_that_does_not_exist_does_not_resolve() {
    let missing = read_with(
        "        - { name: code, in: query, schema: { $ref: \
         '#/components/schemas/Nope/properties/code' } }\n",
    );

    let error = refused(&missing);

    assert!(
        matches!(&error, LoadError::Unresolvable { source: RefError::Missing { reference }, .. }
            if reference == "#/components/schemas/Nope/properties/code"),
        "{error:?}"
    );
}
