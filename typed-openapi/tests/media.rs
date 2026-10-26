//! A body goes out under the media type that the document declares.
//!
//! A JSON body is sent under the `content` key it was declared under, a media
//! range is refused by name, and the key that is picked among several is the one
//! `docs/validation.md` writes down. The documents are the input of the probes of
//! SEV-249 behind F-9 (`A2/wire` `main.yaml`, `A7` `suffix.yaml` and `range.yaml`,
//! and `p07` of `A3a/reduce`), in `tests/fixtures/media.yaml` where they have to
//! load and beside their assertion where they are refused. A case that refuses a
//! document asserts the variant beside the words. The Overlays that the words name
//! as the repair of a range are loaded in
//! `the_overlay_the_words_name_replaces_the_range_with_a_type` and
//! `the_overlay_that_removes_a_range_beside_json_leaves_the_json_key_to_be_sent`.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use serde_json::json;
use typed_openapi::model::Body;
use typed_openapi::tree::{self, Asked, Outcome};
use typed_openapi::{Document, Invocation, JSON_BODY, LoadError, Part, Recorder, Values};

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

/// What the reduction made of a body, and the media type it goes out under
/// (`Body::media_type`). A multipart body is assembled with a boundary of its
/// own, so it names none.
fn picked(doc: &Document, id: &str) -> (&'static str, Option<String>) {
    let body = doc.get(id).expect("the operation").body();
    let kind = match body {
        Body::JsonFields { .. } => "fields",
        Body::JsonWhole { .. } => "whole",
        Body::Opaque { .. } => "opaque",
        Body::Multipart { .. } => "multipart",
        Body::None => "none",
    };
    (kind, body.media_type().map(str::to_owned))
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

/// `A2/wire` `main.yaml`, `things patch` is the input: the body is declared as
/// `application/merge-patch+json`, and it goes out as that.
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

/// `A7` `suffix.yaml`, `createArticle` is the input: a body declared as
/// `application/vnd.api+json`.
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
/// walks the operations can tell a merge patch from a plain JSON body. A body
/// that the document does not declare has none.
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
    assert_eq!(picked(&doc, "getThing"), ("none", None));
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
        (
            "pickJsonBeforeJsonRange",
            "fields",
            Some("application/json"),
        ),
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

/// Loads `content` as the body of `upload` and asserts that the document is
/// refused as `LoadError::MediaRange` for `range`, with words that name the
/// operation, the key and what it is.
fn assert_refused_as_a_range(content: &str, range: &str) {
    let error = Document::load(&posting(content), &[]).expect_err(range);

    assert!(
        matches!(&error, LoadError::MediaRange { op, media_type }
            if op == "upload" && media_type == range),
        "{range}: {error:?}"
    );
    let words = error.to_string();
    assert!(words.contains("upload") && words.contains(range), "{words}");
    assert!(words.contains("media range"), "{words}");
}

/// `A7` `range.yaml` and `p07b`, `p07c` are the input: one body declared under a
/// range. A range names a set of types and a request goes out under one, so a
/// range that is the key that is sent is refused by name.
#[test]
fn a_media_range_as_the_key_that_is_sent_is_refused_by_name() {
    let object = "{ type: object, properties: { a: { type: string } } }";
    let binary = "{ type: string, format: binary }";
    for (key, schema) in [
        ("'*/*'", binary),
        ("'*/*'", object),
        ("image/*", binary),
        ("'application/*'", binary),
        ("'application/*+json'", object),
    ] {
        let content = format!("          {key}:\n            schema: {schema}\n");
        assert_refused_as_a_range(&content, key.trim_matches('\''));
    }
}

/// A range is refused when it is the key that is sent: the first that names
/// JSON, and else the first of all. A key beside it is not read, wherever it
/// stands.
#[test]
fn a_range_is_refused_when_no_key_names_json_and_it_is_the_first() {
    let first = "          image/*:\n            schema: { type: string, format: binary }\n          \
                 text/plain:\n            schema: { type: string }\n";
    assert_refused_as_a_range(first, "image/*");

    let json_last = "          image/*:\n            schema: { type: string, format: binary }\n          \
                     application/json:\n            schema: { type: object }\n";
    Document::load(&posting(json_last), &[]).expect("the key that is sent names JSON");
}

/// `application/*+json` ends in `+json`, so the rule that picks a key reads it as
/// JSON, and it is the key that is sent wherever it stands as the first that
/// names JSON: before `application/json`, after a key that names no JSON, and
/// after one that `Body::build` would otherwise send. It is then refused as the
/// range it is. After a key that names JSON it is not read.
#[test]
fn a_range_that_ends_in_json_is_the_first_json_key_and_is_refused() {
    let range = "          'application/*+json':\n            schema: { type: object, \
                 properties: { a: { type: string } } }\n";
    let json = "          application/json:\n            schema: { type: object, \
                properties: { b: { type: string } } }\n";
    let other = |key: &str| format!("          {key}:\n            schema: {{ type: string }}\n");

    for before in [
        String::new(),
        other("text/plain"),
        other("application/x-www-form-urlencoded"),
        other("multipart/form-data"),
    ] {
        assert_refused_as_a_range(&format!("{before}{range}{json}"), "application/*+json");
        assert_refused_as_a_range(&format!("{before}{range}"), "application/*+json");
    }

    Document::load(&posting(&format!("{json}{range}")), &[])
        .expect("a key that names JSON is sent first");
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

/// The Overlay that removes a range that ends in `+json` from a body that
/// declares `application/json` beside it is the repair of that document: the key
/// that names JSON without a `*` is the one that is sent, with its own schema.
#[test]
fn the_overlay_that_removes_a_range_beside_json_leaves_the_json_key_to_be_sent() {
    let both = "          'application/*+json':\n            schema: { type: object, \
                properties: { a: { type: string } } }\n          application/json:\n            \
                schema: { type: object, properties: { b: { type: string } } }\n";
    let overlay = "overlay: 1.1.0\n\
                   info: { title: t, version: \"1\" }\n\
                   actions:\n  \
                   - target: \"$.paths['/uploads'].post.requestBody.content['application/*+json']\"\n    \
                   remove: true\n";

    let doc = Document::load(&posting(both), &[overlay]).expect("the Overlay repairs it");

    assert_eq!(
        picked(&doc, "upload"),
        ("fields", Some("application/json".to_owned()))
    );
    let names: Vec<&str> = doc
        .get("upload")
        .expect("the operation")
        .body()
        .fields()
        .iter()
        .map(typed_openapi::Field::name)
        .collect();
    assert_eq!(names, ["b"]);
}

/// A key that has no type and subtype is refused as no media type, and a `*`
/// with nothing beside it is no range either: the question whether a key is a
/// media type is asked first.
#[test]
fn a_key_with_no_type_and_subtype_is_no_media_type_and_no_range() {
    for key in ["form-data", "'*'"] {
        let content = format!("          {key}:\n            schema: {{ type: string }}\n");

        let error = Document::load(&posting(&content), &[]).expect_err(key);

        let key = key.trim_matches('\'');
        assert!(
            matches!(&error, LoadError::MediaType { op, media_type }
                if op == "upload" && media_type == key),
            "{key}: {error:?}"
        );
        let words = error.to_string();
        assert!(words.contains("upload") && words.contains(key), "{words}");
        assert!(words.contains("not a media type"), "{words}");
    }
}

/// Loads `key` as the one `content` key of `upload`, with `schema`, and asserts
/// that the document is refused as `LoadError::MediaType` and that the words name
/// the operation, say "not a media type" and spell the key as `shown`, with no
/// character in the words that does not show.
fn assert_refused_as_no_media_type(key: &str, schema: &str, shown: &str) {
    let content = format!("          {key}:\n            schema: {schema}\n");

    let error = Document::load(&posting(&content), &[]).expect_err(key);

    assert!(
        matches!(&error, LoadError::MediaType { op, .. } if op == "upload"),
        "{key}: {error:?}"
    );
    let words = error.to_string();
    assert!(words.contains("upload"), "{words}");
    assert!(words.contains(shown), "{words}");
    assert!(words.contains("not a media type"), "{words}");
    assert!(
        words.chars().all(|c| c == ' ' || c.is_ascii_graphic()),
        "{words:?}"
    );
}

/// A key that goes out as the `Content-Type` is a header value, so a key that no
/// header carries is no media type, whether the body is JSON or sent verbatim.
/// The words write the byte as its code point: a key that looks like a media type
/// says why it is refused. The inputs are `application/json; x="a<U+0001>b"`, the
/// same with a newline, `application/json;x=<DEL>`, and a `text/plain` key.
#[test]
fn a_key_that_no_header_carries_is_no_media_type() {
    let object = "{ type: object, properties: { a: { type: string } } }";
    for (key, schema, shown) in [
        (r#""application/json; x=\"a\u0001b\"""#, object, r"a\u{1}b"),
        (r#""application/json; x=\"a\nb\"""#, object, r"a\u{a}b"),
        (r#""application/json;x=\u007f""#, object, r"x=\u{7f}"),
        (
            r#""text/plain; x=\"a\u0001b\"""#,
            "{ type: string }",
            r"a\u{1}b",
        ),
    ] {
        assert_refused_as_no_media_type(key, schema, shown);
    }
}

/// A key with a white space other than a space or a tab before its type or after
/// its subtype is no media type, because it is not one to RFC 9110: the white
/// space is no `token` character and no optional white space, and the header
/// would carry it. The inputs are `application/json<NBSP>`,
/// `<NEL>application/json`, `application/json<U+3000>`,
/// `application/merge-patch+json<NBSP>` and a `text/plain<NBSP>` key, which a
/// body sent verbatim holds.
#[test]
fn a_key_with_white_space_other_than_a_space_or_a_tab_is_no_media_type() {
    let object = "{ type: object, properties: { a: { type: string } } }";
    let text = "{ type: string }";
    for (key, schema, shown) in [
        (
            r#""application/json\u00a0""#,
            object,
            r"application/json\u{a0}",
        ),
        (
            r#""\u0085application/json""#,
            object,
            r"\u{85}application/json",
        ),
        (
            r#""application/json\u3000""#,
            object,
            r"application/json\u{3000}",
        ),
        (
            r#""application/merge-patch+json\u00a0""#,
            object,
            r"application/merge-patch+json\u{a0}",
        ),
        (r#""text/plain\u00a0""#, text, r"text/plain\u{a0}"),
    ] {
        assert_refused_as_no_media_type(key, schema, shown);
    }
}

/// The words write a character that does not show as its code point, whatever
/// kind of character it is, because a key refused for one would read as a media
/// type with no fault in it. The inputs are `application/json` with a zero width
/// space, a left-to-right mark or a word joiner after it, a byte order mark
/// before it, a soft hyphen inside it, and a combining acute accent after it.
#[test]
fn a_character_that_does_not_show_is_written_as_its_code_point() {
    let object = "{ type: object, properties: { a: { type: string } } }";
    for (key, shown) in [
        (r#""application/json\u200b""#, r"application/json\u{200b}"),
        (r#""\ufeffapplication/json""#, r"\u{feff}application/json"),
        (r#""application/js\u00adon""#, r"application/js\u{ad}on"),
        (r#""application/json\u2060""#, r"application/json\u{2060}"),
        (r#""application/json\u200e""#, r"application/json\u{200e}"),
        (r#""application/json\u0301""#, r"application/json\u{301}"),
    ] {
        assert_refused_as_no_media_type(key, object, shown);
    }
}

/// A space and a tab are the optional white space before the type and after the
/// subtype, so a key with one after its subtype loads and is sent as the document
/// spells it.
#[test]
fn a_space_and_a_tab_after_the_subtype_are_no_fault() {
    let content = "          \"application/json \\t; charset=utf-8\":\n            \
                   schema: { type: object, properties: { a: { type: string } } }\n";

    let doc = Document::load(&posting(content), &[]).expect("the white space is optional");

    assert_eq!(
        content_type(&doc, "upload", Values::new().json(json!({ "a": "x" }))),
        "application/json \t; charset=utf-8"
    );
}

/// A space and a tab at an end of the key are optional white space too, and a
/// field value must not start or end with one (RFC 9113, 8.2.1), so the request
/// goes out without them, under a JSON key and a key sent verbatim alike. The
/// inputs are `<SP>application/json`, `application/json<TAB>` and
/// `<TAB>text/plain; charset=utf-8<SP>`.
#[test]
fn a_space_and_a_tab_at_an_end_of_the_key_are_not_sent() {
    let object = "{ type: object, properties: { a: { type: string } } }";
    for (key, schema, sent) in [
        (r#"" application/json""#, object, "application/json"),
        (r#""application/json\t""#, object, "application/json"),
        (
            r#""\ttext/plain; charset=utf-8 ""#,
            "{ type: string }",
            "text/plain; charset=utf-8",
        ),
    ] {
        let content = format!("          {key}:\n            schema: {schema}\n");

        let doc = Document::load(&posting(&content), &[]).expect(key);

        let values = if schema == object {
            Values::new().json(json!({ "a": "x" }))
        } else {
            Values::new().raw(vec![1])
        };
        assert_eq!(content_type(&doc, "upload", values), sent, "{key}");
    }
}

/// The words spell the key as the document does, so the target of the Overlay
/// that repairs it can match: a quote and a backslash are printed as they stand,
/// and only a character that does not show is written as its code point. The
/// inputs are `form-data; name="file"`, `form\data`, `application/jsoné` and
/// `application/json` with a replacement character after it.
#[test]
fn the_words_spell_the_key_as_the_document_does() {
    for (key, spelled) in [
        (r#"'form-data; name="file"'"#, r#"`form-data; name="file"`"#),
        (r"'form\data'", r"`form\data`"),
        ("'application/json\u{e9}'", "`application/json\u{e9}`"),
        ("'application/json\u{fffd}'", "`application/json\u{fffd}`"),
    ] {
        let content = format!("          {key}:\n            schema: {{ type: string }}\n");

        let error = Document::load(&posting(&content), &[]).expect_err(key);

        assert!(
            matches!(&error, LoadError::MediaType { .. }),
            "{key}: {error:?}"
        );
        assert!(error.to_string().contains(spelled), "{error}");
    }
}

/// A `multipart/form-data` key is never sent: the body is assembled with a
/// `Content-Type` of its own, boundary and all. So it is not asked what a header
/// carries, and a key that no header carries loads and the request goes out as
/// `multipart/form-data; boundary=…`. The inputs are
/// `multipart/form-data; x="a<U+0001>b"` and `multipart/form-data<NBSP>`.
#[test]
fn a_multipart_key_is_not_asked_what_a_header_carries() {
    let schema = "{ type: object, properties: { note: { type: string } } }";
    for key in [
        r#""multipart/form-data; x=\"a\u0001b\"""#,
        r#""multipart/form-data\u00a0""#,
    ] {
        let content = format!("          {key}:\n            schema: {schema}\n");

        let doc = Document::load(&posting(&content), &[]).expect(key);

        assert_eq!(picked(&doc, "upload"), ("multipart", None), "{key}");
        let values = Values::new().multipart(vec![Part::text("note", "hello")]);
        let sent = content_type(&doc, "upload", values);
        assert!(
            sent.starts_with("multipart/form-data; boundary="),
            "{key}: {sent}"
        );
    }
}
