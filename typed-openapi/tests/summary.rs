//! What the reduction says it did, against a document whose shape is known by
//! construction.
//!
//! The fixture below is written for this file rather than taken from
//! `tests/fixtures/`, because every assertion here is a count and a count is
//! only worth making against a document a reader can tally by eye. It is six
//! operations in two groups, and every row of the summary has something in it:
//! a read and five writes, two gates with a shared one between them, two
//! operations the document asks no body of, all three shapes of body that goes
//! out whole, and one parameter this CLI cannot spell.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use typed_openapi::{Document, Summary, Unsupported};

/// Six operations, counted in the module header.
///
/// `session` is `in: cookie` and optional, which is a parameter carried without
/// a flag rather than a refusal — a required one would be a `LoadError`, so
/// there is no document that both holds one and reduces.
const COUNTED: &str = r#"
openapi: 3.0.3
info: { title: Counted, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /widgets:
    get:
      operationId: listWidgets
      parameters:
        - name: session
          in: cookie
          required: false
          schema: { type: string }
      responses: { "200": { description: OK } }
    post:
      operationId: createWidget
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                name: { type: string }
      responses: { "201": { description: Created } }
  /widgets/{id}/scrap:
    post:
      operationId: scrapWidget
      x-cli-gates: [scrap]
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses: { "200": { description: OK } }
  /widgets/{id}/ship:
    post:
      operationId: shipWidget
      x-cli-gates: [scrap, notify]
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                address:
                  type: object
                  properties:
                    city: { type: string }
      responses: { "202": { description: Accepted } }
  /crates:
    post:
      operationId: uploadCrate
      requestBody:
        required: true
        content:
          application/pdf:
            schema: { type: string, format: binary }
      responses: { "201": { description: Created } }
  /crates/parts:
    post:
      operationId: uploadParts
      requestBody:
        required: true
        content:
          multipart/form-data:
            schema:
              type: object
              properties:
                file: { type: string, format: binary }
      responses: { "201": { description: Created } }
"#;

/// One more operation, arriving the way one really arrives: an Overlay the
/// adopter wrote, or a vendor revision they fetched. It writes, and it stands
/// behind a gate two operations already name.
const ONE_MORE: &str = r#"
overlay: 1.1.0
info: { title: One more operation, version: "1.0" }
extends: counted.yaml
actions:
  - target: $.paths
    description: The operation the first document does not describe.
    update:
      /widgets/{id}/recycle:
        post:
          operationId: recycleWidget
          x-cli-gates: [scrap]
          parameters:
            - { name: id, in: path, required: true, schema: { type: integer } }
          responses: { "200": { description: OK } }
"#;

/// A document with nothing in its exception lists: no gate, every parameter
/// reachable. The empty case is worth its own fixture, because a page that says
/// nothing where there is nothing to say is the page an adopter quotes.
const PLAIN: &str = r#"
openapi: 3.0.3
info: { title: Plain, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /widgets:
    get:
      operationId: listWidgets
      responses: { "200": { description: OK } }
"#;

fn summary(document: &str, overlays: &[&str]) -> Summary {
    Document::load(document, overlays)
        .expect("the fixture is a document")
        .summary()
}

/// Every count on the list, against the document in the module header.
#[test]
fn every_count_is_the_documents_own() {
    let summary = summary(COUNTED, &[]);

    assert_eq!(summary.operations(), 6);
    let groups: Vec<&str> = summary
        .groups()
        .iter()
        .map(typed_openapi::CommandName::as_str)
        .collect();
    assert_eq!(groups, ["widgets", "crates"], "in document order");

    assert_eq!(summary.reads(), 1, "only `listWidgets` is a safe method");
    assert_eq!(summary.writes(), 5);

    assert_eq!(summary.behind("scrap"), 2, "`scrapWidget` and `shipWidget`");
    assert_eq!(summary.behind("notify"), 1, "`shipWidget` alone");
    assert_eq!(summary.gates().len(), 2, "and no third word");
    assert_eq!(
        summary.behind("commit"),
        0,
        "the write gate is not one of the named ones"
    );

    assert_eq!(
        summary.bodiless(),
        2,
        "`listWidgets` and `scrapWidget` are asked for no body"
    );
    assert_eq!(
        summary.whole_bodies(),
        3,
        "a nested JSON body, a PDF and a multipart upload"
    );

    assert_eq!(summary.unreachable().len(), 1);
    let carried = summary
        .unreachable()
        .first()
        .expect("the one parameter this CLI cannot spell");
    assert_eq!(carried.operation(), "listWidgets");
    assert_eq!(carried.parameter(), "session");
    assert_eq!(carried.why(), &Unsupported::Cookie);
}

/// The whole claim: a number that describes the document moves when the
/// document does. An Overlay adds one gated write, and every count it is part
/// of follows it — which is what an adopter gets instead of a number that was
/// true when they typed it.
#[test]
fn a_count_follows_the_document_it_counts() {
    let before = summary(COUNTED, &[]);
    let after = summary(COUNTED, &[ONE_MORE]);

    assert_eq!(before.operations(), 6);
    assert_eq!(after.operations(), 7, "the Overlay adds one");

    assert_eq!(before.writes(), 5);
    assert_eq!(after.writes(), 6, "and it writes");

    assert_eq!(before.behind("scrap"), 2);
    assert_eq!(after.behind("scrap"), 3, "and it stands behind `scrap`");

    assert_eq!(before.bodiless(), 2);
    assert_eq!(after.bodiless(), 3, "and it is asked for no body");

    assert_eq!(before.reads(), after.reads(), "nothing else moved");
    assert_eq!(before.groups(), after.groups());
    assert_eq!(before.unreachable(), after.unreachable());

    assert_ne!(
        before.to_string(),
        after.to_string(),
        "and the page a reader quotes moved with them"
    );
}

/// An operation is a read or a write and never both or neither, so the two
/// counts are the operation count between them. A summary that failed this
/// would be two tallies of different things wearing one page.
#[test]
fn the_reads_and_the_writes_are_every_operation() {
    for (document, overlays) in [(COUNTED, &[][..]), (COUNTED, &[ONE_MORE]), (PLAIN, &[])] {
        let summary = summary(document, overlays);
        assert_eq!(
            summary.reads() + summary.writes(),
            summary.operations(),
            "{summary}"
        );
    }
}

/// The reduction a shipped binary meets is the blob, and it enables no reader.
/// Counting off that has to give the same answer as counting off the document,
/// or the page a bless step wrote describes something the binary does not have.
#[test]
fn a_summary_off_the_blob_is_the_summary_off_the_document() {
    let document = Document::load(COUNTED, &[ONE_MORE]).expect("the fixture is a document");
    let blob = document.to_blob().expect("the reduction encodes");
    let shipped = Document::from_blob(&blob).expect("and decodes");

    assert_eq!(document.summary(), shipped.summary());
    assert_eq!(
        document.summary().to_string(),
        shipped.summary().to_string()
    );
}

/// The page names what it counted, so a reader can re-count it. That is the
/// whole difference between a measurement and a number: every group, every
/// gate and every parameter without a flag is on the page beside its tally.
#[test]
fn the_page_names_what_it_counted() {
    let page = summary(COUNTED, &[]).to_string();

    for named in [
        "`widgets`",
        "`crates`",
        "`--scrap`",
        "`--notify`",
        "`listWidgets`",
        "`session`",
    ] {
        assert!(
            page.contains(named),
            "the page does not name {named}:\n{page}"
        );
    }
    assert!(
        page.contains(&Unsupported::Cookie.to_string()),
        "the page does not say why the parameter has no flag:\n{page}"
    );
    assert!(
        page.contains("| operations | 6 |"),
        "the page does not carry the tally:\n{page}"
    );
}

/// Where there is nothing to list, the page says so in a sentence rather than
/// printing a table with no rows — and it still carries every count, so a
/// quoting page has the same rows to quote whatever the document holds.
#[test]
fn an_empty_list_is_a_sentence_and_not_an_empty_table() {
    let summary = summary(PLAIN, &[]);
    assert_eq!(summary.operations(), 1);
    assert_eq!(summary.gates().len(), 0);
    assert_eq!(summary.unreachable().len(), 0);

    let page = summary.to_string();
    assert!(page.contains("No operation names a gate"), "{page}");
    assert!(
        page.contains("every parameter the document declares reaches the request"),
        "{page}"
    );
    assert!(page.contains("| operations | 1 |"), "{page}");
}

/// Rendering is a pure function of the value, so the same summary renders the
/// same bytes — which is what lets a bless step commit the page and a gate fail
/// on a diff.
#[test]
fn one_summary_renders_one_page() {
    let summary = summary(COUNTED, &[ONE_MORE]);
    assert_eq!(summary.to_string(), summary.to_string());
    assert_eq!(summary.to_string(), summary.clone().to_string());
}
