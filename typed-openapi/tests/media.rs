//! A body goes out under the media type that the document declares.
//!
//! A JSON body is sent under the `content` key it was declared under, a media
//! range is refused by name, and the key that is picked among several is the one
//! `docs/validation.md` writes down. The documents are the input of the probes of
//! SEV-249 behind F-9 (`A2/wire` `main.yaml`, `A7` `suffix.yaml` and `range.yaml`,
//! and `p07` of `A3a/reduce`), in `tests/fixtures/media.yaml` where they have to
//! load and beside their assertion where they are refused. A case that refuses a
//! document asserts the variant beside the words, and loads the document with the
//! repair the words name.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use serde_json::json;
use typed_openapi::model::Body;
use typed_openapi::tree::{self, Asked, Outcome};
use typed_openapi::{Document, Invocation, JSON_BODY, LoadError, Recorder, Values};

const MEDIA: &str = include_str!("fixtures/media.yaml");

fn media() -> Document {
    Document::load(MEDIA, &[]).expect("the fixture loads")
}

/// A document with one server and one POST whose body is declared under `content`.
fn posting(content: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         paths:\n  /uploads:\n    post:\n      operationId: upload\n      requestBody:\n        \
         required: true\n        content:\n{content}      responses: {{ \"201\": {{ description: OK }} }}\n"
    )
}

/// What the reduction made of a body, and the media type it goes out under.
/// A multipart body is assembled with a boundary of its own, so it names none.
fn picked(doc: &Document, id: &str) -> (&'static str, Option<String>) {
    match doc.get(id).expect("the operation").body() {
        Body::JsonFields { media_type, .. } => ("fields", Some(media_type.clone())),
        Body::JsonWhole { media_type, .. } => ("whole", Some(media_type.clone())),
        Body::Opaque { media_type, .. } => ("opaque", Some(media_type.clone())),
        Body::Multipart { .. } => ("multipart", None),
        Body::None => ("none", None),
    }
}

/// The `content-type` of the request that `values` make of `id`.
fn content_type(doc: &Document, id: &str, values: Values) -> String {
    let op = doc.get(id).expect("the operation");
    let request = Invocation::new(op, values)
        .expect("the values satisfy the operation")
        .request(doc.base())
        .expect("a request");
    request.headers()["content-type"]
        .to_str()
        .expect("the header is text")
        .to_owned()
}

/// The `content-type` of the request that a command line sends for `id`: the
/// group and the command, then `args`, then the confirmation.
fn sent_under(doc: &Document, id: &str, args: &[&str]) -> String {
    let op = doc.get(id).expect("the operation");
    let mut line = vec!["media", op.group().as_str(), op.command().as_str()];
    line.extend_from_slice(args);
    line.push("--commit");
    let matches = clap::Command::new("media")
        .subcommand_required(true)
        .subcommands(tree::commands(doc))
        .get_matches_from(line);
    let Asked::Run(selection) =
        tree::select(doc, &matches).expect("the command names an operation")
    else {
        panic!("this command line runs the operation");
    };
    let client = Recorder::new();
    let outcome = selection
        .send(&client, doc.base())
        .expect("the request is sent");
    assert!(matches!(outcome, Outcome::Sent(_)), "the commit sends it");
    let mut sent = client.take();
    assert_eq!(sent.len(), 1, "exactly one request went out");
    let request = sent.pop().expect("the length is one");
    request.headers()["content-type"]
        .to_str()
        .expect("the header is text")
        .to_owned()
}

/// A file under `CARGO_TARGET_TMPDIR`, named for this test binary's process so
/// that two runs do not meet in one file.
fn scratch(name: &str, text: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("typed-openapi-media-{}-{name}", std::process::id()));
    std::fs::write(&path, text).expect("writing the scratch file");
    path
}

// ------------------------------------------------------------------ F-9

