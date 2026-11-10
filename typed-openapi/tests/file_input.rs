//! File and multipart flags reach the wire through the public command tree.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::fs;
use std::path::PathBuf;

use clap::Command;
use http::{Method, StatusCode};
use serde_json::json;
use typed_openapi::tree::{self, Outcome};
use typed_openapi::{Document, HttpRequest, Recorder};

fn document() -> Document {
    Document::load(
        include_str!("fixtures/toy.yaml"),
        &[
            include_str!("fixtures/corrections.yaml"),
            include_str!("fixtures/cli.yaml"),
        ],
    )
    .expect("the library's fixture and overlays load")
}

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("file-input");
    fs::create_dir_all(&directory).expect("the scratch directory");
    let path = directory.join(name);
    fs::write(&path, bytes).expect("the input file");
    path
}

fn dispatch(group: &str, path: &str, flags: &[&str]) -> HttpRequest {
    let document = document();
    let matches = Command::new("accounting")
        .subcommands(tree::commands(&document))
        .try_get_matches_from(
            ["accounting", group, "create"]
                .into_iter()
                .chain(flags.iter().copied()),
        )
        .expect("the command line parses");
    let recorder = Recorder::new().strict().answering_route(
        Method::POST,
        path,
        StatusCode::CREATED,
        &json!({"id": 19}),
    );

    let outcome = tree::dispatch(&document, document.base(), &recorder, &matches)
        .expect("the confirmed command dispatches");

    let Outcome::Sent(response) = outcome else {
        panic!("the confirmed write must send");
    };
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.body(), br#"{"id":19}"#);
    assert_eq!(recorder.unused(), 0, "the scripted answer was consumed");
    let mut requests = recorder.take();
    assert_eq!(requests.len(), 1, "exactly one request went out");
    let request = requests.pop().expect("one recorded request");
    assert_eq!(request.method(), Method::POST);
    assert_eq!(request.uri().path(), path);
    request
}

#[test]
fn json_body_file_reaches_dispatch_as_a_nested_json_body() {
    let body = br#"{"name":"Acme Ltd","address":{"street":"1 Example Way","city":"Vienna"}}"#;
    let path = scratch("contact.json", body);

    let request = dispatch(
        "contacts",
        "/contacts",
        &[
            "--json-body",
            path.to_str().expect("UTF-8 path"),
            "--commit",
        ],
    );

    assert_eq!(
        request
            .headers()
            .get("content-type")
            .expect("the media type"),
        "application/json"
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(request.body()).expect("JSON on the wire"),
        json!({"name": "Acme Ltd", "address": {"street": "1 Example Way", "city": "Vienna"}})
    );
}

#[test]
fn file_and_field_flags_reach_dispatch_as_repeated_multipart_parts() {
    let pdf = scratch("invoice.pdf", b"%PDF\0\xff\r\n");
    let scan = scratch("scan.bin", &[0, 1, 254, 255]);
    let file = format!("file={}", pdf.display());
    let attachment = format!("scan={}", scan.display());

    let request = dispatch(
        "documents-multipart",
        "/documents-multipart",
        &[
            "--file",
            &file,
            "--field",
            "note=first=part",
            "--file",
            &attachment,
            "--field",
            "note=second",
            "--commit",
        ],
    );

    let boundary = request
        .headers()
        .get("content-type")
        .expect("the multipart media type")
        .to_str()
        .expect("an ASCII media type")
        .strip_prefix("multipart/form-data; boundary=")
        .expect("the header declares the body's boundary");
    let mut expected = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nfirst=part\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nsecond\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"invoice.pdf\"\r\n\
         Content-Type: application/pdf\r\n\r\n"
    ).into_bytes();
    expected.extend_from_slice(b"%PDF\0\xff\r\n");
    expected.extend_from_slice(format!(
        "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"scan\"; filename=\"scan.bin\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    ).as_bytes());
    expected.extend_from_slice(&[0, 1, 254, 255]);
    expected.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    assert_eq!(request.body(), &expected);
}