/// `A2/wire` `main.yaml`, `things patch`: the body is declared as
/// `application/merge-patch+json` and went out as `application/json`.
#[test]
fn a_merge_patch_goes_out_as_a_merge_patch() {
    let doc = media();
    let values = || Values::new().param("id", 5).json(json!({ "title": "T" }));

    assert_eq!(
        content_type(&doc, "patchThing", values()),
        "application/merge-patch+json"
    );
    assert_eq!(
        sent_under(&doc, "patchThing", &["--id", "5", "--title", "T"]),
        "application/merge-patch+json"
    );
}

/// `A7` `suffix.yaml`, `createArticle`.
#[test]
fn a_json_api_body_goes_out_as_json_api() {
    let doc = media();
    let values = Values::new().json(json!({ "title": "T" }));

    assert_eq!(
        content_type(&doc, "createArticle", values),
        "application/vnd.api+json"
    );
    assert_eq!(
        sent_under(&doc, "createArticle", &["--title", "T"]),
        "application/vnd.api+json"
    );
}

/// A body that goes whole is one `--json-body` file, and goes out under the same
/// type as a body that has flags. The key is sent as the document spells it, with
/// its parameter, because a server matches on that parameter.
#[test]
fn a_body_that_goes_whole_goes_out_under_the_key_it_was_declared_under() {
    let doc = media();
    let profiled = r#"application/vnd.api+json; profile="https://example.com/Profiles/Timestamps""#;
    let values = Values::new().json(json!({ "data": { "type": "comments" } }));
    assert_eq!(content_type(&doc, "createComment", values), profiled);

    let file = scratch("comment.json", r#"{ "data": { "type": "comments" } }"#);
    let path = file.to_str().expect("the path is text");
    let line = ["--".to_owned() + JSON_BODY, path.to_owned()];
    let sent = sent_under(&doc, "createComment", &[&line[0], &line[1]]);
    std::fs::remove_file(&file).expect("removing the scratch file");
    assert_eq!(sent, profiled);
}

/// A key that carries no schema is a body to send whole, and it goes out under
/// the key all the same.
#[test]
fn a_body_with_no_schema_goes_out_under_its_key() {
    let doc = media();
    let values = Values::new().json(json!({ "data": null }));

    assert_eq!(
        content_type(&doc, "postFreeform", values),
        "application/vnd.api+json"
    );
    assert_eq!(
        picked(&doc, "postFreeform"),
        ("whole", Some("application/vnd.api+json".to_owned()))
    );
}

/// The model says which type each JSON body goes out under, so a caller that
/// walks the operations can tell a merge patch from a plain JSON body.
#[test]
fn a_json_body_carries_the_type_it_was_declared_under() {
    let doc = media();

    assert_eq!(
        picked(&doc, "patchThing"),
        ("fields", Some("application/merge-patch+json".to_owned()))
    );
    assert_eq!(
        picked(&doc, "createComment"),
        (
            "whole",
            Some(
                r#"application/vnd.api+json; profile="https://example.com/Profiles/Timestamps""#
                    .to_owned()
            )
        )
    );
}

/// The blob is the only door a shipped binary has onto the document.
#[test]
fn the_type_survives_the_blob() {
    let doc = media();

    let read = Document::from_blob(&doc.to_blob().expect("the document encodes"))
        .expect("the blob decodes");

    assert_eq!(picked(&read, "patchThing"), picked(&doc, "patchThing"));
    assert_eq!(
        content_type(
            &read,
            "patchThing",
            Values::new().param("id", 5).json(json!({}))
        ),
        "application/merge-patch+json"
    );
}

/// `p07a` and `p07d` of `A3a/reduce`, and the keys beside them. With several
/// `content` keys the first that names JSON is sent, wherever it stands, and else
/// the first key of all. A multipart key is no better than any other. A media
/// range beside the key that is sent is not read.
#[test]
fn the_key_that_is_sent_is_the_first_json_one_and_else_the_first_of_all() {
    let doc = media();
    let cases = [
        ("pickJsonOverText", "fields", Some("application/json")),
        ("pickFirstJson", "fields", Some("application/vnd.api+json")),
        (
            "pickUrlencodedFirst",
            "opaque",
            Some("application/x-www-form-urlencoded"),
        ),
        ("pickMultipartFirst", "multipart", None),
        ("pickJsonOverRange", "fields", Some("application/json")),
        ("pickJsonOverJsonRange", "fields", Some("application/json")),
        ("pickTextOverRange", "opaque", Some("text/plain")),
    ];

    for (id, kind, media_type) in cases {
        assert_eq!(
            picked(&doc, id),
            (kind, media_type.map(str::to_owned)),
            "{id}"
        );
    }
}

/// `A7` `range.yaml` and `p07b`, `p07c`: a range was the request's
/// `Content-Type`, so `*/*` went to the server as a `content-type`.
#[test]
fn a_media_range_as_the_key_that_is_sent_is_refused_by_name() {
    let object = "{ type: object, properties: { a: { type: string } } }";
    let binary = "{ type: string, format: binary }";
    for (key, schema) in [
        ("'*/*'", binary),
        ("'*/*'", object),
        ("image/*", binary),
        ("'application/*'", binary),
        // A wildcard in the subtype is no more a type than a range is, and
        // `is_json` took it for JSON.
        ("'application/*+json'", object),
    ] {
        let content = format!("          {key}:\n            schema: {schema}\n");
        let error = Document::load(&posting(&content), &[]).expect_err(key);

        let key = key.trim_matches('\'');
        assert!(
            matches!(&error, LoadError::MediaRange { op, media_type }
                if op == "upload" && media_type == key),
            "{key}: {error:?}"
        );
        let words = error.to_string();
        assert!(words.contains("upload") && words.contains(key), "{words}");
        assert!(words.contains("media range"), "{words}");
    }
}

/// The range is read when it is the key that would be sent, which is the first
/// key when no key names JSON, and not otherwise.
#[test]
fn a_range_is_refused_when_nothing_before_it_is_sent() {
    let first = "          image/*:\n            schema: { type: string, format: binary }\n          \
                 text/plain:\n            schema: { type: string }\n";
    let error = Document::load(&posting(first), &[]).expect_err("the range comes first");
    assert!(
        matches!(&error, LoadError::MediaRange { media_type, .. } if media_type == "image/*"),
        "{error:?}"
    );

    let json_last = "          image/*:\n            schema: { type: string, format: binary }\n          \
                     application/json:\n            schema: { type: object }\n";
    Document::load(&posting(json_last), &[]).expect("the key that is sent names JSON");
}

/// The words name the repair: the vendor's server reads one type, and the
/// Overlay says which, as it does for a `content` key that is no type.
#[test]
fn the_overlay_the_words_name_replaces_the_range_with_a_type() {
    let range = "          '*/*':\n            schema: { type: string, format: binary }\n";
    let overlay = "overlay: 1.1.0\n\
                   info: { title: t, version: \"1\" }\n\
                   actions:\n  \
                   - target: \"$.paths['/uploads'].post.requestBody.content['*/*']\"\n    \
                   remove: true\n  \
                   - target: \"$.paths['/uploads'].post.requestBody.content\"\n    \
                   update: { application/octet-stream: { schema: { type: string, format: binary } } }\n";

    let doc = Document::load(&posting(range), &[overlay]).expect("the Overlay repairs it");

    assert_eq!(
        picked(&doc, "upload"),
        ("opaque", Some("application/octet-stream".to_owned()))
    );
    assert_eq!(
        content_type(&doc, "upload", Values::new().raw(vec![1])),
        "application/octet-stream"
    );
}

/// The refusal of a key that is no type at all is the one it was, and a range is
/// not that: a `*` with nothing beside it names no type either, and is asked
/// first.
#[test]
fn a_key_that_is_no_media_type_is_still_not_a_media_type() {
    for key in ["form-data", "'*'"] {
        let content = format!("          {key}:\n            schema: {{ type: string }}\n");

        let error = Document::load(&posting(&content), &[]).expect_err(key);

        let key = key.trim_matches('\'');
        assert!(
            matches!(&error, LoadError::MediaType { op, media_type }
                if op == "upload" && media_type == key),
            "{key}: {error:?}"
        );
    }
}
