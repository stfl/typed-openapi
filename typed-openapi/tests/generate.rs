//! What a bless step writes, run by the crate that ships the bless step.
//!
//! The fixtures under `tests/fixtures/` are this crate's own, not the example
//! adoption's. `examples/toy/spec/` holds a document with the same content
//! today and a different owner: there it is the vendor's, and the example is
//! free to evolve it. Pointing these tests at that copy would let a change to
//! the example break the library.
//!
//! Everything is written under `CARGO_TARGET_TMPDIR`, which cargo gives an
//! integration test for exactly this, so a run leaves the source tree alone.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as _;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use quote::ToTokens as _;
use serde_json::json;
use typed_openapi::generate::{GenerateError, Settings};
use typed_openapi::overlay::{self, OverlayError, Place};
use typed_openapi::{Document, Effect, LoadError};

const TOY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/toy.yaml");
const CORRECTIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/corrections.yaml"
);
const CLI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/cli.yaml");
const OPTIONAL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/optional.yaml");
const HOSTILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hostile.yaml");

/// A directory of this test's own, emptied first so that nothing it asserts
/// about can be left over from a previous run.
fn out(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    drop(std::fs::remove_dir_all(&dir));
    dir
}

/// The vendor's document and both layers over it, in the order a bless step
/// applies them.
fn layered() -> Settings {
    Settings::new(TOY).overlay(CORRECTIONS).overlay(CLI)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("a file the bless step reported writing")
}

/// A document this test owns outright, written where the output goes.
///
/// The fixtures are copies of the example adoption's files and stay that way; a
/// test about one generator behaviour wants the smallest document that shows
/// it, beside the assertion, where a reader sees both at once.
fn wrote(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(dir).expect("a directory to write the document into");
    std::fs::write(&path, contents).expect("a document this test writes");
    path
}

/// A named schema that states a `pattern`: typify turns it into a newtype whose
/// `FromStr` enforces the rule.
const PATTERNED: &str = r##"
openapi: 3.0.3
info: { title: Patterned, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things/{sku}:
    get:
      operationId: getThing
      parameters:
        - name: sku
          in: path
          required: true
          schema: { $ref: "#/components/schemas/Sku" }
      responses:
        "200": { description: OK }
components:
  schemas:
    Sku:
      type: string
      pattern: "^[A-Z]{3}-[0-9]{4}$"
"##;

/// A newtype that came back from an API and cannot be written to a `format!` is
/// half a type. typify emits `Deref`, `FromStr` and two `TryFrom`s and stops
/// there, so the missing half is this crate's to emit — and the engine the
/// emitted rule runs on is reached through this crate, so the crate holding the
/// generated code declares no dependency on it.
#[test]
fn a_generated_newtype_over_a_string_prints_and_needs_no_engine_of_its_own() {
    let dir = out("patterned");
    let document = wrote(&dir, "sku.yaml", PATTERNED);
    Settings::new(&document)
        .write_to(&dir)
        .expect("the document generates");

    let types = read(&dir.join("src/types.rs"));
    assert!(
        types.contains("impl ::std::fmt::Display for Sku"),
        "a generated string newtype cannot be written to a `format!`:\n{types}"
    );
    assert!(
        types.contains("::typed_openapi::regress::Regex"),
        "the generated pattern check does not reach the engine through this crate:\n{types}"
    );
    assert!(
        !types.contains("<::regress::Regex>"),
        "the generated code names an engine the crate holding it would have to \
         depend on:\n{types}"
    );
}

/// A path item that states `id` as a string, and an operation that overrides it
/// with an integer. The path item's `key` is in the query, and the operation's
/// `key` is in a header, so they are two parameters that share a name, and the
/// header's has no argument. A boolean `key` in the header is what an argument
/// typed by the wrong parameter would show.
const OVERRIDDEN: &str = r#"
openapi: 3.0.3
info: { title: Overridden, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things/{id}:
    parameters:
      - { name: id, in: path, required: true, schema: { type: string } }
      - { name: key, in: query, schema: { type: string } }
    get:
      operationId: getThing
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer, format: int64 } }
        - { name: key, in: header, schema: { type: boolean } }
      responses:
        "200": { description: OK }
"#;

/// A path-level parameter that an operation overrides is one argument, typed by
/// the operation's entry, and a second parameter with a name that the first
/// already has is a note and no argument: a value is filed under a name, so the
/// wrapper has one argument for it.
#[test]
fn an_overridden_parameter_is_one_argument_typed_by_the_operations_entry() {
    let dir = out("overridden");
    let document = wrote(&dir, "overridden.yaml", OVERRIDDEN);
    Settings::new(&document)
        .write_to(&dir)
        .expect("the document generates");

    let ops = read(&dir.join("src/ops.rs"));

    assert!(
        dense(&ops).contains("pubfnget_thing(&self,id:i64,key:::std::option::Option<&str>)"),
        "the wrapper does not take the operation's `id` and the path item's `key`:\n{ops}"
    );
    assert!(
        ops.contains(
            "The document's `key` parameter is not an argument: it is sharing its name with the \
             `query` parameter."
        ),
        "the wrapper does not say why the header `key` is not an argument:\n{ops}"
    );
}

/// A body property and a query parameter of one integer format.
const FORMATTED_INTEGERS: &str = r#"
openapi: 3.0.3
info: { title: Formatted, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /counts:
    post:
      operationId: createCount
      parameters:
        - { name: limit, in: query, schema: { type: integer, format: int32 } }
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [count]
              properties:
                count: { type: integer, format: int32 }
                total: { type: integer, minimum: 0 }
      responses:
        "200": { description: OK }
"#;

/// `docs/validation.md` says where the two roads part for an integer format. The
/// flag reads an `i64` whatever the format says, because a format names a rule
/// and is not one. typify gives a body property the type of its format, so the
/// typed road cannot hold a value past it, and a parameter is an `i64` argument.
#[test]
fn an_integer_format_types_a_body_property_and_leaves_a_parameter_an_i64() {
    let dir = out("formatted-integers");
    let document = wrote(&dir, "counts.yaml", FORMATTED_INTEGERS);
    Settings::new(&document)
        .write_to(&dir)
        .expect("the document generates");

    let types = read(&dir.join("src/types.rs"));
    let ops = read(&dir.join("src/ops.rs"));

    assert!(
        types.contains("pub count: i32"),
        "a body property of `format: int32` is not an `i32`:\n{types}"
    );
    assert!(
        types.contains("pub total: ::std::option::Option<u64>"),
        "a body property with `minimum: 0` is not a `u64`:\n{types}"
    );
    assert!(
        ops.contains("limit: ::std::option::Option<i64>"),
        "a parameter of `format: int32` is not an `i64` argument:\n{ops}"
    );
}

/// A body declared under three keys, the first that names JSON being
/// `application/vnd.api+json`, and a success response under a range that ends in
/// `+json`.
const SEVERAL_KEYS: &str = r#"
openapi: 3.0.3
info: { title: Several, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things:
    post:
      operationId: createThing
      requestBody:
        required: true
        content:
          text/csv: { schema: { type: string } }
          application/vnd.api+json:
            schema: { type: object, properties: { first: { type: string } } }
          application/json:
            schema: { type: object, properties: { second: { type: string } } }
      responses:
        "200":
          description: OK
          content:
            application/*+json:
              schema: { type: object, properties: { answer: { type: string } } }
"#;

/// `docs/validation.md` says which key a body is sent under. The type of the
/// wrapper's argument is read from that entry, so the argument holds a caller to
/// the schema of the body that goes out and not to another entry's.
#[test]
fn the_argument_is_typed_by_the_entry_the_body_is_sent_under() {
    let dir = out("several-keys");
    let document = wrote(&dir, "several.yaml", SEVERAL_KEYS);
    Settings::new(&document)
        .write_to(&dir)
        .expect("the document generates");

    let types = read(&dir.join("src/types.rs"));
    let ops = read(&dir.join("src/ops.rs"));

    assert!(
        types.contains("pub first:"),
        "the body is not typed by the first key that names JSON:\n{types}"
    );
    assert!(
        !types.contains("pub second:"),
        "a key beside the one that is sent is typed:\n{types}"
    );
    assert!(
        ops.contains("body: &crate::types::CreateThingBody"),
        "the wrapper takes no body of the type that was generated:\n{ops}"
    );
}

/// What the wrapper of one body that the document leaves optional shows.
struct OptionalBody {
    what: &'static str,
    wrapper: &'static str,
    argument: &'static str,
    builder: String,
    note: &'static str,
}

/// One body of each kind in `tests/fixtures/optional.yaml` that the document
/// leaves optional: a flat body with a required property, a flat body with none,
/// a body that goes whole, a body sent verbatim and a multipart body.
fn optional_bodies() -> [OptionalBody; 5] {
    // The wrapper names its operation, so that a body that does not serialise
    // says whose it was.
    let json = |id: &str| {
        format!(
            ".body(body.map(|body|crate::to_json(\"{id}\",body)).transpose()?.map(typed_openapi::Payload::Json))"
        )
    };
    let note = "`body` is optional: with `None` the request carries no body.";
    [
        OptionalBody {
            what: "a flat body with a required property",
            wrapper: "create_reminder",
            argument: "body:::std::option::Option<&crate::types::CreateReminderBody>",
            builder: json("createReminder"),
            note,
        },
        OptionalBody {
            what: "a flat body with no required property",
            wrapper: "create_note",
            argument: "body:::std::option::Option<&crate::types::CreateNoteBody>",
            builder: json("createNote"),
            note,
        },
        OptionalBody {
            what: "a body that goes whole",
            wrapper: "create_export",
            argument: "body:::std::option::Option<&crate::types::CreateExportBody>",
            builder: json("createExport"),
            note,
        },
        OptionalBody {
            what: "a body sent verbatim",
            wrapper: "upload_attachment",
            argument: "body:::std::option::Option<::std::vec::Vec<u8>>",
            builder: ".body(body.map(typed_openapi::Payload::Raw))".to_owned(),
            note,
        },
        OptionalBody {
            what: "a multipart body",
            wrapper: "upload_scan",
            argument: "parts:::std::option::Option<::std::vec::Vec<Part>>",
            builder: ".body(parts.map(typed_openapi::Payload::Multipart))".to_owned(),
            note: "`parts` is optional: with `None` the request carries no body.",
        },
    ]
}

/// A body the document leaves optional is an `Option` argument, of every kind,
/// and `None` adds no body to the request: the command line sends none when its
/// operator gives none, and the wrapper is the same call. `A7/docs/optbody.yaml`
/// is `createReminder`, whose wrapper takes `Option<&T>`.
#[test]
fn an_optional_body_is_an_option_argument_that_none_leaves_out() {
    let dir = out("optional-bodies");
    Settings::new(OPTIONAL)
        .write_to(&dir)
        .expect("the document generates");
    let ops = read(&dir.join("src/ops.rs"));

    for case in optional_bodies() {
        let text = wrapper(&ops, case.wrapper);
        let dense = dense(&text);
        let what = case.what;
        assert!(
            dense.contains(case.argument),
            "{what} is not optional:\n{text}"
        );
        assert!(
            dense.contains(&case.builder),
            "{what} is not left out by `None`:\n{text}"
        );
        assert!(
            text.contains(case.note),
            "the wrapper of {what} does not say what `None` sends:\n{text}"
        );
    }
}

/// A body the document requires is a plain argument, and its wrapper says
/// nothing of `None`: no signature of an adoption whose bodies are all required
/// has an `Option` in it. The toy's are, and so is the fixture's `createLabel`,
/// in which no property is required.
#[test]
fn a_required_body_is_a_plain_argument() {
    let dir = out("required-bodies");
    layered().write_to(&dir).expect("the fixtures generate");
    let toy = read(&dir.join("src/ops.rs"));
    let dir = out("required-flat-body");
    Settings::new(OPTIONAL)
        .write_to(&dir)
        .expect("the document generates");
    let optional = read(&dir.join("src/ops.rs"));

    for (what, ops, wrapper_name, argument, builder) in [
        (
            "a flat body",
            &toy,
            "create_voucher",
            "body:&crate::types::Voucher",
            ".json(crate::to_json(\"createVoucher\",body)?)",
        ),
        (
            "a body sent verbatim",
            &toy,
            "upload_document",
            "body:::std::vec::Vec<u8>",
            ".raw(body)",
        ),
        (
            "a multipart body",
            &toy,
            "upload_document_multipart",
            "parts:::std::vec::Vec<Part>",
            ".multipart(parts)",
        ),
        (
            "a flat body with no required property",
            &optional,
            "create_label",
            "body:&crate::types::CreateLabelBody",
            ".json(crate::to_json(\"createLabel\",body)?)",
        ),
        (
            "a flat body with a required property",
            &optional,
            "create_tag",
            "body:&crate::types::CreateTagBody",
            ".json(crate::to_json(\"createTag\",body)?)",
        ),
    ] {
        let text = wrapper(ops, wrapper_name);
        let dense = dense(&text);
        assert!(
            dense.contains(argument),
            "{what} is not a plain argument:\n{text}"
        );
        assert!(
            dense.contains(builder),
            "{what} is not passed to its own `Values` call:\n{text}"
        );
        assert!(
            !text.contains("is optional"),
            "the wrapper of {what} calls it optional:\n{text}"
        );
    }
}

/// A response is read for its type by the rule that reads a `+json` suffix, and
/// a range that ends in one names JSON by it, so the wrapper returns the type of
/// that schema and not `NoContent`.
#[test]
fn a_response_under_a_range_that_ends_in_json_is_typed() {
    let dir = out("several-keys-response");
    let document = wrote(&dir, "several.yaml", SEVERAL_KEYS);
    Settings::new(&document)
        .write_to(&dir)
        .expect("the document generates");

    let types = read(&dir.join("src/types.rs"));
    let ops = read(&dir.join("src/ops.rs"));

    assert!(
        types.contains("pub answer:"),
        "the response is not typed:\n{types}"
    );
    assert!(
        ops.contains("Call<'_, crate::types::CreateThingResponse>"),
        "the wrapper does not return the type of the response:\n{ops}"
    );
}

/// A `{id}` path with the path item's parameters in `ITEM`, an operation that
/// names `#/components/parameters/Id`, and that component in `COMPONENT`.
const REFERRED: &str = r##"
openapi: 3.0.3
info: { title: Referred, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things/{id}:
    parameters: ITEM
    get:
      operationId: getThing
      parameters:
        - $ref: "#/components/parameters/Id"
      responses:
        "200": { description: OK }
components:
  parameters:
    Id: COMPONENT
"##;

/// The entry that wins may be a reference, and it types the argument all the
/// same: the path item's entry that it replaced does not. A parameter that the
/// document gives by reference alone is declared too.
#[test]
fn an_argument_is_typed_by_the_winning_entry_when_the_operation_names_it_by_reference() {
    let text = "{ name: id, in: path, required: true, schema: { type: string } }";
    let integer = "{ name: id, in: path, required: true, schema: { type: integer, format: int64, minimum: 1 } }";
    for (name, item, component, wrapper) in [
        (
            "over-text",
            &format!("[{text}]"),
            integer,
            "pub fn get_thing(&self, id: i64)",
        ),
        (
            "over-integer",
            &format!("[{integer}]"),
            text,
            "pub fn get_thing(&self, id: &str)",
        ),
        (
            "alone",
            &"[]".to_owned(),
            integer,
            "pub fn get_thing(&self, id: i64)",
        ),
    ] {
        let dir = out(&format!("referred-{name}"));
        let document = wrote(
            &dir,
            "referred.yaml",
            &REFERRED
                .replace("ITEM", item)
                .replace("COMPONENT", component),
        );
        Settings::new(&document)
            .write_to(&dir)
            .unwrap_or_else(|error| panic!("{name}: the document generates: {error}"));

        let ops = read(&dir.join("src/ops.rs"));

        assert!(
            ops.contains(wrapper),
            "{name}: the wrapper is not `{wrapper}`:\n{ops}"
        );
    }
}

/// The artefacts are one Overlay application seen several ways, so the
/// reduction a binary loads has to be the reduction of the document that was
/// committed beside it. Reducing the written YAML again is the only check that
/// they left the generator together.
#[test]
fn the_written_model_is_the_written_documents_reduction() {
    let dir = out("agree");
    let written = layered()
        .summary_page("src/summary.md")
        .write_to(&dir)
        .expect("the fixtures generate");

    assert_eq!(
        written,
        vec![
            dir.join("spec/toy.overlaid.yaml"),
            dir.join("src/types.rs"),
            dir.join("src/ops.rs"),
            dir.join("src/model.postcard"),
            dir.join("src/summary.md"),
        ],
        "the artefacts, in the layout a generated crate embeds them by"
    );

    let blob = std::fs::read(&written[3]).expect("the reduced model");
    let shipped = Document::from_blob(&blob).expect("the written blob is a reduction");
    let fresh =
        Document::load(&read(&written[0]), &[]).expect("the written document is a document");
    assert!(
        shipped == fresh,
        "the reduction a binary reads is not the written document's"
    );
}

/// The confirmation word an adopter chooses is the word the blob confirms with,
/// on the document and on every operation. A shipped binary reads the blob and
/// nothing else, so a bless step that dropped the word would write a blob that
/// confirms with `commit` while the adopter's help and scripts say `yes`.
#[test]
fn the_confirmation_word_an_adopter_chooses_is_the_word_the_blob_confirms_with() {
    let chosen = layered()
        .commit_word("yes")
        .expect("`yes` is a word nobody spends");
    for (settings, word) in [(layered(), "commit"), (chosen, "yes")] {
        let dir = out(&format!("confirms-{word}"));
        let written = settings.write_to(&dir).expect("the fixtures generate");
        let blob = std::fs::read(&written[3]).expect("the reduced model");
        let shipped = Document::from_blob(&blob).expect("the written blob is a reduction");

        assert_eq!(shipped.commit(), word, "the document");
        let writes: Vec<_> = shipped
            .iter()
            .filter(|op| op.effect() == Effect::Write)
            .collect();
        assert!(!writes.is_empty(), "the fixtures have writes to confirm");
        for op in writes {
            assert_eq!(op.commit(), word, "{}", op.id());
        }
    }
}

/// The page is the written model's own count, and it says where that count
/// came from.
///
/// Both halves matter. A page whose numbers are not the blob's would send an
/// adopter's prose one way while their binary went another; a page of bare
/// numbers would be indistinguishable from the remembered ones it exists to
/// replace, and a reader who cannot tell those apart cannot act on either.
///
/// The page is held to the rendering twice over, because comparing two
/// renderings is satisfied by two renderings that state nothing: once as the
/// whole of what follows the header, and once by reading a count back out of
/// the page and holding it to the accessor the same `Summary` answers — which
/// is the use an adopter quoting a row makes of it.
#[test]
fn the_summary_page_is_the_written_models_count_and_says_where_it_came_from() {
    let dir = out("summary");
    let written = layered()
        .regenerated_by("just bless")
        .summary_page("src/summary.md")
        .write_to(&dir)
        .expect("the fixtures generate");
    let page = read(&written[4]);

    // The provenance: what counts them again, and what they were counted from.
    assert!(
        page.starts_with("<!--\nGenerated by `just bless` from "),
        "the page does not name the command that counts it again:\n{page}"
    );
    for input in ["toy.yaml", "corrections.yaml", "cli.yaml"] {
        assert!(
            page.contains(input),
            "the page does not name {input}, which it was counted from:\n{page}"
        );
    }

    let blob = std::fs::read(&written[3]).expect("the reduced model");
    let shipped = Document::from_blob(&blob).expect("the written blob is a reduction");
    let counted = shipped.summary();

    // Below that header the page is the written model's own rendering, with
    // nothing the generator added and nothing it dropped.
    let (_, body) = page
        .split_once("-->\n\n")
        .expect("the page carries the generated header");
    assert_eq!(
        body,
        counted.to_string(),
        "the page below its header is not the written model's rendered count:\n{page}"
    );

    // And the rendering states the counts, so that equality cannot hold by
    // both sides saying nothing.
    assert!(
        counted.operations() > 0,
        "the fixtures reduce to no operations, so the page has nothing to count"
    );
    assert!(
        body.contains(&format!("| operations | {} |", counted.operations())),
        "the page does not state the operation count the model holds:\n{page}"
    );
}

/// An adoption that quotes no count gets no page, and is not told it got one.
///
/// The page is Markdown in a Rust source directory, committed, and listed in
/// whatever check holds a bless step's output to the tree — a whole obligation,
/// which is why only an adopter can take it on. Both halves are asserted: a
/// file that appears anyway is one nobody asked for, and a path in the answer
/// that names no file would send a caller iterating the answer to copy, list or
/// check a file that is not there.
#[test]
fn a_bless_step_that_asks_for_no_summary_page_writes_none() {
    let dir = out("unasked");
    let written = layered().write_to(&dir).expect("the fixtures generate");

    assert_eq!(
        written,
        vec![
            dir.join("spec/toy.overlaid.yaml"),
            dir.join("src/types.rs"),
            dir.join("src/ops.rs"),
            dir.join("src/model.postcard"),
        ],
        "the answer names an artefact the caller did not ask for"
    );
    for path in &written {
        assert!(
            path.exists(),
            "{} was reported and not written",
            path.display()
        );
    }
    assert!(
        !dir.join("src/summary.md").exists(),
        "a page nobody asked for landed in the adopter's source directory"
    );
}

/// Where the page goes is the adopter's to say, because nothing the generator
/// writes reads it.
///
/// The other four embed each other by relative path, so `write_to` owns their
/// layout; a summary is reached only by what the adoption points at it, and an
/// adoption whose prose lives in `docs/` has no reason to keep the page under
/// `src/`. A relative path lands under the directory the rest do, and an
/// absolute one lands where it says.
#[test]
fn the_summary_page_lands_where_the_adoption_asks_for_it() {
    let dir = out("elsewhere");
    let counted = out("counted").join("deep/api-summary.md");

    let relative = layered()
        .summary_page("docs/how-big.md")
        .write_to(&dir)
        .expect("the fixtures generate");
    assert_eq!(
        relative.last(),
        Some(&dir.join("docs/how-big.md")),
        "a relative path did not land under the directory the rest did"
    );
    assert!(
        !dir.join("src/summary.md").exists(),
        "the page also landed at a path this crate chose"
    );

    let absolute = layered()
        .summary_page(&counted)
        .write_to(&dir)
        .expect("the fixtures generate");
    assert_eq!(
        absolute.last(),
        Some(&counted),
        "an absolute path did not land where it says"
    );
    assert!(
        read(&counted).contains("# What the reduction found"),
        "the page written outside the crate directory is not the summary"
    );
}

/// A bless step is run again on every vendor revision and its output is
/// committed, so a generator that wrote different bytes for one document would
/// make every diff unreadable — and would make `just blessed`, which fails on a
/// non-empty diff, fail on nothing at all.
#[test]
fn two_bless_runs_over_one_document_write_the_same_bytes() {
    let first = out("twice-first");
    let second = out("twice-second");
    let written = layered().write_to(&first).expect("the fixtures generate");
    layered().write_to(&second).expect("and generate again");

    // What the comparison below runs over, named rather than counted: a loop
    // over nothing, or over the wrong four files, compares nothing and passes.
    assert_eq!(
        written,
        vec![
            first.join("spec/toy.overlaid.yaml"),
            first.join("src/types.rs"),
            first.join("src/ops.rs"),
            first.join("src/model.postcard"),
        ],
        "the artefacts this compares are not the ones a bless step writes"
    );

    for path in &written {
        let name = path
            .strip_prefix(&first)
            .expect("every written path is under the directory it was written to");
        let (before, after) = (
            std::fs::read(path).expect("the first run's artefact"),
            std::fs::read(second.join(name)).expect("the second run's artefact"),
        );
        assert!(
            before == after,
            "{} is not reproducible",
            name.to_string_lossy()
        );
    }
}

/// A document tagging a value with a `format` the adopter owns a Rust type for.
/// The document never says what that type is: `Settings::replace` is the other
/// half, and neither half is any use alone.
const FORMATTED: &str = r#"
openapi: 3.0.3
info: { title: Formatted, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /prices:
    get:
      operationId: listPrices
      responses:
        "200": { description: OK }
components:
  schemas:
    Price:
      type: object
      required: [currency]
      properties:
        currency: { type: string, format: currency }
"#;

/// The route for an adopter who wants a type of their own rather than the one
/// a named schema would generate: the document tags the shape with a `format`
/// and `replace` says which Rust path stands for it. It is the whole of the
/// adopter's say over the generated types, so both halves are worth pinning —
/// with it their path appears and typify defines nothing, and without it
/// nothing in the file mentions a type the document never named.
#[test]
fn a_replaced_format_becomes_a_type_the_generated_code_never_defines() {
    let with = out("replaced");
    Settings::new(wrote(&with, "prices.yaml", FORMATTED))
        .replace("currency", "crate::Currency")
        .write_to(&with)
        .expect("the document generates");
    let types = read(&with.join("src/types.rs"));
    assert!(
        types.contains("crate::Currency"),
        "`format: currency` did not become the adopter's own type:\n{types}"
    );
    assert!(
        !types.contains("struct Currency"),
        "typify defined a type the adopter owns:\n{types}"
    );

    let without = out("unreplaced");
    Settings::new(wrote(&without, "prices.yaml", FORMATTED))
        .write_to(&without)
        .expect("the document generates");
    let types = read(&without.join("src/types.rs"));
    assert!(
        !types.contains("crate::Currency"),
        "a type the adopter never asked for:\n{types}"
    );
    assert!(
        types.contains("pub currency: ::std::string::String"),
        "an unreplaced format is the string the document declares:\n{types}"
    );
}

/// One operation declaring both a list this crate can spell and an object it
/// cannot.
const SHAPED: &str = r##"
openapi: 3.0.3
info: { title: Shaped, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /vouchers:
    get:
      operationId: listVouchers
      parameters:
        - name: tag
          in: query
          schema: { type: array, items: { type: string } }
        - name: filter
          in: query
          schema:
            type: object
            properties:
              opened:
                type: object
                properties:
                  from: { type: string }
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Voucher" }
components:
  schemas:
    Voucher:
      type: object
      properties:
        id: { type: integer }
"##;

/// A parameter that `openapiv3` holds as `AnySchema` (an integer bound spelled
/// `0.0`), a one-element `allOf` over a named schema, over an array, and an array
/// of such wrappers: each has a flag, so each has an argument of the type the
/// flag carries.
const SCALARS: &str = r##"
openapi: 3.0.3
info: { title: Scalars, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things:
    get:
      operationId: listThings
      parameters:
        - { name: floor, in: query, schema: { type: integer, minimum: 0.0 } }
        - { name: code, in: query, schema: { allOf: [{ $ref: "#/components/schemas/Code" }], description: A code. } }
        - { name: days, in: query, schema: { allOf: [{ type: array, items: { type: string } }], description: Days. } }
        - { name: codes, in: query, required: true, schema: { type: array, items: { allOf: [{ $ref: "#/components/schemas/Code" }] } } }
        - { name: ratio, in: query, required: true, schema: { type: number, maximum: 1e3 } }
        - { name: mixed, in: query, schema: { type: string, allOf: [{ $ref: "#/components/schemas/Code" }] } }
      responses:
        "200": { description: OK }
components:
  schemas:
    Code: { type: string, pattern: "^[A-Z]{3}$" }
"##;

/// The wrapper of an operation takes what the command line takes. A parameter
/// whose schema `openapiv3` holds as `AnySchema`, or inside a one-element
/// `allOf`, has a flag, so it has an argument, typed as a bare one is. One that
/// has no flag is said in the documentation and is no argument.
#[test]
fn a_scalar_read_through_a_wrapper_or_an_any_schema_is_an_argument_of_its_type() {
    let dir = out("scalars");
    Settings::new(wrote(&dir, "scalars.yaml", SCALARS))
        .write_to(&dir)
        .expect("the document generates");
    let ops = read(&dir.join("src/ops.rs"));

    for argument in [
        "floor: ::std::option::Option<i64>",
        "code: ::std::option::Option<crate::types::Code>",
        "days: ::std::vec::Vec<&str>",
        "codes: ::std::vec::Vec<crate::types::Code>",
        "ratio: f64",
    ] {
        assert!(ops.contains(argument), "no `{argument}` in\n{ops}");
    }
    assert!(!ops.contains("mixed:"), "{ops}");
    assert!(
        ops.contains(
            "The document's `mixed` parameter is not an argument: it is stated with `allOf`, \
             which this CLI does not read as a value."
        ),
        "the wrapper does not say why `mixed` is not an argument:\n{ops}"
    );
}

/// A list parameter whose array is named: bare, in a wrapper of one element, and
/// with items that are named too.
const NAMED_LISTS: &str = r##"
openapi: 3.0.3
info: { title: Lists, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things:
    get:
      operationId: listThings
      parameters:
        - { name: days, in: query, schema: { $ref: "#/components/schemas/Days" } }
        - { name: weeks, in: query, schema: { allOf: [{ $ref: "#/components/schemas/Days" }], description: Weeks. } }
        - { name: codes, in: query, required: true, schema: { $ref: "#/components/schemas/Codes" } }
        - { name: kinds, in: query, schema: { allOf: [{ $ref: "#/components/schemas/Codes" }] } }
      responses:
        "200": { description: OK }
components:
  schemas:
    Days: { type: array, items: { type: string } }
    Codes: { type: array, items: { $ref: "#/components/schemas/Code" } }
    Code: { type: string, pattern: "^[A-Z]{3}$" }
"##;

/// The reduction reads a list through a `$ref`, so the wrapper does too: the
/// argument is the `Vec` of the items' type, as it is for an array stated inline.
#[test]
fn a_list_parameter_whose_array_is_named_is_a_vec_argument() {
    let dir = out("named-lists");
    Settings::new(wrote(&dir, "lists.yaml", NAMED_LISTS))
        .write_to(&dir)
        .expect("the document generates");
    let ops = read(&dir.join("src/ops.rs"));

    for argument in [
        "days: ::std::vec::Vec<&str>",
        "weeks: ::std::vec::Vec<&str>",
        "codes: ::std::vec::Vec<crate::types::Code>",
        "kinds: ::std::vec::Vec<crate::types::Code>",
    ] {
        assert!(ops.contains(argument), "no `{argument}` in\n{ops}");
    }
}

/// An array that `openapiv3` holds as `AnySchema` for an empty keyword has a list
/// flag, so its wrapper takes a `Vec` of its items' type, as the same array without
/// the keyword does.
#[test]
fn a_list_that_openapiv3_holds_as_any_is_a_vec_argument() {
    let dir = out("any-lists");
    Settings::new(wrote(&dir, "lists.yaml", ANY_LISTS))
        .write_to(&dir)
        .expect("the document generates");
    let ops = read(&dir.join("src/ops.rs"));

    for argument in [
        "days: ::std::vec::Vec<&str>",
        "codes: ::std::vec::Vec<crate::types::Code>",
    ] {
        assert!(ops.contains(argument), "no `{argument}` in\n{ops}");
    }
}

/// Two list parameters whose arrays state an empty keyword: one with items stated
/// inline, one with items that are named.
const ANY_LISTS: &str = r##"
openapi: 3.0.3
info: { title: Lists, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things:
    get:
      operationId: listThings
      parameters:
        - { name: days, in: query, schema: { type: array, items: { type: string }, properties: {} } }
        - { name: codes, in: query, required: true, schema: { type: array, items: { $ref: "#/components/schemas/Code" }, enum: [] } }
      responses:
        "200": { description: OK }
components:
  schemas:
    Code: { type: string, pattern: "^[A-Z]{3}$" }
"##;

/// A CLI and a Rust caller reach one request builder, so a parameter has to
/// mean the same thing on both roads: a list is a `Vec` argument filled the way
/// a repeated flag fills it, and a parameter neither can supply is an argument
/// on neither — said in the wrapper's own documentation rather than left for a
/// reader to notice.
#[test]
fn a_list_parameter_is_a_vec_argument_and_one_with_no_flag_is_no_argument() {
    let dir = out("shaped");
    Settings::new(wrote(&dir, "shaped.yaml", SHAPED))
        .write_to(&dir)
        .expect("the document generates");
    let ops = read(&dir.join("src/ops.rs"));

    assert!(
        ops.contains("tag: ::std::vec::Vec<&str>"),
        "the list is not a Vec:\n{ops}"
    );
    assert!(
        ops.contains(r#".each("tag", tag)"#),
        "the list does not reach the request builder repeated:\n{ops}"
    );
    assert!(
        !ops.contains("filter:"),
        "an argument the wrapper has nowhere to put:\n{ops}"
    );
    assert!(
        ops.contains(
            "The document's `filter` parameter is not an argument: it is neither a value \
             nor a list of values."
        ),
        "the wrapper does not say what it does not carry:\n{ops}"
    );
}

/// Every generated file opens by telling its reader how to rewrite it and
/// where a correction belongs. Both are read off the settings, so an adoption
/// that spells its bless step differently gets its own spelling back.
#[test]
fn every_generated_file_names_the_command_that_rewrites_it() {
    let dir = out("header");
    layered()
        .regenerated_by("just bless")
        .summary_page("src/summary.md")
        .write_to(&dir)
        .expect("the fixtures generate");

    for file in [
        "spec/toy.overlaid.yaml",
        "src/types.rs",
        "src/ops.rs",
        "src/summary.md",
    ] {
        assert!(
            read(&dir.join(file)).contains("Generated by `just bless`"),
            "{file} does not name the command that rewrites it"
        );
    }
    assert!(
        read(&dir.join("src/ops.rs"))
            .contains("belongs in `fixtures/corrections.yaml` and `fixtures/cli.yaml`"),
        "the generated Rust does not point at the Overlay it was corrected by"
    );
}

/// A path that is not there names itself, rather than surfacing as a bare
/// `No such file or directory` from somewhere inside the generator.
#[test]
fn a_document_that_is_not_there_is_named() {
    let error = Settings::new("nowhere.yaml")
        .overlay(CORRECTIONS)
        .write_to(out("missing"))
        .expect_err("no document to generate from");
    assert!(
        matches!(&error, GenerateError::Read { path, .. } if path.ends_with("nowhere.yaml")),
        "{error}"
    );
}

/// The fixture at `path` with its third line, the `title` of both, replaced by
/// an unclosed quote. The reader finds the quote unclosed on the next line.
fn with_an_unclosed_quote(path: &str) -> String {
    read(Path::new(path))
        .lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 2 {
                "  title: \"unclosed"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a bless step prints for `error`: its own words, then each cause under
/// it on a line of its own.
fn reported(error: &GenerateError) -> String {
    let mut chain = vec![error.to_string()];
    let mut cause = error.source();
    while let Some(next) = cause {
        chain.push(next.to_string());
        cause = next.source();
    }
    chain.join("\n")
}

/// A layer the reader cannot read is named by its file, and the reader's own
/// words give the line and the column where it stopped.
///
/// The bless step prints the layer, then each cause under it, so a person who
/// sees an unclosed quote in a long file is told which file and where.
#[test]
fn a_layer_that_is_not_yaml_is_named_with_its_line_and_column() {
    let dir = out("unclosed");
    let layer = wrote(
        &dir,
        "corrections.yaml",
        &with_an_unclosed_quote(CORRECTIONS),
    );

    let error = Settings::new(TOY)
        .overlay(&layer)
        .overlay(CLI)
        .write_to(&dir)
        .expect_err("a layer with an unclosed quote");

    assert!(
        matches!(&error, GenerateError::Overlay { path, source: OverlayError::Syntax(cause) }
            if path.ends_with("corrections.yaml")
                && matches!(cause.place(), Some(Place::Text { line: 4, .. }))),
        "{error:?}"
    );
    let reported = reported(&error);
    assert!(reported.contains("corrections.yaml"), "{reported}");
    assert!(reported.contains("at line 4"), "{reported}");
}

/// The vendor's document that the reader cannot read is named by its file in
/// the same way, with the line and the column where the reader stopped.
#[test]
fn a_document_that_is_not_yaml_is_named_with_its_line_and_column() {
    let dir = out("unclosed-document");
    let document = wrote(&dir, "toy.yaml", &with_an_unclosed_quote(TOY));

    let error = Settings::new(&document)
        .overlay(CORRECTIONS)
        .overlay(CLI)
        .write_to(&dir)
        .expect_err("a document with an unclosed quote");

    assert!(
        matches!(&error, GenerateError::Overlay { path, source: OverlayError::Document(cause) }
            if path.ends_with("toy.yaml")
                && matches!(cause.place(), Some(Place::Text { line: 4, .. }))),
        "{error:?}"
    );
    let reported = reported(&error);
    assert!(reported.contains("toy.yaml"), "{reported}");
    assert!(reported.contains("at line 4"), "{reported}");
}

/// A document whose second operation breaks the OpenAPI model, read by the bless
/// step. The model says "expected a boolean" and stops at `paths`, so the report
/// is the node the reduction found by reading the path items one at a time.
#[test]
fn a_document_the_openapi_model_refuses_is_named_by_its_operation() {
    let dir = out("not-openapi");
    let document = wrote(
        &dir,
        "vendor.yaml",
        r#"openapi: 3.0.3
info: { title: Vendor, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /a:
    get: { operationId: a, responses: { "200": { description: OK } } }
  /b:
    get:
      operationId: b
      deprecated: "yes"
      responses: { "200": { description: OK } }
"#,
    );

    let error = Settings::new(&document)
        .write_to(&dir)
        .expect_err("the OpenAPI model refuses the second operation");

    assert!(
        matches!(&error, GenerateError::Unusable(LoadError::Shape { at, .. })
            if at == "paths[/b].get.deprecated"),
        "{error:?}"
    );
    let reported = reported(&error);
    assert!(
        reported.contains("`paths[/b].get.deprecated`"),
        "{reported}"
    );
    assert!(
        reported.contains("invalid type: string \"yes\", expected a boolean"),
        "{reported}"
    );
}

/// A vendor document at `version` with nothing in it, and a layer with one
/// action that `update`s the root of the document. `update` is the value of
/// `update:`, written as a flow collection.
fn with_root_update(dir: &Path, version: &str, update: &str) -> Settings {
    let document = wrote(
        dir,
        "vendor.yaml",
        &format!(
            "openapi: {version}\ninfo: {{ title: Vendor, version: \"1.0\" }}\nservers: [{{ url: \
             \"http://localhost:9999\" }}]\npaths: {{}}\n"
        ),
    );
    let layer = wrote(
        dir,
        "layer.yaml",
        &format!(
            "overlay: 1.1.0\ninfo: {{ title: t, version: \"1\" }}\nactions:\n  - target: $\n    \
             description: the root\n    update: {update}\n"
        ),
    );
    Settings::new(&document).overlay(&layer)
}

/// The bless step lays the layers itself, and a layer that changes `openapi` is
/// refused at its file, as `Document::load_with` refuses it at its index. The
/// error names the file, and the cause names both versions. The inputs are the
/// ones `Document::load` is given in `tests/reading.rs`: another version,
/// another patch of 3.0, and a layer that replaces the document with a list and
/// so leaves no key.
#[test]
fn a_layer_that_changes_openapi_is_named_by_its_file() {
    for (label, update, after, said) in [
        (
            "another-version",
            "{ openapi: 3.1.0 }",
            Some(json!("3.1.0")),
            "changes `openapi` from `3.0.3` to `3.1.0`",
        ),
        (
            "another-patch",
            "{ openapi: 3.0.0 }",
            Some(json!("3.0.0")),
            "changes `openapi` from `3.0.3` to `3.0.0`",
        ),
        (
            "no-key",
            "[1]",
            None,
            "changes `openapi` from `3.0.3` to no `openapi` key",
        ),
    ] {
        let dir = out(&format!("relabel-{label}"));

        let error = with_root_update(&dir, "3.0.3", update)
            .write_to(&dir)
            .expect_err(label);

        assert!(
            matches!(&error, GenerateError::Overlay { path, source: OverlayError::OpenApiVersion { before, after: now } }
                if path.ends_with("layer.yaml")
                    && before.as_deref() == Some(&json!("3.0.3"))
                    && now.as_deref() == after.as_ref()),
            "{label}: {error:?}"
        );
        let reported = reported(&error);
        assert!(reported.contains("layer.yaml"), "{label}: {reported}");
        assert!(reported.contains(said), "{label}: {reported}");
    }
}

/// A 3.1 document given a layer whose target is not in it is refused for its
/// version and not for the layer. The bless step asks the vendor's version
/// before it lays a layer, so the document is what is refused when the layer
/// would also have been. The control is a 3.0.3 document with a layer that
/// writes the version it has, which changes nothing.
#[test]
fn a_3_1_document_is_refused_for_its_version_before_the_bless_step_lays_a_layer() {
    let dir = out("version-first");
    let document = wrote(
        &dir,
        "vendor.yaml",
        "openapi: 3.1.0\ninfo: { title: Vendor, version: \"1.0\" }\nservers: [{ url: \
         \"http://localhost:9999\" }]\npaths: {}\n",
    );
    let never_applies = wrote(
        &dir,
        "never.yaml",
        "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - target: $.nothing\n    \
         description: a target that is not there\n    update:\n      x: 1\n",
    );

    let error = Settings::new(&document)
        .overlay(&never_applies)
        .write_to(&dir)
        .expect_err("a 3.1 document");

    assert!(
        matches!(&error, GenerateError::Unusable(LoadError::Version { version }) if version == "3.1.0"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "the document does not describe a usable CLI",
        "the vendor's document is not yet overlaid, so the words must not say it is"
    );
    let control = out("version-first-control");
    assert!(
        with_root_update(&control, "3.0.3", "{ openapi: 3.0.3 }")
            .write_to(&control)
            .is_ok(),
        "a layer that writes the version the document has is not a change"
    );
}

/// Every string a YAML 1.1 reader would misread, as the vendor writes them.
const LOOKALIKES: &str = r#"
openapi: 3.0.3
info:
  title: Lookalikes
  version: "1.0"
  x-strings: ["N", "yes", "on", "1.0", "017", "1_000", "~", "null", "true", "0x1F"]
servers: [{ url: "http://localhost:9999" }]
paths:
  /things:
    get:
      operationId: getThings
      responses:
        "200": { description: OK }
"#;

/// The overlaid document is read by every tool an adopter points at it, and
/// not only by this crate's reader, which keeps `yes` a string. The emitter
/// therefore writes the lookalikes in quotes, and the written file reads back
/// to the value that was written.
#[test]
fn the_overlaid_document_quotes_what_yaml_1_1_would_misread_and_reads_back_equal() {
    let dir = out("lookalikes");
    let document = wrote(&dir, "lookalikes.yaml", LOOKALIKES);
    let written = Settings::new(&document)
        .write_to(&dir)
        .expect("the document generates");
    let text = read(&written[0]);

    assert_eq!(
        overlay::parse(&text).expect("the written document is YAML"),
        overlay::parse(LOOKALIKES).expect("the vendor's document is YAML"),
        "the overlaid document does not read back to what was written"
    );
    for lookalike in [
        "\"N\"", "\"yes\"", "\"on\"", "\"1.0\"", "\"017\"", "\"null\"",
    ] {
        assert!(
            text.contains(lookalike),
            "{lookalike} is not quoted in the overlaid document:\n{text}"
        );
    }
}

/// A document whose schema names are not already Rust type names.
///
/// Nothing here is unusual in a vendor's document: an underscore, a dash, a
/// leading digit, a name that is already camel rather than pascal. Every one of
/// them is a name typify has to change before it can be a type, which is the
/// whole point of the fixture — the toy's own schemas are spelled `Voucher` and
/// `Currency` and so agree with typify by accident.
const AWKWARDLY_NAMED: &str = r##"
openapi: 3.0.3
info: { title: Awkward, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /vouchers:
    post:
      operationId: addVoucher
      requestBody:
        content:
          application/json:
            schema: { $ref: "#/components/schemas/Model_voucher" }
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { $ref: "#/components/schemas/voucher-summary" }
  /vouchers/recent:
    get:
      operationId: listRecentVouchers
      parameters:
        - name: since
          in: query
          schema: { $ref: "#/components/schemas/2fa_stamp" }
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema:
                type: array
                items: { $ref: "#/components/schemas/voucher-summary" }
components:
  schemas:
    Model_voucher:
      type: object
      properties:
        total: { type: string }
    voucher-summary:
      type: object
      properties:
        count: { type: integer }
    2fa_stamp:
      type: string
"##;

/// Every type a wrapper names, as `ops.rs` spells it.
fn named_by_wrappers(ops: &str) -> Vec<String> {
    ops.split("crate::types::")
        .skip(1)
        .map(|after| {
            after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect()
        })
        .collect()
}

/// A wrapper that names a type the schemas did not generate is a crate that
/// does not compile, and the compiler is the adopter's rather than this
/// crate's — so the two emitters cannot be left to derive a schema's Rust name
/// each for themselves. This is that agreement, asserted on a document whose
/// names force typify to rename every one of them.
#[test]
fn every_type_a_wrapper_names_is_one_the_generated_schemas_define() {
    for (name, document) in [
        ("awkward-wrappers", AWKWARDLY_NAMED),
        ("patterned-wrappers", PATTERNED),
    ] {
        let dir = out(name);
        Settings::new(wrote(&dir, "document.yaml", document))
            .write_to(&dir)
            .expect("the document generates");
        let types = read(&dir.join("src/types.rs"));
        let ops = read(&dir.join("src/ops.rs"));

        let named = named_by_wrappers(&ops);
        assert!(
            !named.is_empty(),
            "{name}: no wrapper names a generated type, so this proves nothing"
        );
        for ty in named {
            assert!(
                types.contains(&format!("pub struct {ty}"))
                    || types.contains(&format!("pub enum {ty}"))
                    || types.contains(&format!("pub type {ty}")),
                "{name}: a wrapper names `crate::types::{ty}`, which the \
                 generated schemas do not define:\n{ops}"
            );
        }
    }
}

/// An operation whose parameter is named after a Rust keyword.
///
/// `type` is an ordinary thing for an API to filter on, and the document is
/// under no obligation to avoid Rust's vocabulary. `ref`, `match` and `move`
/// are the same case.
const KEYWORDED: &str = r##"
openapi: 3.0.3
info: { title: Keyworded, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /vouchers:
    get:
      operationId: listVouchers
      parameters:
        - name: type
          in: query
          schema: { type: string }
        - name: ref
          in: query
          schema: { type: string }
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Voucher" }
components:
  schemas:
    Voucher:
      type: object
      properties:
        id: { type: integer }
"##;

/// A parameter named after a keyword keeps the document's own word, as a raw
/// identifier, because a mangled `type_` reads as something the generator
/// invented. What crosses the wire is unaffected either way: the wire name
/// travels beside the argument as the literal the request builder is given.
#[test]
fn a_parameter_named_after_a_keyword_is_a_raw_identifier_and_keeps_its_wire_name() {
    let dir = out("keyworded");
    Settings::new(wrote(&dir, "document.yaml", KEYWORDED))
        .write_to(&dir)
        .expect("a keyword is a spelling problem, not a document this crate refuses");

    let ops = read(&dir.join("src/ops.rs"));
    assert!(
        ops.contains("r#type: ::std::option::Option<&str>")
            && ops.contains("r#ref: ::std::option::Option<&str>"),
        "a keyword parameter is not a raw identifier:\n{ops}"
    );
    assert!(
        ops.contains(r#".maybe("type", r#type)"#) && ops.contains(r#".maybe("ref", r#ref)"#),
        "the wire name did not survive the spelling:\n{ops}"
    );
}

/// A wrapper this crate cannot spell names the operation it belongs to. The
/// alternative is what a bare parse failure over a generated file gives an
/// adopter: a `syn` error with a position in a token stream and nothing to open.
#[test]
fn an_operation_that_cannot_be_spelled_names_itself() {
    let dir = out("unspellable");
    // A leading digit survives snake-casing and is not the start of any
    // identifier, raw or otherwise.
    let document = KEYWORDED.replace("operationId: listVouchers", "operationId: 2listVouchers");
    let failure = Settings::new(wrote(&dir, "document.yaml", &document))
        .write_to(&dir)
        .expect_err("`2listVouchers` has no spelling as a Rust identifier");
    assert!(
        matches!(&failure, GenerateError::Operation { op, .. } if op == "2listVouchers"),
        "{failure:?}"
    );
    assert_eq!(
        failure.to_string(),
        "the wrapper for 2listVouchers cannot be generated",
        "the failure has to name the operation: a generated file is six thousand \
         lines and a `syn` position is not something an adopter can open"
    );
    assert_eq!(
        failure
            .source()
            .expect("the operation's failure has a cause")
            .to_string(),
        "the operationId has no spelling as a Rust identifier"
    );
}

/// An optional parameter whose name starts with a digit (`3ds`, a vendor's
/// three-domain-secure flag) has a flag on the command line, and a Rust
/// identifier does not start with a digit. The parameter's name is its wire key,
/// so an Overlay cannot rename it and could only remove it: the generator puts
/// `_` in front of the word instead, and the wrapper says which wire key the
/// argument sends.
#[test]
fn a_parameter_whose_name_starts_with_a_digit_gets_a_prefixed_identifier_and_keeps_its_wire_key() {
    let dir = out("digit-param");
    let document = KEYWORDED
        .replace("- name: type", "- name: 3ds")
        .replace("- name: ref", "- name: \"2\"");
    Settings::new(wrote(&dir, "document.yaml", &document))
        .write_to(&dir)
        .expect("a name that starts with a digit is a spelling problem and not a refusal");

    let ops = read(&dir.join("src/ops.rs"));
    assert!(
        ops.contains("x3ds: ::std::option::Option<&str>")
            && ops.contains("x2: ::std::option::Option<&str>"),
        "a digit is not the start of an identifier:\n{ops}"
    );
    assert!(
        ops.contains(r#".maybe("3ds", x3ds)"#) && ops.contains(r#".maybe("2", x2)"#),
        "the wire key has to survive the spelling:\n{ops}"
    );
    for said in [
        "`x3ds` sends the document's `3ds`, under a name of its own: a Rust identifier does not \
         start with a digit, so `x` stands in front of the word.",
        "`x2` sends the document's `2`, under a name of its own: a Rust identifier does not \
         start with a digit, so `x` stands in front of the word.",
    ] {
        assert!(
            ops.contains(said),
            "the wrapper does not say which wire key the argument sends:\n{ops}"
        );
    }
}

/// The prefix is one more spelling of the plain word, so it moves aside for an
/// argument declared before it as any other does, and the wrapper gives both
/// reasons for the name it ends with.
#[test]
fn a_prefixed_identifier_that_is_spent_moves_aside_and_says_both_reasons() {
    let dir = out("digit-param-spent");
    let document = KEYWORDED
        .replace("- name: type", "- name: x3ds")
        .replace("- name: ref", "- name: 3ds");
    Settings::new(wrote(&dir, "document.yaml", &document))
        .write_to(&dir)
        .expect("two spellings of one word are one more argument");

    let ops = read(&dir.join("src/ops.rs"));
    assert!(
        ops.contains(r#".maybe("x3ds", x3ds)"#) && ops.contains(r#".maybe("3ds", x3ds_2)"#),
        "the later parameter keeps its wire key under the next name:\n{ops}"
    );
    assert!(
        ops.contains(
            "`x3ds_2` sends the document's `3ds`, under a name of its own: a Rust identifier \
             does not start with a digit, so `x` stands in front of the word, and an argument \
             declared before it had already spent that spelling."
        ),
        "the wrapper gives one reason for a name that has two:\n{ops}"
    );
}

/// An adopter's lint set can be as strict as they like (`clippy::nursery`,
/// `missing_docs`, `unused_qualifications`, then `-D warnings`), and none of it
/// is aimed at a file they did not write. The exemption is the same block in both
/// files, it names `warnings` beside the groups it has always named, and its
/// reason says that the crate holding the file does not grade it.
#[test]
fn both_generated_files_allow_every_warning_and_say_why() {
    let dir = out("allowed");
    layered().write_to(&dir).expect("the fixtures generate");

    for file in ["src/types.rs", "src/ops.rs"] {
        let source = read(&dir.join(file));
        let (_, after) = source
            .split_once("#![allow(")
            .unwrap_or_else(|| panic!("{file} carries no `#![allow(`"));
        let (allowed, _) = after
            .split_once(")]")
            .unwrap_or_else(|| panic!("{file} does not close its `#![allow(`"));
        let items: Vec<&str> = allowed
            .lines()
            .map(str::trim)
            .filter(|line| line.ends_with(','))
            .collect();
        assert!(
            items.contains(&"warnings,"),
            "{file} does not allow `warnings`: {items:?}"
        );
        // The reason is one string that the file spells over two lines.
        assert!(
            dense(&allowed.replace('\\', "")).contains(&dense(
                "reason = \"generated source is not graded on style or on any lint that the crate \
                 holding it turns on; the allow covers this module and nothing else\""
            )),
            "{file} gives no reason that covers `warnings`:\n{allowed}"
        );
    }
}

/// The standard items that generated code names, and that a schema can be called.
const STANDARD: [&str; 10] = [
    "Ok", "Err", "Some", "None", "String", "Vec", "Option", "Result", "Box", "Default",
];

/// Every place in `source` where a standard item is named by one bare word: a
/// path of one segment in an expression or a pattern, and in a type for a type
/// that a file holds none of. `Default::default` is the standard item's own path
/// of two, and counts too.
fn bare_standard_items(source: &str, types_too: bool) -> Vec<String> {
    use syn::visit_mut::VisitMut;

    struct Bare {
        found: Vec<String>,
        types_too: bool,
    }
    impl Bare {
        fn note(&mut self, path: &syn::Path) {
            let words: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            let bare = path.leading_colon.is_none()
                && match words.as_slice() {
                    [one] => STANDARD.contains(&one.as_str()),
                    [first, second] => first == "Default" && second == "default",
                    _ => false,
                };
            if bare {
                self.found.push(words.join("::"));
            }
        }
    }
    impl VisitMut for Bare {
        fn visit_expr_path_mut(&mut self, expr: &mut syn::ExprPath) {
            self.note(&expr.path);
            syn::visit_mut::visit_expr_path_mut(self, expr);
        }
        fn visit_pat_tuple_struct_mut(&mut self, pat: &mut syn::PatTupleStruct) {
            self.note(&pat.path);
            syn::visit_mut::visit_pat_tuple_struct_mut(self, pat);
        }
        fn visit_pat_ident_mut(&mut self, pat: &mut syn::PatIdent) {
            if STANDARD.contains(&pat.ident.to_string().as_str()) {
                self.found.push(pat.ident.to_string());
            }
        }
        fn visit_type_path_mut(&mut self, ty: &mut syn::TypePath) {
            if self.types_too {
                self.note(&ty.path);
            }
            syn::visit_mut::visit_type_path_mut(self, ty);
        }
    }

    let mut file: syn::File = syn::parse_file(source).expect("a generated file is Rust");
    let mut bare = Bare {
        found: Vec::new(),
        types_too,
    };
    bare.visit_file_mut(&mut file);
    bare.found
}

/// A schema is free to be called `Ok`, `Err`, `Some`, `None`, `String`, `Vec`,
/// `Option`, `Result`, `Box` or `Default`, so the generated code names each of
/// those items by its full path, and nothing it writes means whatever a schema is
/// called. `types.rs` is typify's, and the items that it writes by one word are
/// the ones of its own templates (`Ok(…)`, `Err(…)`, `Default::default()`), found
/// in expressions and patterns, where a bare word is never a schema; the types of
/// a file are the schemas'. `ops.rs` is this crate's own, and not one word of it
/// is bare, a type included, because it names a schema only by the path of the
/// module that holds it.
///
/// The compile test of `tests/hostile.rs` holds both files to the meaning. This
/// holds `ops.rs` to the text, because no schema can be called what `ops.rs`
/// names, so no compile would fail.
#[test]
fn the_generated_code_names_every_standard_item_by_its_full_path() {
    let toy = out("standard-paths");
    layered().write_to(&toy).expect("the fixtures generate");
    let hostile = out("standard-paths-hostile");
    Settings::new(HOSTILE)
        .replace("cone-number", "crate::pyrometry::Cone")
        .write_to(&hostile)
        .expect("the control document generates");

    for (label, root) in [("toy", toy), ("hostile", hostile)] {
        let types = bare_standard_items(&read(&root.join("src/types.rs")), false);
        assert!(types.is_empty(), "{label}: types.rs names {types:?} bare");
        let ops = bare_standard_items(&read(&root.join("src/ops.rs")), true);
        assert!(ops.is_empty(), "{label}: ops.rs names {ops:?} bare");
    }
}

/// A named schema that is a bare string: no `pattern`, no `enum`, no `format`.
///
/// Naming a schema purely to give a field a description and one place to change
/// it is an ordinary thing to do, and it is the one shape typify gives a
/// `Display` of its own.
const UNCONSTRAINED: &str = r##"
openapi: 3.0.3
info: { title: Unconstrained, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /vouchers:
    get:
      operationId: listVouchers
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Voucher" }
components:
  schemas:
    Filename:
      type: string
      description: The name the server gave an uploaded file.
    Sku:
      type: string
      pattern: "^[A-Z]{3}-[0-9]{4}$"
    Voucher:
      type: object
      properties:
        attachment: { $ref: "#/components/schemas/Filename" }
        sku: { $ref: "#/components/schemas/Sku" }
"##;

/// Which newtypes typify prints for itself is not a thing to predict: it writes
/// a `Display` for an unconstrained string newtype and omits it for a
/// pattern-constrained one. Both shapes have to end up with exactly one, and
/// the document here carries one of each so that neither half can pass alone.
#[test]
fn every_string_newtype_prints_and_none_of_them_twice() {
    let dir = out("unconstrained");
    Settings::new(wrote(&dir, "document.yaml", UNCONSTRAINED))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    for name in ["Filename", "Sku"] {
        let written = types
            .matches(&format!("impl ::std::fmt::Display for {name} "))
            .count();
        assert_eq!(
            written, 1,
            "`{name}` has {written} `Display` impls, and a generated crate \
             compiles with exactly one:\n{types}"
        );
    }
}

/// Descriptions in every shape a vendor writes prose in, including the four
/// that Markdown turns into code and rustdoc then compiles, and the five that a
/// reading of the text line by line misses.
///
/// Three of the five are hidden by a Markdown container, which puts a block where
/// no line starts with a fence or with four spaces: a fence inside a block quote,
/// a fence opened on a list-marker line, and an indented block inside a block
/// quote. The other two stand at the top and are missed by a reading that looks
/// for a word after the fence: an info string that is only a class (`{.json}`),
/// and one that names a target to skip (`ignore-wasm32`).
///
/// Each carries a word in capitals that appears nowhere else, so that a test
/// finds the one doc comment it is about.
///
/// The hanging list marker is carried three times because no one position
/// stands for the others: under a paragraph line and under a blank line in a
/// property's description, and under a paragraph line in an operation's
/// summary. The asterisk-bulleted list beside them is a list in the other
/// spelling of its marker, which a doc comment holds as the vendor wrote it.
const PROSE: &str = r##"
openapi: 3.0.3
info: { title: Prose, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /vouchers:
    get:
      operationId: listVouchers
      summary: |
        List the vouchers.
        *     A summary the vendor hung from a marker.

            A summary the vendor indented.

        > ```
        > SUMMARY-FENCE is not Rust
        > ```
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Voucher" }
components:
  schemas:
    Voucher:
      type: object
      properties:
        kind:
          type: string
          description: |
            The kind of the voucher.

                A paragraph the vendor indented four spaces.
        parts:
          type: string
          description: |
            What a complete voucher needs:

            - a contact
                - with an address
                - and a name
        sample:
          type: string
          description: |
            An example of one:

            ```
            {"id": "1"}
            ```
        labelled:
          type: string
          description: |
            An example the vendor labelled:

            ```rust
            this is pseudocode, not Rust
            ```

            and the same with tildes:

            ~~~rust
            nor is this
            ~~~
        buried:
          type: string
          description: |
            An example the vendor indented under its heading:

                ```
                {"id": "1"}
                ```
        highlighted:
          type: string
          description: |
            An example whose language rustdoc does not run:

            ```json
            {"id": "1"}
            ```
        hanging:
          type: string
          description: |
            A list whose content the vendor hung five columns from its marker:
            *     the content of that item.
        hanging_apart:
          type: string
          description: |
            The same, set off from its lead-in by a blank line:

            *     the content of that item too.
        hanging_numbered:
          type: string
          description: |
            An ordered list hung the same way:
            1.     the content of that item as well.
        starred:
          type: string
          description: |
            A list the vendor bulleted with asterisks:

            * one item under an asterisk
            * and a second one
        aside:
          type: string
          description: |
            A description that mentions //! in passing.
        ruled:
          type: string
          description: |
            A paragraph, and a rule that is set three columns in:
               ***
        quoted:
          type: string
          description: |
            A note in a block quote:

            > Example request:
            > ```
            > QUOTED-FENCE is not Rust
            > ```
        listed:
          type: string
          description: |
            Steps:

            - ```
              LISTED-FENCE is not Rust either
              ```
        quotedindent:
          type: string
          description: |
            Quoted and indented:

            >     QUOTED-INDENT is not Rust
        classed:
          type: string
          description: |
            An example with a class on its fence:

            ```{.json}
            {"CLASSED-FENCE": "1"}
            ```
        targeted:
          type: string
          description: |
            An example ignored on one target:

            ```ignore-wasm32
            {"TARGETED-FENCE": "2"}
            ```
"##;

/// Every doc comment in `source`, verbatim and in its own column, on items
/// that depend on nothing.
///
/// Copying the bytes and their positions is what makes this a fair question to
/// put to the doctest runner; generating a crate it could compile directly
/// would mean handing it serde as well, and the answer would be the same. A
/// `/** */` comment is copied as well as a run of `///` lines, although the
/// generator writes only the second: the column of a block comment's lines is
/// half of what decides whether a paragraph in it is code, so a generator that
/// wrote one would be judged as the comment would be run.
///
/// A `#[doc = "…"]` attribute is no comment, and a copy that left it out would
/// hand the judges less than the file holds, so a file that has one fails here.
fn doc_comments_alone(source: &str) -> String {
    if let Some(attribute) = source
        .lines()
        .find(|line| line.trim_start().starts_with("#[doc"))
    {
        panic!(
            "a doc attribute stands in the file, and the judges are given comments: {attribute}"
        );
    }
    let mut out = String::from("pub struct Docs {\n");
    let mut fields = 0;
    let mut block = false;
    let mut carrying = false;
    for line in source.lines() {
        let trimmed = line.trim_start();
        let in_comment = block || trimmed.starts_with("/**") || trimmed.starts_with("///");
        if in_comment {
            out.push_str(line);
            out.push('\n');
            block = (block || trimmed.starts_with("/**")) && !trimmed.ends_with("*/");
            carrying = !block;
            continue;
        }
        if carrying {
            let _ = writeln!(out, "    pub f{fields}: i64,");
            fields += 1;
            carrying = false;
        }
    }
    out.push_str("}\n");
    out
}

/// rustdoc compiles and runs the code blocks in a doc comment, and Markdown
/// makes a code block out of two shapes of ordinary prose: a run of lines
/// indented four spaces, and a fence naming no language. Both are things a
/// vendor writes without meaning Rust, and neither is reachable by a lint
/// allowance — a doctest is executed, not linted. So the comments of `PROSE`
/// are handed to the doctest runner, which is the only thing that answers the
/// question properly.
#[test]
fn a_vendors_prose_carries_nothing_rustdoc_will_run() {
    let dir = out("prose");
    Settings::new(wrote(&dir, "document.yaml", PROSE))
        .write_to(&dir)
        .expect("the document generates");

    for (file, alone) in [
        ("src/types.rs", "types_docs.rs"),
        ("src/ops.rs", "ops_docs.rs"),
    ] {
        let comments = doc_comments_alone(&read(&dir.join(file)));
        assert!(
            comments.contains("///") || comments.contains("/**"),
            "{file}: no vendor prose was extracted, so this proves nothing"
        );
        let said = rustdoc_tests(&dir, alone, &comments);
        assert!(
            said.contains("running 0 tests"),
            "rustdoc found something to run in {file}:\n{said}\n{comments}"
        );
    }
}

/// What `rustdoc --test` says about `comments`, which is a file of doc comments
/// on items that depend on nothing: the tests it found, and what it said about
/// each.
fn rustdoc_tests(dir: &Path, name: &str, comments: &str) -> String {
    let path = wrote(dir, name, comments);
    let ran = std::process::Command::new("rustdoc")
        .args(["--test", "--edition", "2024"])
        .arg(&path)
        .output()
        .expect("rustdoc is on PATH beside the rustfmt a bless step already needs");
    format!(
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    )
}

/// Whether `rustc` accepts `comments`, which is a file of doc comments on items
/// that depend on nothing, and what it said when it did not.
fn rustc_accepts(dir: &Path, name: &str, comments: &str) -> Result<(), String> {
    let path = wrote(dir, name, comments);
    let compiled = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "lib",
            "--emit=metadata",
        ])
        .arg("--out-dir")
        .arg(dir)
        .arg(&path)
        .output()
        .expect("rustc is on PATH beside the rustfmt a bless step already needs");
    if compiled.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&compiled.stderr).into_owned())
    }
}

/// The page rustdoc renders for `Docs`, the one item of `comments`.
fn rustdoc_page(dir: &Path, name: &str, comments: &str) -> String {
    let path = wrote(dir, name, comments);
    let site = dir.join("site");
    let ran = std::process::Command::new("rustdoc")
        .args(["--edition", "2024", "--crate-type", "lib", "--crate-name"])
        .arg("docs")
        .arg("-o")
        .arg(&site)
        .arg(&path)
        .output()
        .expect("rustdoc is on PATH beside the rustfmt a bless step already needs");
    assert!(
        ran.status.success(),
        "rustdoc stopped on the comments:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    read(&site.join("docs/struct.Docs.html"))
}

/// Whether `line` is a `///` doc comment line, in whatever column.
fn is_doc(line: &str) -> bool {
    line.trim_start().starts_with("///")
}

/// The doc comment that holds `marker`, as rustdoc is handed it: the text of the
/// run of `///` lines the marker is on, less the indentation that all of its
/// lines share.
///
/// A doc comment is the unit these tests assert on. The same words in some
/// other item's comment would pass a search of the whole file.
fn doc_with(source: &str, marker: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains(marker))
        .unwrap_or_else(|| panic!("no line holds `{marker}`:\n{source}"));
    assert!(
        is_doc(lines[at]),
        "`{marker}` is in no `///` comment:\n{source}"
    );
    let from = lines[..at]
        .iter()
        .rposition(|line| !is_doc(line))
        .map_or(0, |apart| apart + 1);
    let to = at + lines[at..].iter().take_while(|line| is_doc(line)).count();
    let text: Vec<&str> = lines[from..to]
        .iter()
        .map(|line| line.trim_start().strip_prefix("///").unwrap_or(line))
        .collect();
    let shared = text
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    text.iter()
        .map(|line| line.get(shared..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A document that carries each description in the three places a generated
/// doc comment is written: a property `p0` on (a field, four columns in), a
/// schema `S0` on (a type, none in) and an operation `o0` on (a method, four
/// in). Only the first `kinds` of them are carried as types and as operations,
/// because each is a good deal of generated code.
fn described(descriptions: &[String], kinds: usize) -> String {
    let properties: serde_json::Map<String, serde_json::Value> = descriptions
        .iter()
        .enumerate()
        .map(|(at, text)| {
            (
                format!("p{at}"),
                json!({ "type": "string", "description": text }),
            )
        })
        .collect();
    let mut schemas = serde_json::Map::new();
    schemas.insert(
        "Thing".to_owned(),
        json!({ "type": "object", "properties": properties }),
    );
    let mut paths = serde_json::Map::new();
    for (at, text) in descriptions.iter().take(kinds).enumerate() {
        schemas.insert(
            format!("S{at}"),
            json!({ "type": "string", "description": text }),
        );
        paths.insert(
            format!("/o{at}"),
            json!({ "get": {
                "operationId": format!("o{at}"),
                "summary": text,
                "responses": { "200": { "description": "OK" } }
            } }),
        );
    }
    json!({
        "openapi": "3.0.3",
        "info": { "title": "Described", "version": "1.0" },
        "servers": [{ "url": "http://localhost:9999" }],
        "paths": paths,
        "components": { "schemas": schemas }
    })
    .to_string()
}

/// What capping the indentation costs, stated where it can be seen: a nested
/// list stays nested, and the words are the vendor's own.
#[test]
fn prose_that_was_never_code_is_still_prose() {
    let dir = out("prose-kept");
    Settings::new(wrote(&dir, "document.yaml", PROSE))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert!(
        types.contains("- a contact") && types.contains("   - with an address"),
        "a nested list did not survive the capping:\n{types}"
    );
    assert!(
        types.contains("A paragraph the vendor indented four spaces."),
        "the vendor's words did not survive:\n{types}"
    );
    assert!(
        types.contains("```text"),
        "a fence naming no language was left as Rust:\n{types}"
    );
    assert!(
        !types.contains("```rust") && !types.contains("~~~rust"),
        "a fence the vendor labelled `rust` was left for rustdoc to compile:\n{types}"
    );
    assert!(
        types.contains("```json"),
        "a language rustdoc does not run was renamed for nothing:\n{types}"
    );
    assert!(
        types.contains("//! in passing"),
        "a line mentioning a doc comment did not survive:\n{types}"
    );
    assert!(
        types.contains("*    the content of that item."),
        "a hanging list item lost its marker, or kept the gap that opens a code \
         block inside it:\n{types}"
    );
    assert!(
        types.contains("1.    the content of that item as well."),
        "an ordered marker is a list marker too:\n{types}"
    );
    assert!(
        types.contains("* one item under an asterisk") && types.contains("* and a second one"),
        "a list the vendor bulleted with asterisks is spelled as they spelled it:\n{types}"
    );

    // The same rule reaching a wrapper's doc, where the vendor's prose is a
    // summary rather than a property's description. A wrapper's doc has the
    // generator's paragraphs after the summary, so neither place stands in for
    // the other.
    let ops = read(&dir.join("src/ops.rs"));
    assert!(
        ops.contains("*    A summary the vendor hung from a marker."),
        "a hanging list item in a summary was left as the vendor wrote it:\n{ops}"
    );
}

/// The five shapes that a reading of the text line by line does not see. Each
/// fence comes out naming a language that rustdoc leaves alone, in the container
/// it was written in, and the indented block in a quote, which has no word to
/// change, comes out with its indentation capped, as a paragraph. In each the
/// words are the vendor's.
///
/// The exact comment is asserted, one per shape, and not a search of the file:
/// a fence rewritten in some other item's comment would satisfy a search.
#[test]
fn a_fence_inside_a_container_names_a_language_rustdoc_leaves_alone() {
    let dir = out("prose-containers");
    Settings::new(wrote(&dir, "document.yaml", PROSE))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert_eq!(
        doc_with(&types, "QUOTED-FENCE"),
        "A note in a block quote:\n\n> Example request:\n> ```text\n> QUOTED-FENCE is not Rust\n> ```",
        "a fence in a block quote"
    );
    assert_eq!(
        doc_with(&types, "LISTED-FENCE"),
        "Steps:\n\n- ```text\n  LISTED-FENCE is not Rust either\n  ```",
        "a fence opened on the line of a list marker"
    );
    assert_eq!(
        doc_with(&types, "CLASSED-FENCE"),
        "An example with a class on its fence:\n\n```text\n{\"CLASSED-FENCE\": \"1\"}\n```",
        "an info string that is only a class"
    );
    assert_eq!(
        doc_with(&types, "TARGETED-FENCE"),
        "An example ignored on one target:\n\n```text\n{\"TARGETED-FENCE\": \"2\"}\n```",
        "an info string that skips a target and runs on every other"
    );
    assert_eq!(
        doc_with(&types, "QUOTED-INDENT"),
        "Quoted and indented:\n\n>    QUOTED-INDENT is not Rust",
        "an indented block in a block quote has no info string to change, so \
         its indentation is capped as everywhere else"
    );
    assert!(
        doc_with(&read(&dir.join("src/ops.rs")), "SUMMARY-FENCE")
            .contains("> ```text\n> SUMMARY-FENCE is not Rust\n> ```"),
        "a fence in a block quote in an operation's summary"
    );
}

/// What the vendor indented is capped at three columns, wherever it stands, and
/// what the vendor fenced is theirs: a sample is not capped. A tab at the start
/// of a line is four columns, as Markdown counts it, so a sub-item that is
/// indented by a tab is still a sub-item.
#[test]
fn what_is_indented_is_capped_and_what_is_fenced_is_kept() {
    let descriptions = [
        "A list:\n\n- TABBED\n\t- b\n".to_owned(),
        format!("Quoted:\n\n>{}DEEP-QUOTE\n", " ".repeat(30)),
        "Before:\n\n- ```text\n  * STAR-IN-ITEM is kept\n        DEEP-IN-ITEM is no code\n  ```\n"
            .to_owned(),
        "```json\n* STAR-AT-TOP\n      DEEP-AT-TOP\n```\n".to_owned(),
    ];
    let dir = out("prose-capped");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 0)))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert_eq!(doc_with(&types, "TABBED"), "A list:\n\n- TABBED\n   - b");
    assert_eq!(doc_with(&types, "DEEP-QUOTE"), "Quoted:\n\n>    DEEP-QUOTE");
    assert_eq!(
        doc_with(&types, "STAR-IN-ITEM"),
        "Before:\n\n- ```text\n  * STAR-IN-ITEM is kept\n        DEEP-IN-ITEM is no code\n  ```"
    );
    assert_eq!(
        doc_with(&types, "STAR-AT-TOP"),
        "```json\n* STAR-AT-TOP\n      DEEP-AT-TOP\n```"
    );
}

/// Hand the doc comments of the generated `src/types.rs` and `src/ops.rs` in `dir`
/// to `rustdoc --test`, and fail naming the file in which it finds a test to run.
///
/// `name` keeps the files this writes apart from those of another call that
/// shares `dir`.
fn assert_nothing_runs(dir: &Path, name: &str) {
    for file in ["types", "ops"] {
        let comments = doc_comments_alone(&read(&dir.join(format!("src/{file}.rs"))));
        let said = rustdoc_tests(dir, &format!("{name}_{file}.rs"), &comments);
        assert!(
            said.contains("running 0 tests"),
            "rustdoc found something to run in src/{file}.rs:\n{}",
            said.lines().take(40).collect::<Vec<_>>().join("\n")
        );
    }
}

/// Whether `source` holds no `/** */` comment: a description is written as `///`
/// lines, which `rustfmt` and rustc leave as they stand.
fn no_block_comment(source: &str) -> bool {
    !source
        .lines()
        .any(|line| line.trim_start().starts_with("/**"))
}

/// A star in front of every line of a description is not taken out of it.
///
/// rustc rebuilds the text of a `/* */` comment by taking a column of stars out
/// when every line it counts starts with one, and a thematic break `***`, a line
/// that opens with `**bold**` and a paragraph that is bold from its first word are
/// such lines. What is left of a line after that is another line: `**     code`
/// is then `*     code`, a list item that holds an indented block, and a
/// paragraph above a rule is left four columns in. A `///` line is read as
/// written, so each description below comes out as its own lines, with nothing
/// to run and nothing missing.
#[test]
fn a_star_in_front_of_every_line_of_a_description_is_left_where_it_is() {
    let descriptions = [
        "**Note** text NOTE-LINE\n**     let _ = 1;\n".to_owned(),
        "Para RULED-ONE.\n   ***\n".to_owned(),
        "Para RULED-TWO.\n   ***\n   ***\n".to_owned(),
        "Para RULED-BOLD.\n  **bold** a\n  **bold** b\n".to_owned(),
        "* STARRED-ALL\n* b\n".to_owned(),
        "**First** PLAIN-FIRST\n   ***\n".to_owned(),
    ];
    let dir = out("prose-stars");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 400)))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    for file in ["types", "ops"] {
        let source = read(&dir.join(format!("src/{file}.rs")));
        assert!(no_block_comment(&source), "{file}.rs holds a block comment");
    }
    for (description, marker) in descriptions.iter().zip([
        "NOTE-LINE",
        "RULED-ONE",
        "RULED-TWO",
        "RULED-BOLD",
        "STARRED-ALL",
        "PLAIN-FIRST",
    ]) {
        assert_eq!(
            doc_with(&types, marker),
            description.trim_end(),
            "{marker}: the lines the vendor wrote"
        );
    }
    assert_nothing_runs(&dir, "stars");
}

/// A `"` that opens on one line and closes on a later one is not a reason for a
/// line to move.
///
/// `rustfmt` takes the text between such quotes for a string, and lays out the
/// body of a `/* */` comment by leaving a line inside a string at its own column
/// and putting every other line at the column of the item: the paragraphs of a
/// description come out four columns in and the lines inside the quotes at none,
/// which makes the paragraphs code. A `///` line is left where it is, so the
/// lines of each description below come out as they were written, as a type's
/// doc, a field's and a wrapper's.
#[test]
fn a_quote_that_runs_over_a_line_break_leaves_every_line_where_it_is() {
    let descriptions = [
        "A \"quote that\nspans the lines\n\n   and closes\" later. OVER-LINES".to_owned(),
        "First paragraph. STRAY\n\nA stray \" in the second.\nAnd a third line.\n\nAnd a fourth paragraph."
            .to_owned(),
        "Shown as `\"` in a span. IN-A-SPAN\n\nAnd then this paragraph.".to_owned(),
    ];
    let dir = out("prose-quotes");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 400)))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert!(no_block_comment(&types), "a description is a block comment");
    for (description, marker) in descriptions
        .iter()
        .zip(["OVER-LINES", "STRAY", "IN-A-SPAN"])
    {
        assert_eq!(
            doc_with(&types, marker),
            description.trim_end(),
            "{marker}: the lines the vendor wrote"
        );
    }
    assert_nothing_runs(&dir, "quotes");
}

/// The settings an adopter formats with are not a way to move a line of a
/// description.
///
/// `rustfmt` writes the indentation of an item as tabs under `hard_tabs`, and it
/// would write the body of a `/* */` comment with it: each run of `tab_spaces`
/// leading spaces would be a tab, rustdoc strips one tab and reads the next as
/// four columns, so at a width of 2 or 3 a vendor's line indented by two or
/// three spaces would be a code block. With `format_code_in_doc_comments` it
/// puts a space after the slashes of every line in a fence that has none, so a
/// sample would gain a column. A `///` line that starts with a space is a fixed
/// point of both, and the indentation of its item is outside it.
///
/// The matrix, the corpus and a few lines of each kind are written under each
/// setting, and each setting is judged by `rustdoc --test` over both files,
/// which finds nothing to run, and by the exact text of a sample. No page is
/// rendered. Every description is a field, and the first 400 are a type and a
/// wrapper too, which keeps the four settings quick.
///
/// `rustfmt` reads `hard_tabs` on every toolchain, and the file holding a tab in
/// front of `///` shows it did. It reads `format_code_in_doc_comments` on a
/// nightly only, and a stable `rustfmt` ignores the option with a warning, so
/// that setting is judged only where a probe shows the option changed the code in
/// a doc comment (`formats_doc_code`).
#[test]
fn no_setting_of_rustfmt_moves_a_line_of_a_description() {
    let mut descriptions = blocks();
    descriptions.extend(random_descriptions(400));
    descriptions.extend([
        "Para.\n\n  Two in. TWO\n\n   Three in.\n\n    Four in.\n".to_owned(),
        "A \"quote that\nspans the lines\n\n   and closes\" later.".to_owned(),
        "Sample SAMPLE:\n\n```json\n{\"at\": \"the margin\"}\n\n  {\"in\": 2}\n```\n".to_owned(),
    ]);
    let document = described(&descriptions, 400);
    for (name, config) in [
        ("tabs-2", "hard_tabs = true\ntab_spaces = 2\n"),
        ("tabs-3", "hard_tabs = true\ntab_spaces = 3\n"),
        ("tabs-4", "hard_tabs = true\ntab_spaces = 4\n"),
        (
            "doc-code",
            "unstable_features = true\nformat_code_in_doc_comments = true\n",
        ),
    ] {
        let dir = out(&format!("prose-settings-{name}"));
        wrote(&dir, "rustfmt.toml", config);
        if config.contains("format_code_in_doc_comments") && !formats_doc_code(&dir) {
            continue;
        }
        Settings::new(wrote(&dir, "document.json", &document))
            .write_to(&dir)
            .expect("the document generates");
        let types = read(&dir.join("src/types.rs"));

        assert!(
            !config.contains("hard_tabs") || types.contains("\t///"),
            "`rustfmt` did not read the settings `{name}`, so this proves nothing"
        );
        assert_eq!(
            doc_with(&types, "SAMPLE"),
            "Sample SAMPLE:\n\n```json\n{\"at\": \"the margin\"}\n\n  {\"in\": 2}\n```",
            "{name}: a sample as the vendor wrote it"
        );
        assert_nothing_runs(&dir, name);
    }
}

/// Whether the `rustfmt` on `PATH` formats the code in a doc comment under the
/// settings in `dir`, which `format_code_in_doc_comments` asks for and a stable
/// `rustfmt` ignores.
fn formats_doc_code(dir: &Path) -> bool {
    let path = wrote(
        dir,
        "probe.rs",
        "/// ```\n/// let   x=1;\n/// ```\npub struct Probe;\n",
    );
    let ran = std::process::Command::new("rustfmt")
        .args(["--edition", "2024"])
        .arg(&path)
        .output()
        .expect("rustfmt is on PATH, as a bless step needs it");
    ran.status.success() && read(&path).contains("let x = 1;")
}

/// A line that starts with a `/` is a line of its doc comment, and the comment
/// holds nothing but `///` lines.
///
/// After the slashes of a `///` line it would make `////`, which is no doc
/// comment, and `prettyplease` writes such a line as an attribute instead. That
/// holds for a description of one line as for one of several, and the
/// descriptions below are both. The judges are given every comment of both
/// files, and `doc_comments_alone` fails on a doc attribute they would not be
/// given.
#[test]
fn a_line_that_starts_with_a_slash_stays_a_line_of_its_doc_comment() {
    let descriptions = [
        "First line.\n/second line SLASHED\n//third line.\n".to_owned(),
        "/one slash ALONE-ONE".to_owned(),
        "//two slashes ALONE-TWO".to_owned(),
        "/// three slashes ALONE-THREE".to_owned(),
    ];
    let dir = out("prose-slash");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 400)))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert_eq!(
        doc_with(&types, "SLASHED"),
        "First line.\n/second line SLASHED\n//third line."
    );
    for (description, marker) in
        descriptions
            .iter()
            .skip(1)
            .zip(["ALONE-ONE", "ALONE-TWO", "ALONE-THREE"])
    {
        assert_eq!(
            doc_with(&types, marker),
            *description,
            "{marker}: a description of one line"
        );
        assert!(
            doc_with(&read(&dir.join("src/ops.rs")), marker)
                .starts_with(&format!("{description}\n")),
            "{marker}: the summary opens the wrapper's doc"
        );
    }
    for file in ["types", "ops"] {
        assert!(
            !read(&dir.join(format!("src/{file}.rs"))).contains("#[doc"),
            "a line of a description is an attribute in {file}.rs, not a comment line"
        );
    }
    assert_nothing_runs(&dir, "slash");
}

/// The Markdown containers a block can stand in, as the text before the first
/// line of the block and the text before each line after it.
const CONTAINERS: [(&str, &str); 17] = [
    ("", ""),
    ("> ", "> "),
    (">", ">"),
    ("   > ", "   > "),
    ("> > ", "> > "),
    ("- ", "  "),
    ("* ", "  "),
    ("+ ", "  "),
    ("-   ", "    "),
    ("1. ", "   "),
    ("1) ", "   "),
    ("- - ", "    "),
    ("- > ", "  > "),
    ("> - ", ">   "),
    ("> 1. ", ">    "),
    ("[^note]: ", "    "),
    ("   ", "   "),
];

/// What can follow the fence of a block: the words rustdoc reads as Rust, the
/// ones it reads as another language, the attributes it has a syntax for and
/// the spellings that Markdown resolves before rustdoc sees them.
const INFOS: [&str; 40] = [
    "",
    " ",
    "rust",
    "Rust",
    "rust,ignore",
    "rust edition2018",
    "rust2015",
    "rust2018",
    "rust2021",
    "rust2024",
    "rust2027",
    "rustc",
    "rust_2018",
    "ignore",
    "ignore-wasm32",
    "ignore-",
    "should_panic",
    "no_run",
    "compile_fail",
    "test_harness",
    "standalone_crate",
    "edition2021",
    "E0308",
    "custom",
    "text",
    "json",
    "json,ignore",
    "ignore,json",
    "{.json}",
    "{.rust}",
    ".json",
    "json {.x}",
    "rust{.x}",
    "key=value",
    "class=\"rust\"",
    "{",
    "\"",
    "r&#117;st",
    "&#105;gnore",
    "j\\son",
];

/// Every block of every container in `CONTAINERS`, in the descriptions a
/// vendor could write them in.
fn blocks() -> Vec<String> {
    let mut texts = Vec::new();
    for (first, rest) in CONTAINERS {
        let apart = rest.trim_end();
        for info in INFOS {
            for mark in ["```", "~~~"] {
                texts.push(format!("{first}{mark}{info}\n{rest}CODE\n{rest}{mark}\n"));
                texts.push(format!(
                    "Intro.\n\n{first}{mark}{info}\n{rest}CODE\n{rest}{mark}\n"
                ));
            }
        }
        texts.push(format!("{first}````rust\n{rest}CODE\n"));
        texts.push(format!(
            "{first}```rust\n{rest}CODE\n{rest}```\n{rest}MORE\n"
        ));
        for indent in ["    ", "        ", "\t", "\t\t", "  \t"] {
            texts.push(format!("Intro.\n\n{first}{indent}CODE\n"));
            texts.push(format!("{first}Intro.\n{apart}\n{rest}{indent}CODE\n"));
            texts.push(format!(
                "{first}Intro.\n{apart}\n{rest}{indent}CODE\n{apart}\n{rest}{indent}{indent}MORE\n"
            ));
        }
    }
    texts
}

/// No container and no info string gets a vendor's sentence run by `rustdoc`.
///
/// This is the oracle and not a copy of its rule. The comments of a document
/// that holds every block of every container are handed to `rustdoc --test`,
/// which is the program the adopter's `cargo test --doc` runs, so a spelling
/// that rustdoc reads as Rust in a version this test was not written against
/// fails here. A comment that is a test would be listed, and one that is
/// `ignore`d is listed too. The blocks stand in a type, a field and a wrapper,
/// which are three places of two files, and both files are judged.
#[test]
fn no_container_and_no_info_string_gets_a_vendors_sentence_run_by_rustdoc() {
    let dir = out("prose-matrix");
    let descriptions = blocks();
    Settings::new(wrote(
        &dir,
        "document.json",
        &described(&descriptions, descriptions.len()),
    ))
    .write_to(&dir)
    .expect("the document generates");
    let comments = doc_comments_alone(&read(&dir.join("src/types.rs")));
    let wrappers = doc_comments_alone(&read(&dir.join("src/ops.rs")));
    assert!(
        comments.matches("pub f").count() >= 2 * descriptions.len(),
        "a description did not reach a field and a type, so this proves less than it says"
    );
    assert!(
        wrappers.matches("pub f").count() >= descriptions.len(),
        "a description did not reach a wrapper, so this proves less than it says"
    );

    assert_nothing_runs(&dir, "matrix");

    // The same file with a doctest of Rust's own added, so that "running 0
    // tests" above is the absence of tests and not a rustdoc that cannot
    // read the file.
    let control = format!("{comments}\n/// ```\n/// let _ = 1;\n/// ```\npub struct Control;\n");
    assert!(
        rustdoc_tests(&dir, "control_docs.rs", &control).contains("running 1 test\n"),
        "rustdoc does not find a plain doctest in the same file"
    );
}

/// A small generator of numbers that is the same on every run, so that a
/// description that fails is the same description when the test is run again.
struct Dice(u64);

impl Dice {
    fn roll(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a>(&mut self, of: &[&'a str]) -> &'a str {
        let at = usize::try_from(self.roll() % of.len() as u64).expect("an index");
        of[at]
    }
}

/// What a line of a description can start with: nothing, indentation of every
/// kind, white space that is no indentation, and the openers of each container
/// Markdown has.
const STARTS: &[&str] = &[
    "",
    "",
    "",
    " ",
    "  ",
    "   ",
    "    ",
    "      ",
    "\t",
    "  \t",
    "    \t",
    "\u{a0}",
    "\u{a0}    ",
    "\u{c}",
    "\u{2003}  ",
    "> ",
    ">",
    ">     ",
    "   > ",
    "> > ",
    "- ",
    "-     ",
    "* ",
    "+ ",
    "1. ",
    "1.    ",
    "10. ",
    "1) ",
    "- [ ] ",
    "[^a]: ",
    "[a]: ",
    "# ",
    "  - ",
    "-\t",
    ">\t",
    "|",
    "<div>",
    "\\",
    "&#32;",
    "/",
];

/// What a line of a description can say after its start: the fences of every
/// kind, code, HTML in every position, the lines that Markdown reads as
/// something else, and the characters that no comment may hold.
const SAYINGS: &[&str] = &[
    "```",
    "```rust",
    "``` ",
    "```ignore-x",
    "```{.json}",
    "```json",
    "```<vendorx>",
    "``` ```",
    "~~~",
    "~~~ rust",
    "~~~~",
    "````",
    "```text",
    "CODE",
    "more",
    "",
    "",
    "<vendorx>",
    "</vendorx>",
    "<vendorx a=\"b\">x</vendorx>",
    "<vendorx",
    "<<vendorx>>",
    "`<vendorx>`",
    "`",
    "``",
    "\\<vendorx>",
    "&lt;vendorx&gt;",
    "&#60;vendorx>",
    "[a](<vendorx>)",
    "[a]: <vendorx>",
    "![i](x \"<vendorx>\")",
    "<!--VENDOR",
    "-->",
    "<https://example.com/a>",
    "[a](javascript:vendorx)",
    "[a](JaVaScRiPt:vendorx)",
    "[a](&#106;avascript:vendorx)",
    "![i](data:text/html,vendorx)",
    "[a][r]",
    "[r]: vbscript:vendorx",
    "[a](https://example.com/a)",
    "[a](./a) [b](#b) [c](mailto:a@example.com)",
    "a \"quote",
    "closes\" here",
    "**",
    "**     ```rust",
    "*     CODE",
    "a < b",
    "---",
    "-----",
    "=====",
    "***",
    "* * *",
    "[^a]",
    "| a | b |",
    "| - | - |",
    "    CODE",
    "\tCODE",
    "    ```",
    "<pre>",
    "</pre>",
    "<span>",
    "\u{202e}x",
    "a\rb",
    "a\r\nb",
];

/// Descriptions of one to nine lines, each a few starts and a saying.
fn random_descriptions(count: usize) -> Vec<String> {
    let mut dice = Dice(0x9E37_79B9_7F4A_7C15);
    (0..count)
        .map(|_| {
            let lines = 1 + dice.roll() % 9;
            let mut text = (0..lines)
                .map(|_| {
                    let starts = dice.roll() % 3;
                    let start: String = (0..=starts).map(|_| dice.pick(STARTS)).collect();
                    format!("{start}{}", dice.pick(SAYINGS))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if dice.roll() % 2 == 0 {
                text.push('\n');
            }
            text
        })
        .collect()
}

/// A description that a generator of the shapes above wrote is neither run by
/// rustdoc nor shown as the markup it holds, nor a way to a script.
///
/// The matrix above names the shapes. This is every other way to put them
/// together, a few thousand of them from a generator that does not change, and
/// the same judges: `rustdoc --test` says whether anything runs, and the page
/// that rustdoc renders says whether an element of the vendor's is on it and
/// where each link on it leads. `vendorx` is an element that no page of
/// rustdoc's has, and `VENDOR` opens a comment that no page has either.
#[test]
fn a_few_thousand_descriptions_put_together_at_random_are_neither_run_nor_shown_as_markup() {
    let dir = out("prose-random");
    let descriptions = random_descriptions(3000);
    Settings::new(wrote(
        &dir,
        "document.json",
        &described(&descriptions, descriptions.len()),
    ))
    .write_to(&dir)
    .expect("the document generates");

    assert_nothing_runs(&dir, "random");
    for file in ["types", "ops"] {
        let comments = doc_comments_alone(&read(&dir.join(format!("src/{file}.rs"))));
        let page = rustdoc_page(&dir, &format!("random_{file}_page.rs"), &comments);
        assert!(
            !page.contains("<vendorx") && !page.contains("<!--VENDOR"),
            "an element or a comment of the vendor's is on the page of {file}.rs"
        );
        assert!(
            page.contains("&lt;vendorx"),
            "the page of {file}.rs shows nothing of the vendor's markup, so this proves less than it says"
        );
        assert_eq!(
            unknown_scheme(&page),
            None,
            "a link or an image on the page of {file}.rs leads to a scheme that is not known to be safe"
        );
        assert!(
            page.contains("href=\"https://example.com/a\""),
            "the page of {file}.rs holds none of the vendor's links, so this proves less than it says"
        );
    }
}

/// The first `href` or `src` on `page` that names a scheme other than `http`,
/// `https` and `mailto`, if there is one.
///
/// A value with a `:` before any `/`, `?` or `#` names a scheme. The page is the
/// evidence, and what it holds beside the vendor's links is rustdoc's own, which
/// are relative or `https`.
fn unknown_scheme(page: &str) -> Option<String> {
    ["href=\"", "src=\""]
        .iter()
        .flat_map(|attribute| page.split(attribute).skip(1))
        .filter_map(|after| after.split('"').next())
        .find(|value| {
            let before = value.split(['/', '?', '#']).next().unwrap_or_default();
            let known = ["http:", "https:", "mailto:"].iter().any(|scheme| {
                value
                    .get(..scheme.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
            });
            before.contains(':') && !known
        })
        .map(str::to_owned)
}

/// The page that `rustdoc` renders for the comments of `src/{file}.rs` in `dir`,
/// after `rustc` has accepted the comments and the page has been found to lead to
/// no scheme that is unknown to be safe.
fn judged_page(dir: &Path, file: &str) -> String {
    let comments = doc_comments_alone(&read(&dir.join(format!("src/{file}.rs"))));
    let refused = rustc_accepts(dir, "hostile_docs.rs", &comments);
    assert!(refused.is_ok(), "rustc refuses {file}.rs:\n{refused:?}");
    let page = rustdoc_page(dir, &format!("hostile_{file}_page.rs"), &comments);
    assert_eq!(
        unknown_scheme(&page),
        None,
        "a link on the page of {file}.rs leads to a script"
    );
    page
}

/// The control document, which a vendor could have written by accident, holds
/// the five shapes that hide a code block, markup, a link to a script, a
/// character that changes the direction of text and a carriage return alone, in
/// descriptions and in two operation summaries. The judges are put to both
/// files as they are to the matrix: `rustdoc --test` finds nothing to run,
/// `rustc` accepts the comments, and the page of each file leads to no script.
/// The element of the vendor's is on the page of `types.rs` as text, and the
/// link in the summary of an operation is on the page of `ops.rs` as text.
#[test]
fn the_control_document_hands_rustdoc_nothing_to_run_and_the_compiler_nothing_to_refuse() {
    let dir = out("prose-hostile");
    Settings::new(HOSTILE)
        .write_to(&dir)
        .expect("the control document blesses");

    assert_nothing_runs(&dir, "hostile");
    let types_page = judged_page(&dir, "types");
    let ops_page = judged_page(&dir, "ops");
    assert!(
        !types_page.contains("<blink>"),
        "the vendor's element is on the page"
    );
    assert!(types_page.contains("&lt;blink&gt;"));
    assert!(
        types_page.contains("href=\"https://kiln.example/rules\""),
        "a link to a page is a link"
    );
    assert!(
        ops_page.contains("[the tally sheet](javascript:alert(1))"),
        "the link to a script in a summary is shown as the text it is written in"
    );
    let ops = read(&dir.join("src/ops.rs"));
    assert!(
        doc_with(&ops, "tally sheet")
            .contains("> ```text\n> curl https://kiln.example/wadding/tallies"),
        "a fence in the summary of an operation is inert"
    );
}

/// The nine characters that rustc refuses in a comment, with the lint
/// `text_direction_codepoint_in_comment`: the embeddings, the overrides, the
/// isolates and the two that end them.
const BIDI: [char; 9] = [
    '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}',
    '\u{2069}',
];

/// A bidirectional control in a description is a comment line that rustc
/// refuses to compile, and a `#![allow]` in the module cannot reach it because
/// the lint is raised by the lexer. The comments of the document are handed to
/// `rustc` itself, which is what answers whether the crate compiles.
#[test]
fn a_bidi_control_in_a_description_is_written_escaped_and_the_comments_compile() {
    let marks: String = BIDI.iter().flat_map(|mark| ['x', *mark]).collect();
    let descriptions = [
        "The label, e.g. \u{202b}abc\u{202c} as shown.".to_owned(),
        format!("Every one: {marks}end"),
        "Fenced:\n\n```json\n{\"k\": \"\u{202e}FENCED\"}\n```\n".to_owned(),
        "Spanned `\u{2066}SPANNED` here.".to_owned(),
    ];
    let dir = out("prose-bidi");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 400)))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert!(
        !types.contains(BIDI),
        "a generated file holds a character that changes the direction of text"
    );
    assert_eq!(
        doc_with(&types, "e.g."),
        "The label, e.g. \\u{202b}abc\\u{202c} as shown.",
        "the character is written as its code point, in Rust's own spelling"
    );
    assert_eq!(
        doc_with(&types, "FENCED"),
        "Fenced:\n\n```json\n{\"k\": \"\\u{202e}FENCED\"}\n```",
        "a sample is escaped too, because the compiler reads its line"
    );
    assert_eq!(
        doc_with(&types, "SPANNED"),
        "Spanned `\\u{2066}SPANNED` here."
    );

    let refused = rustc_accepts(&dir, "bidi_docs.rs", &doc_comments_alone(&types));
    assert!(
        refused.is_ok(),
        "rustc refuses the doc comments:\n{refused:?}"
    );
}

/// A carriage return that no line feed follows ends a line, as Markdown reads it,
/// and a pair of them ends one line and not two. The compiler refuses a bare one
/// in a doc comment, and `rustfmt` refuses the file that holds one, so the
/// description is written with line feeds alone.
#[test]
fn a_lone_carriage_return_in_a_description_is_a_line_break() {
    let descriptions = [
        "A carriage\rreturn alone.".to_owned(),
        "First line.\r\nSecond line.".to_owned(),
        "Fenced:\r```\rCODE\r```\r".to_owned(),
    ];
    let dir = out("prose-cr");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 400)))
        .write_to(&dir)
        .expect("a carriage return is no reason to refuse a bless");
    let types = read(&dir.join("src/types.rs"));

    assert!(!types.contains('\r'), "a carriage return reached the file");
    assert_eq!(
        doc_with(&types, "return alone"),
        "A carriage\nreturn alone."
    );
    assert_eq!(doc_with(&types, "Second line"), "First line.\nSecond line.");
    assert_eq!(doc_with(&types, "CODE"), "Fenced:\n```text\nCODE\n```");
}

/// What a vendor writes as HTML is shown as the characters it is written in.
/// The rendered page is the evidence, since `cargo doc` accepts the markup
/// without a warning once `rustdoc::all` is allowed, and the page then carries
/// the vendor's element as an element.
#[test]
fn vendor_html_is_shown_as_text_on_the_rendered_page() {
    let descriptions = [
        "<script>alert(1)</script> and a < b, an autolink <https://example.com/a>, \
         a code span `<b>x</b>`, an escaped \\<i>, an escaped backslash \\\\<u> \
         and a sample:\n\n\
         ```html\n<div>SAMPLE</div>\n```\n"
            .to_owned(),
        "<div>\nan escaped \\<i> inside BLOCKED\n</div>\n".to_owned(),
    ];
    let dir = out("prose-html");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 400)))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert_eq!(
        doc_with(&types, "SAMPLE"),
        "\\<script>alert(1)\\</script> and a \\< b, an autolink \\<https://example.com/a>, \
         a code span `<b>x</b>`, an escaped \\<i>, an escaped backslash \\\\\\<u> \
         and a sample:\n\n\
         ```html\n<div>SAMPLE</div>\n```"
    );

    assert_eq!(
        doc_with(&types, "BLOCKED"),
        "\\<div>\nan escaped \\<i> inside BLOCKED\n\\</div>",
        "a block of HTML is escaped by its first `<` on each line, and a `<` \
         that is escaped already is left"
    );

    let page = rustdoc_page(&dir, "page_docs.rs", &doc_comments_alone(&types));
    assert!(
        !page.contains("<script>alert(1)</script>"),
        "the vendor's element is on the page"
    );
    assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(
        !page.contains("href=\"https://example.com/a\""),
        "an autolink is a link to the vendor's choice of address"
    );
    assert!(page.contains("&lt;b&gt;x&lt;/b&gt;"), "a code span is text");
}

/// Links and images that lead to a scheme that is not known to be safe, with the
/// text that the generated doc holds of each: the `[` that opens it is escaped.
///
/// The destination is asked about as the parser hands it over, so a spelling that
/// a browser reads as `javascript:` is no way round the rule: a capital, an
/// entity, a tab in the name, a reference whose definition holds the scheme, a
/// link inside the text of another.
const SHOWN_LINKS: [(&str, &str); 9] = [
    (
        "[a](javascript:alert(1)) LINK-JS",
        "\\[a](javascript:alert(1)) LINK-JS",
    ),
    (
        "[a](JaVaScRiPt:alert(1)) LINK-CASE",
        "\\[a](JaVaScRiPt:alert(1)) LINK-CASE",
    ),
    (
        "[a](&#106;avascript:alert(1)) LINK-ENTITY",
        "\\[a](&#106;avascript:alert(1)) LINK-ENTITY",
    ),
    (
        "[a](<java\tscript:alert(1)>) LINK-TAB",
        "\\[a](\\<java\tscript:alert(1)>) LINK-TAB",
    ),
    (
        "[a](data:text/html,x) LINK-DATA",
        "\\[a](data:text/html,x) LINK-DATA",
    ),
    (
        "![a](javascript:x) LINK-IMAGE",
        "!\\[a](javascript:x) LINK-IMAGE",
    ),
    (
        "![a](data:image/svg+xml,x) LINK-IMAGE-DATA",
        "!\\[a](data:image/svg+xml,x) LINK-IMAGE-DATA",
    ),
    (
        "[a][r] and [s] LINK-REFERENCE\n\n[r]: javascript:alert(1)\n[s]: vbscript:x",
        "\\[a]\\[r] and \\[s] LINK-REFERENCE\n\n[r]: javascript:alert(1)\n[s]: vbscript:x",
    ),
    (
        "[[a](javascript:x)](javascript:y) LINK-NESTED",
        "\\[\\[a](javascript:x)](javascript:y) LINK-NESTED",
    ),
];

/// Links and images that lead to a page, a mail, a place on this page or a path,
/// which stay as the vendor wrote them.
const KEPT_LINKS: [&str; 10] = [
    "[a](https://example.com/a) KEEP-HTTPS",
    "[a](HTTP://example.com/b) KEEP-CASE",
    "[a](mailto:a@example.com) KEEP-MAIL",
    "[a](./a) KEEP-RELATIVE",
    "[a](a/b:c) KEEP-COLON-AFTER-SLASH",
    "[a](#fragment) KEEP-FRAGMENT",
    "[a](/absolute?x=a:b) KEEP-ABSOLUTE",
    "![a](https://example.com/a.png) KEEP-IMAGE",
    "[a]() KEEP-EMPTY",
    "[a][r] KEEP-REFERENCE\n\n[r]: https://example.com/r",
];

/// The word of `text` that starts with `prefix`, which names its description.
fn marked<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.split_whitespace()
        .find(|word| word.starts_with(prefix))
        .unwrap_or(text)
}

/// A link or an image is followed only to a scheme that is known to be safe, and
/// is shown as the characters it is written in everywhere else. The rendered page
/// is the evidence for both.
#[test]
fn a_link_is_followed_only_to_a_scheme_that_is_known_to_be_safe() {
    let descriptions: Vec<String> = SHOWN_LINKS
        .iter()
        .map(|(text, _)| (*text).to_owned())
        .chain(KEPT_LINKS.iter().map(|text| (*text).to_owned()))
        .collect();
    let dir = out("prose-links");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 400)))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    for (text, shown) in SHOWN_LINKS {
        assert_eq!(doc_with(&types, marked(text, "LINK-")), shown);
    }
    for text in KEPT_LINKS {
        assert_eq!(doc_with(&types, marked(text, "KEEP-")), text);
    }

    let page = rustdoc_page(&dir, "links_page.rs", &doc_comments_alone(&types));
    assert_eq!(
        unknown_scheme(&page),
        None,
        "a link or an image leads to a script"
    );
    for followed in [
        "href=\"https://example.com/a\"",
        "href=\"mailto:a@example.com\"",
        "href=\"#fragment\"",
        "href=\"https://example.com/r\"",
        "src=\"https://example.com/a.png\"",
    ] {
        assert!(page.contains(followed), "the page holds no `{followed}`");
    }
    assert!(
        page.contains("[a](javascript:alert(1))"),
        "a link to a script is shown as the text it is written in"
    );
}

/// A fence that names an edition of Rust is Rust to rustdoc, so it is renamed as
/// `rust` is, and a language that rustdoc leaves alone keeps its name.
///
/// `rust2018` and `rust2021` are words of rustdoc's own, which a list of the
/// words kept would have to know, so every word that starts with `rust` is
/// renamed. The fences hold a statement that a doctest would run, and rustdoc is
/// the judge of whether one does.
#[test]
fn a_fence_that_names_an_edition_of_rust_is_renamed() {
    let words = [
        "rust2015", "rust2018", "rust2021", "rust2024", "rustc", "rust",
    ];
    let mut descriptions: Vec<String> = words
        .iter()
        .map(|word| format!("```{word}\nlet _ = 1; // FENCE[{word}]\n```\n"))
        .collect();
    descriptions.push("```json\nlet _ = 1; // FENCE[json]\n```\n".to_owned());
    let dir = out("prose-editions");
    Settings::new(wrote(&dir, "document.json", &described(&descriptions, 400)))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    for word in words {
        assert_eq!(
            doc_with(&types, &format!("FENCE[{word}]")),
            format!("```text\nlet _ = 1; // FENCE[{word}]\n```"),
            "`{word}` names Rust to rustdoc or starts like a word that does"
        );
    }
    assert_eq!(
        doc_with(&types, "FENCE[json]"),
        "```json\nlet _ = 1; // FENCE[json]\n```",
        "the name of a language rustdoc leaves alone is the vendor's"
    );
    assert_nothing_runs(&dir, "editions");
}

/// A type the adopter owns, reached from a wrapper by both routes: a parameter
/// points at a named schema carrying the format, and the response states the
/// format itself. The schema's name is what the adopter's path ends in, which
/// is the coincidence the rule does not turn on.
const OWNED: &str = r##"
openapi: 3.0.3
info: { title: Owned, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /prices:
    get:
      operationId: listPrices
      parameters:
        - name: over
          in: query
          schema: { $ref: "#/components/schemas/Cents" }
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { type: string, format: money }
components:
  schemas:
    Cents:
      type: string
      format: money
"##;

/// A schema stating the format without naming it has no type in the generated
/// module, because typify defined none — the adopter's own path is the answer,
/// and it is good anywhere the generated crate compiles. A wrapper reaching for
/// `crate::types::Cents` there would name a module the type was never in, and a
/// rule that derived the name instead of asking would have no way to know that.
///
/// The named schema beside it is the other half of the same reading: it has a
/// type, so the wrapper names that. `Names` tells the two apart by asking
/// typify what each became, never by looking at the format.
///
/// This is route 2 whole: `replace` is how an adopter owns a type no document
/// can describe, and a parameter is an ordinary place to spend it.
#[test]
fn a_schema_the_adopter_owns_is_named_by_the_adopters_own_path() {
    let dir = out("owned");
    Settings::new(wrote(&dir, "document.yaml", OWNED))
        .replace("money", "cents::Cents")
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));
    let ops = dense(&read(&dir.join("src/ops.rs")));

    assert!(
        ops.contains("Call<'_,cents::Cents>"),
        "the wrapper does not answer with the type the adopter owns:\n{ops}"
    );
    assert!(
        ops.contains("over:::std::option::Option<crate::types::Cents>"),
        "the wrapper does not take the type the named schema became:\n{ops}"
    );
    assert!(
        types.contains("pub struct Cents(pub cents::Cents);"),
        "the named schema is not a newtype over the type the adopter owns:\n{types}"
    );
}

/// A document with paths and no `components` block at all. Nothing in OpenAPI
/// requires one, and an API whose operations take and return nothing but
/// scalars declares no schema to put in it.
const NAMELESS: &str = r#"
openapi: 3.0.3
info: { title: Nameless, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /ping:
    get:
      operationId: ping
      responses:
        "200": { description: OK }
"#;

/// A document that names no schema generates like any other: the wrappers name
/// no type, and the types file carries what typify writes for itself. Refusing
/// it would be this crate inventing a requirement the format does not have.
#[test]
fn a_document_that_names_no_schema_still_generates() {
    let dir = out("nameless");
    Settings::new(wrote(&dir, "document.yaml", NAMELESS))
        .write_to(&dir)
        .expect("a document with no components is a document");

    assert!(
        read(&dir.join("src/ops.rs")).contains("pub fn ping(&self)"),
        "the operation did not reach a wrapper"
    );
    assert!(
        !read(&dir.join("src/types.rs")).contains("crate::types::"),
        "a wrapper names a type in a document that declares none"
    );
}

/// A named schema whose whole shape the adopter owns the type for. The schema
/// is called one thing and the type another, which is the ordinary case and
/// the one typify answers by wrapping.
const PROMISED: &str = r#"
openapi: 3.0.3
info: { title: Promised, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /prices:
    get:
      operationId: listPrices
      responses:
        "200": { description: OK }
components:
  schemas:
    Amount:
      type: string
      format: cash
"#;

/// The `const _` block the generated file carries, from `const` to its closing
/// brace at column zero.
fn promise_in(types: &str) -> String {
    let block: Vec<&str> = types
        .lines()
        .skip_while(|line| !line.starts_with("const _: () = {"))
        .take_while(|line| *line != "};")
        .collect();
    format!("{}\n}};\n", block.join("\n"))
}

/// `Settings::replace` promises typify that the adopter's type parses from a
/// string and prints to one, and typify writes the newtype's `FromStr`,
/// `Display` and both `TryFrom`s in terms of that promise. The promise is
/// checked where it is made, because breaking it is one of the worst failures
/// this crate can hand an adopter.
#[test]
fn a_type_the_adopter_owns_is_held_to_what_replace_promised() {
    let dir = out("promised");
    Settings::new(wrote(&dir, "document.yaml", PROMISED))
        .replace("cash", "crate::Owned")
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert!(
        types.contains("parses_from_a_string::<crate::Owned>()")
            && types.contains("prints_to_a_string::<crate::Owned>()"),
        "the promise the generated newtype rests on is not checked:\n{types}"
    );
}

/// A type the adopter owns, with `FromStr` and without `Display`.
const HALF_KEPT: &str = "
pub struct Owned;
impl ::std::str::FromStr for Owned {
    type Err = ::std::convert::Infallible;
    fn from_str(_: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self)
    }
}
";

/// The whole value of the check is the message, so the message is what is
/// pinned: one error, naming the adopter's type and the trait it is missing,
/// against a line that says what asked for it. The alternative an adopter gets
/// without this is `E0599: no method named `fmt`` from inside a generated
/// newtype, or — for a missing `FromStr` — four `E0271`s and two `E0276`s
/// about an associated type that cannot be resolved.
#[test]
fn a_broken_promise_names_the_type_and_the_trait_it_lacks() {
    let dir = out("promise-broken");
    Settings::new(wrote(&dir, "document.yaml", PROMISED))
        .replace("cash", "crate::Owned")
        .write_to(&dir)
        .expect("the document generates");

    let promise = promise_in(&read(&dir.join("src/types.rs")));
    let path = wrote(&dir, "half_kept.rs", &format!("{HALF_KEPT}{promise}"));
    let ran = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "lib",
            "--emit",
            "metadata",
        ])
        .arg("-o")
        .arg(dir.join("half_kept.rmeta"))
        .arg(&path)
        .output()
        .expect("rustc is on PATH beside the rustfmt a bless step already needs");
    let complained = String::from_utf8_lossy(&ran.stderr);

    assert!(
        complained.contains("the trait `std::fmt::Display` is not implemented for `Owned`"),
        "the failure does not name the adopter's type and the trait it lacks:\n{complained}"
    );
    assert!(
        complained.contains("prints_to_a_string"),
        "the failure does not point at the line that says what asked for it:\n{complained}"
    );
}

/// The same format, declared inline rather than under a name.
const INLINE: &str = r#"
openapi: 3.0.3
info: { title: Inline, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /prices:
    get:
      operationId: listPrices
      responses:
        "200": { description: OK }
components:
  schemas:
    Price:
      type: object
      properties:
        figure: { type: string, format: cash }
"#;

/// typify emits a replaced type directly wherever it does not wrap it, and
/// writes nothing in terms of it — so there is no promise to keep and none is
/// demanded. Asking anyway would refuse a type for missing a trait the
/// generated code never uses.
#[test]
fn a_type_nothing_was_written_in_terms_of_is_asked_for_nothing() {
    let dir = out("inline");
    Settings::new(wrote(&dir, "document.yaml", INLINE))
        .replace("cash", "crate::Owned")
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert!(
        types.contains("crate::Owned"),
        "the adopter's type did not reach the generated field:\n{types}"
    );
    assert!(
        !types.contains("a_type_named_by_settings_replace"),
        "a promise is demanded where nothing rests on it:\n{types}"
    );
}

/// Two named schemas the adopter owns the same Rust type for, under two
/// formats: one schema is called what the type's path ends in and the other is
/// not.
const EITHER_NAME: &str = r#"
openapi: 3.0.3
info: { title: Either name, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /prices:
    get:
      operationId: listPrices
      responses:
        "200": { description: OK }
components:
  schemas:
    Cents:
      type: string
      format: cents
      description: An amount in whole cents.
    Amount:
      type: string
      format: amount
    Price:
      type: object
      required: [figure]
      properties:
        figure:
          type: string
          format: cents
          description: What it costs.
"#;

/// Whether a named schema wraps the type the adopter owns is not a coincidence
/// between two names. `Cents` is what `money::Cents` ends in and `Amount` is
/// not, and both are the same `#[serde(transparent)]` newtype over it.
///
/// The coincidence is what typify goes by, and it is the adopter's to pay for
/// in the wrong currency: a schema name is what stands in the corrected
/// document the vendor and every other consumer read, so "rename the schema to
/// get the bare type" asks them to spend a name they do not own. One behaviour,
/// whichever name is there, is the thing that can be reasoned about.
#[test]
fn a_named_schema_wraps_the_adopters_type_whatever_it_is_called() {
    let dir = out("either-name");
    Settings::new(wrote(&dir, "document.yaml", EITHER_NAME))
        .replace("cents", "money::Cents")
        .replace("amount", "money::Cents")
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert!(
        types.contains("pub struct Cents(pub money::Cents);"),
        "a schema called what the adopter's path ends in stands alone:\n{types}"
    );
    assert!(
        types.contains("pub struct Amount(pub money::Cents);"),
        "a schema called something else does not wrap:\n{types}"
    );
    // One rule means one obligation: both wrappers are written in terms of the
    // adopter's type, so the promise is demanded for a coinciding name exactly
    // as for any other.
    assert!(
        types.contains("parses_from_a_string::<money::Cents>()"),
        "a wrapper was written and nothing holds the type it rests on:\n{types}"
    );
}

/// A wrapper costs a name and a `.0`, and it must cost nothing on the wire: an
/// adoption that reads a field as the adopter's type and one that reads it as
/// the newtype are reading the same bytes. `#[serde(transparent)]` is what says
/// so, and it is asserted against the `struct` line so that a wrapper emitted
/// without it cannot pass.
///
/// What the attribute *does* is held where there is a serializer to run it:
/// `examples/toy/api/tests/money.rs` sends a value both ways.
#[test]
fn the_wrapper_a_named_schema_becomes_is_transparent_on_the_wire() {
    let dir = out("transparent");
    Settings::new(wrote(&dir, "document.yaml", EITHER_NAME))
        .replace("cents", "money::Cents")
        .replace("amount", "money::Cents")
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    for wrapper in ["Cents", "Amount"] {
        assert!(
            types.contains(&format!(
                "#[serde(transparent)]\npub struct {wrapper}(pub money::Cents);"
            )),
            "`{wrapper}` is a wrapper the wire can see:\n{types}"
        );
    }
}

/// The adopter's type is reached through a definition of this crate's own, and
/// what the document said about the value has to arrive there with it. Only the
/// *shape* is replaced — a shape the adopter owns the Rust for, so nothing is
/// lost — and the vendor's prose is not shape. It reads the same at a named
/// schema, where it becomes the wrapper's doc comment, and at a property, where
/// it becomes the field's.
#[test]
fn the_prose_a_schema_carries_survives_the_substitution() {
    let dir = out("owned-prose");
    Settings::new(wrote(&dir, "document.yaml", EITHER_NAME))
        .replace("cents", "money::Cents")
        .replace("amount", "money::Cents")
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    assert!(
        types.contains("///An amount in whole cents.\n#[derive"),
        "the named schema's sentence did not reach the wrapper it became:\n{types}"
    );
    assert!(
        types.contains("///What it costs.\n    pub figure: money::Cents,"),
        "the property's sentence did not reach the field:\n{types}"
    );
}

/// A schema named like the definition this crate declares a replaced type
/// under. The vendor spells it with dashes; Rust does not, and that is the
/// whole of the collision.
const RESERVED: &str = r#"
openapi: 3.0.3
info: { title: Reserved, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /prices:
    get:
      operationId: listPrices
      responses:
        "200": { description: OK }
components:
  schemas:
    typed-openapi-replaced0:
      type: object
      properties:
        figure: { type: string }
    Cents:
      type: string
      format: money
"#;

/// The type the adopter owns is reached through a definition of this crate's
/// own, so a schema whose Rust name is that definition's is a schema that would
/// quietly become their type — everywhere it is used, with nothing left to
/// carry what it declares. Every other named schema generates a type of its
/// own, so having none is the collision and nothing else, and it is refused by
/// name rather than approximated.
#[test]
fn a_schema_named_like_a_replaced_type_is_refused_by_name() {
    let dir = out("reserved");
    let failure = Settings::new(wrote(&dir, "document.yaml", RESERVED))
        .replace("money", "money::Cents")
        .write_to(&dir)
        .expect_err("a schema cannot be the definition a replaced type is reached through");
    assert_eq!(
        failure.to_string(),
        "the schema `typed-openapi-replaced0` reduces to a Rust name this crate \
         declares a type `Settings::replace` substitutes under, so it generates \
         no type of its own; rename it in an Overlay",
        "the refusal has to name the schema to rename"
    );
}

/// Two schemas the vendor spells differently and Rust does not: a dash and an
/// underscore reduce to the same PascalCase word.
const COLLIDING: &str = r##"
openapi: 3.0.3
info: { title: Colliding, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /vouchers:
    post:
      operationId: addVoucher
      requestBody:
        content:
          application/json:
            schema: { $ref: "#/components/schemas/voucher-summary" }
      responses:
        "200": { description: OK }
components:
  schemas:
    voucher-summary:
      type: object
      properties:
        count: { type: integer }
    Voucher_Summary:
      type: object
      properties:
        total: { type: string }
"##;

/// typify writes a definition per schema and uniquifies nothing, so two names
/// that reduce to one are two `struct`s under the same name in the same file.
/// Renaming one here would be a generator choosing a public name nobody asked
/// for — the same reason two operations reducing to one `<group> <command>`
/// are refused rather than renamed.
#[test]
fn two_schemas_that_reduce_to_one_type_are_refused_by_name() {
    let dir = out("colliding");
    let failure = Settings::new(wrote(&dir, "document.yaml", COLLIDING))
        .write_to(&dir)
        .expect_err("one name cannot be two types");
    assert_eq!(
        failure.to_string(),
        "the schema `voucher-summary` and the schema `Voucher_Summary` are both \
         `VoucherSummary` in Rust; rename one of them in an Overlay",
        "the refusal has to name both schemas and the type they share"
    );
}

/// `source` with the layout taken out of it: no whitespace, and no comma left
/// hanging before a closing bracket.
///
/// A generated file is `rustfmt`'s to lay out, so where it broke a line is not
/// something an assertion should depend on — what a call names and what a
/// struct holds is. The trailing comma goes with the line break that caused
/// it: `rustfmt` writes one into a list it split across lines and leaves it out
/// of one it did not.
fn dense(source: &str) -> String {
    let packed: String = source.split_whitespace().collect();
    packed
        .replace(",>", ">")
        .replace(",)", ")")
        .replace(",]", "]")
}

/// One item of a generated file, from the line that opens it to its closing
/// brace at column zero.
fn item(source: &str, opens: &str) -> String {
    let lines: Vec<&str> = source
        .lines()
        .skip_while(|line| !line.starts_with(opens))
        .take_while(|line| *line != "}")
        .collect();
    assert!(!lines.is_empty(), "`{opens}` is not in:\n{source}");
    lines.join("\n")
}

/// One wrapper of a generated `ops.rs`, from the documentation above its
/// signature to the closing brace of its body.
fn wrapper(ops: &str, name: &str) -> String {
    let signature = format!("    pub fn {name}(");
    let lines: Vec<&str> = ops.lines().collect();
    let head = lines
        .iter()
        .position(|line| line.starts_with(&signature))
        .unwrap_or_else(|| panic!("`{name}` is not in:\n{ops}"));
    let from = lines[..head]
        .iter()
        .rposition(|line| !is_doc(line))
        .map_or(0, |apart| apart + 1);
    let to = lines[head..]
        .iter()
        .position(|line| *line == "    }")
        .unwrap_or_else(|| panic!("`{name}` has no closing brace in:\n{ops}"));
    lines[from..=head + to].join("\n")
}

/// The fixture's `createLedgerEntry` describes its request body where it uses
/// it rather than under a name in `components.schemas`, which is an ordinary
/// thing for a document to do and says nothing about how much the document
/// describes: the body states a `required` list, one property that is a `$ref`
/// to a schema carrying a `pattern`, one that declares a `format`, and one
/// that is another named schema.
///
/// A body type of `serde_json::Value` would drop every one of those, and an
/// adopter reading `fits::<serde_json::Value>` reads it as the document having
/// no opinion — so both roads to the request are pinned here: the wrapper a
/// Rust caller uses and the check a `--json-body` file goes through.
#[test]
fn a_request_body_stated_inline_is_a_type_rather_than_a_bag_of_json() {
    let dir = out("inline-body");
    layered().write_to(&dir).expect("the fixtures generate");
    let types = read(&dir.join("src/types.rs"));
    let ops = dense(&read(&dir.join("src/ops.rs")));

    assert!(
        types.contains("pub struct CreateLedgerEntryBody"),
        "the schema the operation states inline generated no type:\n{types}"
    );
    assert!(
        ops.contains("body:&crate::types::CreateLedgerEntryBody"),
        "the wrapper does not take the type the document describes"
    );
    assert!(
        ops.contains(r#"fits::<crate::types::CreateLedgerEntryBody>("createLedgerEntry",body)"#),
        "a `--json-body` file is not held to the schema the document states"
    );
    assert!(
        !ops.contains(r#"fits::<serde_json::Value>("createLedgerEntry""#),
        "the body check accepts anything at all"
    );
}

/// The fixture's `createContact` is the other JSON body: `address` is an
/// object, so the whole body goes through `--json-body` and no property gets a
/// flag.
///
/// That is the one body kind nothing else stands in front of. A flat body's
/// per-field flags each run the rules their own schema states before a request
/// is built, and a body with no flags runs none — so the type named here is the
/// whole of what a file is held to, and `cli/src/raw.rs` is the seam that asks.
/// An operation that answered for any JSON would sit in the arm the bodiless
/// and non-JSON operations share, which reads as a document with no opinion
/// about a body it in fact describes in full.
#[test]
fn a_body_that_goes_whole_is_held_to_the_type_it_describes() {
    let dir = out("whole-body");
    layered().write_to(&dir).expect("the fixtures generate");
    let types = read(&dir.join("src/types.rs"));
    let check = dense(&item(
        &read(&dir.join("src/ops.rs")),
        "    pub fn check_body(",
    ));

    assert!(
        item(&types, "pub struct Contact").contains("pub address: Address,"),
        "`Contact` no longer nests, so this is not the body kind under test:\n{types}"
    );
    assert!(
        check.contains(r#"fits::<crate::types::Contact>("createContact",body)"#),
        "a `--json-body` file for a body with no flags is held to nothing:\n{check}"
    );
    assert!(
        !check.contains(r#"fits::<serde_json::Value>("createContact""#),
        "the body check accepts anything at all"
    );
}

/// The claim the type exists is not the claim that matters. What matters is
/// that a rule the document states about a value is a rule the generated code
/// runs, and this is that chain, link by link: the body's `account` is the
/// newtype the named schema became, that newtype's `FromStr` carries the
/// document's own `pattern`, and deserialising one goes through that `FromStr`
/// rather than around it. A value the pattern forbids therefore does not
/// deserialise into the body type — which is what an adopter's body check is
/// for.
#[test]
fn a_rule_the_document_states_runs_on_a_body_stated_inline() {
    let dir = out("inline-rule");
    layered().write_to(&dir).expect("the fixtures generate");
    let types = read(&dir.join("src/types.rs"));
    let ops = dense(&read(&dir.join("src/ops.rs")));

    assert!(
        ops.contains("body:&crate::types::CreateLedgerEntryBody")
            && ops.contains(
                r#"fits::<crate::types::CreateLedgerEntryBody>("createLedgerEntry",body)"#
            ),
        "neither road to the request names the type the rules are on, so nothing \
         below this runs on a body that is actually sent"
    );
    assert!(
        item(&types, "pub struct CreateLedgerEntryBody").contains("pub account: LedgerAccount,"),
        "the body's field is not the type the `$ref` names:\n{types}"
    );
    assert!(
        item(&types, "impl ::std::str::FromStr for LedgerAccount").contains(r#"("^[0-9]{4}$")"#),
        "the newtype does not carry the document's own rule:\n{types}"
    );
    assert!(
        dense(&item(
            &types,
            "impl<'de> ::serde::Deserialize<'de> for LedgerAccount"
        ))
        .contains("String::deserialize(deserializer)?.parse()"),
        "deserialising the newtype does not go through the rule:\n{types}"
    );
}

/// Which properties a body may leave out is part of what the document says
/// about it, and it reaches the type the same way the rest does.
#[test]
fn a_required_list_on_a_body_stated_inline_reaches_the_generated_type() {
    let dir = out("inline-required");
    layered().write_to(&dir).expect("the fixtures generate");
    let types = read(&dir.join("src/types.rs"));
    let ops = dense(&read(&dir.join("src/ops.rs")));
    let body = item(&types, "pub struct CreateLedgerEntryBody");

    assert!(
        ops.contains("body:&crate::types::CreateLedgerEntryBody"),
        "the wrapper does not take the type the list is on"
    );

    assert!(
        body.contains("pub account: LedgerAccount,"),
        "a property the document requires is optional:\n{body}"
    );
    assert!(
        body.contains("pub memo: ::std::option::Option<Memo>,"),
        "a property the document does not require is not:\n{body}"
    );
}

/// An answer is described the same way a request is, so it is read the same
/// way: the fixture's response is an inline object and its list of vouchers is
/// an inline array, and both come back as types rather than as values to pick
/// apart by hand.
#[test]
fn a_response_stated_inline_is_a_type_too() {
    let dir = out("inline-response");
    layered().write_to(&dir).expect("the fixtures generate");
    let types = read(&dir.join("src/types.rs"));
    let ops = dense(&read(&dir.join("src/ops.rs")));

    assert!(
        types.contains("pub struct CreateLedgerEntryResponse"),
        "the response the operation states inline generated no type:\n{types}"
    );
    assert!(
        ops.contains(
            "::std::result::Result<Call<'_,crate::types::CreateLedgerEntryResponse>,Error>"
        ),
        "the wrapper does not deserialise into it"
    );
    assert!(
        ops.contains(
            "::std::result::Result<Call<'_,::std::vec::Vec<crate::types::Voucher>>,Error>"
        ),
        "a list stated inline lost the type of what is in it"
    );
}

/// A document whose answer is a list of a shape it states in the same breath.
const NESTED: &str = r##"
openapi: 3.0.3
info: { title: Nested, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /positions:
    get:
      operationId: listPositions
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema:
                type: array
                items:
                  type: object
                  required: [sku]
                  properties:
                    sku: { $ref: "#/components/schemas/Sku" }
components:
  schemas:
    Sku:
      type: string
      pattern: "^[A-Z]{3}-[0-9]{4}$"
"##;

/// The item of a list is a schema like any other, and nothing about being
/// nested makes the rules on it less real — this is the shape a command line
/// has no per-field flag for, so the type is the whole of what holds a caller
/// to the document.
#[test]
fn the_item_of_a_list_stated_inline_is_a_type() {
    let dir = out("nested");
    Settings::new(wrote(&dir, "document.yaml", NESTED))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));
    let ops = dense(&read(&dir.join("src/ops.rs")));

    assert!(
        ops.contains(
            "::std::result::Result<Call<'_,::std::vec::Vec<crate::types::ListPositionsResponseItem>>,Error>"
        ),
        "the item of the list is not a type:\n{ops}"
    );
    assert!(
        item(&types, "pub struct ListPositionsResponseItem").contains("pub sku: Sku,"),
        "the rule on the item's property did not survive being nested:\n{types}"
    );
}

/// A body stated inline, reaching a type the adopter owns by both routes: one
/// property points at a named schema carrying the format, and the other spells
/// a format the document declares nowhere else.
const OWNED_INLINE: &str = r##"
openapi: 3.0.3
info: { title: Owned inline, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /ledger/entries:
    post:
      operationId: createLedgerEntry
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [amount]
              properties:
                amount: { $ref: "#/components/schemas/Cents" }
                stated: { type: string, format: cash }
      responses:
        "201": { description: Created }
components:
  schemas:
    Cents:
      type: string
      format: money
"##;

/// `Settings::replace` is the adopter's whole say over the generated types, and
/// it reaches a property of a body the document states inline exactly as it
/// reaches a property of a named schema — whether the property names the
/// format itself or points at a schema that does. This is the case the first
/// adoption lost: a monetary amount on a body nobody generated a type for is a
/// rule enforced nowhere.
///
/// The two properties are also the whole rule side by side: the one that points
/// at a named schema gets the newtype that schema became, and the one that
/// states the format gets the adopter's type itself. Being named is what
/// decides it, and nothing else does.
#[test]
fn a_type_the_adopter_owns_reaches_a_body_stated_inline() {
    let dir = out("owned-inline");
    Settings::new(wrote(&dir, "document.yaml", OWNED_INLINE))
        .replace("money", "cents::Cents")
        .replace("cash", "cash::Cash")
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));
    let ops = dense(&read(&dir.join("src/ops.rs")));
    let body = item(&types, "pub struct CreateLedgerEntryBody");

    assert!(
        ops.contains("body:&crate::types::CreateLedgerEntryBody"),
        "the wrapper does not take the type the adopter's own reaches through"
    );
    assert!(
        body.contains("pub amount: Cents,"),
        "a `$ref` to a schema the adopter owns is not the type it became:\n{types}"
    );
    assert!(
        body.contains("pub stated: ::std::option::Option<cash::Cash>,"),
        "a format the body declares itself, which the document declares nowhere \
         else, is not their type:\n{types}"
    );
    assert!(
        types.contains("pub struct Cents(pub cents::Cents);"),
        "the named schema is not a newtype over the type the adopter owns:\n{types}"
    );
    assert!(
        !types.contains("struct Cash"),
        "a format stated without a name was given a type of its own:\n{types}"
    );
}

/// Four bodies stated inline that are not objects: a list, a bare string, a
/// choice between two shapes, and a body the document states no schema for at
/// all.
const UNSHAPED: &str = r##"
openapi: 3.0.3
info: { title: Unshaped, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /vouchers/bulk:
    post:
      operationId: addVouchers
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: array
              items: { $ref: "#/components/schemas/Voucher" }
      responses:
        "201": { description: Created }
  /vouchers/note:
    post:
      operationId: noteVoucher
      requestBody:
        required: true
        content:
          application/json:
            schema: { type: string }
      responses:
        "201": { description: Created }
  /vouchers/either:
    post:
      operationId: eitherVoucher
      requestBody:
        required: true
        content:
          application/json:
            schema:
              oneOf:
                - { $ref: "#/components/schemas/Voucher" }
                - { type: string }
      responses:
        "201": { description: Created }
  /vouchers/anything:
    post:
      operationId: anythingVoucher
      requestBody:
        required: true
        content:
          application/json: {}
      responses:
        "201": { description: Created }
components:
  schemas:
    Voucher:
      type: object
      properties:
        id: { type: integer }
"##;

/// Nothing says a request body has to be an object, and a document that states
/// something else has still stated it — so each of these is the type the
/// document describes rather than a refusal. `serde_json::Value` is left for
/// the one body that earns it: the one the document states no schema for,
/// where the document really does have no opinion.
#[test]
fn a_body_stated_inline_that_is_not_an_object_is_still_what_the_document_says() {
    let dir = out("unshaped");
    Settings::new(wrote(&dir, "document.yaml", UNSHAPED))
        .write_to(&dir)
        .expect("a body that is not an object is a body");
    let ops = dense(&read(&dir.join("src/ops.rs")));

    for (what, spelling) in [
        ("a list", "body:&::std::vec::Vec<crate::types::Voucher>"),
        ("a bare string", "body:&::std::string::String"),
        (
            "a choice of shapes",
            "body:&crate::types::EitherVoucherBody",
        ),
        ("a body with no schema", "body:&serde_json::Value"),
    ] {
        assert!(
            ops.contains(spelling),
            "{what} stated inline is not `{spelling}`"
        );
    }
    assert!(
        read(&dir.join("src/types.rs")).contains("pub enum EitherVoucherBody"),
        "a choice between two shapes generated no type"
    );
}

/// A document whose inline body reduces to the name one of its own schemas
/// already has.
const CLAIMED: &str = r#"
openapi: 3.0.3
info: { title: Claimed, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /ledger/entries:
    post:
      operationId: createLedgerEntry
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                account: { type: string }
      responses:
        "201": { description: Created }
components:
  schemas:
    CreateLedgerEntryBody:
      type: object
      properties:
        total: { type: string }
"#;

/// A name derived from an operation can land on a name the document already
/// uses, and typify answers that by handing back the type it already has —
/// which would give the wrapper a body type describing a different shape,
/// silently. Two things under one name are refused by name here, the way two
/// schemas reducing to one type already are, because a generator picking a
/// public name nobody asked for is the worse answer.
#[test]
fn a_body_stated_inline_that_claims_a_schemas_name_is_refused_by_name() {
    let dir = out("claimed");
    let failure = Settings::new(wrote(&dir, "document.yaml", CLAIMED))
        .write_to(&dir)
        .expect_err("one name cannot be two types");

    assert!(
        matches!(&failure, GenerateError::OneType { inline: true, .. }),
        "{failure:?}"
    );
    assert_eq!(
        failure.to_string(),
        "the schema `CreateLedgerEntryBody` and the request body stated inline by \
         `createLedgerEntry` are both `CreateLedgerEntryBody` in Rust; give an inline \
         schema a `title` in an Overlay",
        "the refusal has to say which of the two is the operation's own, and the cheap repair"
    );
}

/// The repair the refusal names is the smaller one: a `title` on the inline
/// schema, where the other repair renames a schema the vendor wrote.
#[test]
fn a_title_on_the_inline_schema_mends_the_collision_the_refusal_names() {
    const TITLES_IT: &str = "overlay: 1.1.0\ninfo: { title: t, version: \"1\" }\nactions:\n  - \
         target: $.paths['/ledger/entries'].post.requestBody.content['application/json'].schema\n    \
         description: a name for the shape the operation states\n    update:\n      title: \
         LedgerEntry\n";
    let dir = out("claimed-then-titled");

    Settings::new(wrote(&dir, "document.yaml", CLAIMED))
        .overlay(wrote(&dir, "titles.overlay.yaml", TITLES_IT))
        .write_to(&dir)
        .expect("a title names the inline schema");

    let ops = dense(&read(&dir.join("src/ops.rs")));
    assert!(ops.contains("body:&crate::types::LedgerEntry"), "{ops}");
}

/// The same body, under a name the document gives it.
const TITLED: &str = r#"
openapi: 3.0.3
info: { title: Titled, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /ledger/entries:
    post:
      operationId: createLedgerEntry
      requestBody:
        required: true
        content:
          application/json:
            schema:
              title: LedgerEntry
              type: object
              properties:
                account: { type: string }
      responses:
        "201": { description: Created }
"#;

/// `title` is what OpenAPI offers for naming a shape, so a vendor who wrote one
/// has named the type and the name derived from the operation stands aside. It
/// is a fact of the document either way, which is what makes a bless run
/// reproduce it.
#[test]
fn a_body_stated_inline_takes_the_name_the_document_gives_it() {
    let dir = out("titled");
    Settings::new(wrote(&dir, "document.yaml", TITLED))
        .write_to(&dir)
        .expect("the document generates");
    let ops = dense(&read(&dir.join("src/ops.rs")));

    assert!(
        ops.contains("body:&crate::types::LedgerEntry"),
        "the name the document gave the shape was overruled:\n{ops}"
    );
    assert!(
        !ops.contains("CreateLedgerEntryBody"),
        "the derived name was used beside the one the document gave"
    );
}

/// A body of scalars is a body a command line can take apart, and generating a
/// type for it must not change that: the flags come off the reduced model,
/// which knows nothing about Rust types, and both roads still reach one request
/// builder.
#[test]
fn a_flat_body_stated_inline_still_grows_a_flag_per_field() {
    const CORRECTIONS: &str = include_str!("fixtures/corrections.yaml");
    const CLI: &str = include_str!("fixtures/cli.yaml");
    let document = read(Path::new(TOY));
    let doc = Document::load(&document, &[CORRECTIONS, CLI]).expect("the fixtures reduce");

    let typed_openapi::model::Body::JsonFields { fields, .. } = doc
        .get("createLedgerEntry")
        .expect("createLedgerEntry")
        .body()
    else {
        panic!("a body of scalars is taken apart into flags");
    };
    let named: Vec<&str> = fields.iter().map(typed_openapi::Field::name).collect();
    assert_eq!(named, ["account", "amount", "memo", "vatRate"]);
}

/// Three schemas that differ in what they say about further properties, and an
/// inline body that says `additionalProperties: true` beside its own, with a
/// list of objects that say it too.
const OPEN: &str = r#"
openapi: 3.0.3
info: { title: Open, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /profiles:
    post:
      operationId: createProfile
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [name]
              properties:
                name: { type: string }
                notes:
                  type: array
                  items:
                    type: object
                    properties:
                      tag: { type: string }
                    additionalProperties: true
              additionalProperties: true
      responses:
        "200": { description: OK }
components:
  schemas:
    Address:
      type: object
      properties:
        street: { type: string }
      additionalProperties: true
    Said:
      type: object
      properties:
        street: { type: string }
      additionalProperties: {}
    Unsaid:
      type: object
      properties:
        street: { type: string }
    Closed:
      type: object
      properties:
        street: { type: string }
      additionalProperties: false
"#;

/// The same document with every `true` spelled as the schema that admits
/// everything.
fn open_spelled_as_a_schema() -> String {
    OPEN.replace("additionalProperties: true", "additionalProperties: {}")
}

/// typify reads `additionalProperties: true` beside properties as "ignore the
/// rest" and writes no field, so a request body that carries a property the
/// schema leaves open would be an `Undeclared` one: the check finds nothing
/// that took it. It writes the `flatten`ed map for the schema `{}`, which says
/// the same thing, so the generator says it that way, at any depth.
#[test]
fn an_open_schema_beside_properties_gets_a_map_for_the_rest_at_any_depth() {
    let dir = out("open-true");
    Settings::new(wrote(&dir, "document.yaml", OPEN))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    let map = "    pub extra: ::serde_json::Map<::std::string::String, ::serde_json::Value>,";
    for opens in [
        "pub struct Address",
        "pub struct CreateProfileBody ",
        "pub struct CreateProfileBodyNotesItem",
    ] {
        let schema = item(&types, opens);
        assert!(
            schema.contains("    #[serde(flatten)]") && schema.contains(map),
            "`{opens}` has no map for the properties it leaves open:\n{types}"
        );
    }
}

/// `true` is rewritten to the schema `{}` and to nothing else: the two are one
/// document to the generator, and a schema that states nothing stays closed.
#[test]
fn an_open_schema_is_the_same_as_the_schema_that_admits_everything() {
    let dir = out("open-spelled-two-ways");
    Settings::new(wrote(&dir, "true.yaml", OPEN))
        .write_to(dir.join("true"))
        .expect("the document with `true` generates");
    Settings::new(wrote(&dir, "schema.yaml", &open_spelled_as_a_schema()))
        .write_to(dir.join("schema"))
        .expect("the document with `{}` generates");

    let spelled_true = read(&dir.join("true/src/types.rs"));
    let spelled_schema = read(&dir.join("schema/src/types.rs"));

    // The header names the command and the document the types came from.
    let without_header = |types: &str| -> String {
        types
            .lines()
            .filter(|line| !line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        without_header(&spelled_true),
        without_header(&spelled_schema)
    );
}

/// A schema that says nothing about further properties is closed as far as the
/// check goes, and `false` is closed by typify itself. Neither gets the map.
#[test]
fn a_schema_that_leaves_the_rest_unsaid_or_shut_gets_no_map() {
    let dir = out("open-shut");
    Settings::new(wrote(&dir, "document.yaml", OPEN))
        .write_to(&dir)
        .expect("the document generates");
    let types = read(&dir.join("src/types.rs"));

    let unsaid = item(&types, "pub struct Unsaid");
    let closed_by_typify = item(&types, "pub struct Closed");
    assert!(!unsaid.contains("extra"), "{unsaid}");
    assert!(!unsaid.contains("deny_unknown_fields"), "{unsaid}");
    assert!(!closed_by_typify.contains("extra"), "{closed_by_typify}");
    assert!(
        types.contains("#[serde(deny_unknown_fields)]\npub struct Closed"),
        "`false` no longer closes the type:\n{types}"
    );
}

// ----------------------------------------------- what a wrapper answers with

/// One operation, `getThing`, whose `responses` are `RESPONSES`, beside the
/// responses and the schemas a `$ref` can lead to. The first three operations of
/// the probe `A4/docs/refs.yaml` behind F-19 are this document with a different
/// slot.
const ANSWERS: &str = r##"
openapi: 3.0.3
info: { title: Answers, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things/{id}:
    get:
      operationId: getThing
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses: RESPONSES
components:
  responses:
    ThingResponse:
      description: OK
      content:
        application/json:
          schema: { $ref: "#/components/schemas/Thing" }
    Empty: { description: Nothing }
    Two:
      description: OK
      content:
        application/json: { schema: { $ref: "#/components/schemas/Thing" } }
        application/hal+json: { schema: { $ref: "#/components/schemas/Other" } }
    Same: { $ref: "#/components/responses/ThingResponse" }
    Loop: { $ref: "#/components/responses/Again" }
    Again: { $ref: "#/components/responses/Loop" }
  schemas:
    Thing: { type: object, properties: { a: { type: string } } }
    Other: { type: object, properties: { b: { type: string } } }
"##;

/// What the bless step writes for [`ANSWERS`] with `responses` in the slot: the
/// `ops.rs` of the document, or the refusal.
fn answering(name: &str, responses: &str) -> Result<String, GenerateError> {
    let dir = out(name);
    let document = wrote(
        &dir,
        "answers.yaml",
        &ANSWERS.replace("RESPONSES", responses),
    );
    Settings::new(document).write_to(&dir)?;
    Ok(read(&dir.join("src/ops.rs")))
}

/// Whether the wrapper of `getThing` answers with `ty`, spelled as a generated
/// file spells it.
fn answers_with(ops: &str, ty: &str) -> bool {
    dense(ops).contains(&dense(&format!(
        "-> ::std::result::Result<Call<'_, {ty}>, Error>"
    )))
}

/// The probe's `getThing`: a success response that is a `$ref` to
/// `#/components/responses/` has a body, and the wrapper returns its type. A
/// wrapper that took it for a response with no content would return `NoContent`
/// and `send` would throw the body away without a word.
#[test]
fn a_success_response_that_is_a_reference_is_typed() {
    let ops = answering(
        "answer-ref",
        r##"{ "200": { $ref: "#/components/responses/ThingResponse" } }"##,
    )
    .expect("the document generates");
    assert!(answers_with(&ops, "crate::types::Thing"), "{ops}");
}

/// A reference to a response that is itself a reference is followed to the end,
/// as the reduction follows a chain of `requestBodies`.
#[test]
fn a_chain_of_response_references_is_followed() {
    let ops = answering(
        "answer-chain",
        r##"{ "200": { $ref: "#/components/responses/Same" } }"##,
    )
    .expect("the document generates");
    assert!(answers_with(&ops, "crate::types::Thing"), "{ops}");
}

/// The probe's `listThings`: `2XX` is a success status, and the answer is the
/// array its schema states.
#[test]
fn a_success_response_under_a_range_is_typed() {
    let ops = answering(
        "answer-range",
        r##"{ "2XX": { description: OK, content: { application/json: {
            schema: { type: array, items: { $ref: "#/components/schemas/Thing" } } } } } }"##,
    )
    .expect("the document generates");
    assert!(
        answers_with(&ops, "::std::vec::Vec<crate::types::Thing>"),
        "{ops}"
    );
}

/// The probe's `latestThing` and its neighbours. A success response with no
/// `content` is an answer with no body, and it stands beside one that has a JSON
/// body, so the call answers `Option` of the type: `None` is the `204`. The order
/// of the responses decides nothing, and neither does where the empty response
/// is written.
#[test]
fn a_success_response_with_no_content_beside_a_json_one_makes_the_answer_an_option() {
    const THING: &str = "crate::types::Thing";
    for (label, responses, ty) in [
        (
            "a 204 listed before the 200",
            r##"{ "204": { description: Nothing yet }, "200": { description: OK, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } } } } }"##,
            "::std::option::Option<crate::types::Thing>",
        ),
        (
            "a 204 listed after the 200",
            r##"{ "200": { description: OK, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } } } },
              "204": { description: Nothing yet } }"##,
            "::std::option::Option<crate::types::Thing>",
        ),
        (
            "a range with a JSON body beside a 204",
            r##"{ "2XX": { description: OK, content: { application/json: {
                schema: { type: array, items: { $ref: "#/components/schemas/Thing" } } } } },
              "204": { description: Nothing yet } }"##,
            "::std::option::Option<::std::vec::Vec<crate::types::Thing>>",
        ),
        (
            "a response component with no content beside a JSON 200",
            r##"{ "200": { $ref: "#/components/responses/ThingResponse" },
              "202": { $ref: "#/components/responses/Empty" } }"##,
            "::std::option::Option<crate::types::Thing>",
        ),
        (
            "a JSON body with no schema",
            r#"{ "200": { description: OK, content: { application/json: {} } },
              "204": { description: Nothing yet } }"#,
            "::std::option::Option<serde_json::Value>",
        ),
        (
            "a body that is not JSON beside both",
            r##"{ "200": { $ref: "#/components/responses/ThingResponse" },
              "201": { description: Report, content: { application/pdf: {
                schema: { type: string, format: binary } } } },
              "204": { description: Nothing yet } }"##,
            "::std::option::Option<crate::types::Thing>",
        ),
        (
            "a JSON body beside a body that is not JSON",
            r##"{ "200": { $ref: "#/components/responses/ThingResponse" },
              "201": { description: Report, content: { application/pdf: {
                schema: { type: string, format: binary } } } } }"##,
            THING,
        ),
    ] {
        let ops = answering("answer-option", responses).expect(label);
        assert!(answers_with(&ops, ty), "{label}:\n{ops}");
    }
}

/// sevDesk's two operations with two success responses name one schema twice,
/// and so does a response that is a reference beside one that is stated: the
/// wrapper has one answer when the responses agree on it.
#[test]
fn success_responses_that_name_one_type_answer_with_it() {
    const THING: &str = "crate::types::Thing";
    for (label, responses, ty) in [
        (
            "the same schema twice",
            r##"{ "200": { description: OK, content: { application/json: {
                schema: { $ref: "#/components/schemas/Thing" } } } },
              "201": { description: Created, content: { application/json: {
                schema: { $ref: "#/components/schemas/Thing" } } } } }"##,
            THING,
        ),
        (
            "a response component and a stated response",
            r##"{ "200": { $ref: "#/components/responses/ThingResponse" },
              "201": { description: Created, content: { application/json: {
                schema: { $ref: "#/components/schemas/Thing" } } } } }"##,
            THING,
        ),
        (
            "an explicit code and the range around it",
            r##"{ "200": { $ref: "#/components/responses/ThingResponse" },
              "2XX": { $ref: "#/components/responses/Same" } }"##,
            THING,
        ),
        (
            "the same schema stated inline twice",
            r#"{ "200": { description: OK, content: { application/json: {
                schema: { type: object, properties: { c: { type: string } } } } } },
              "201": { description: Created, content: { application/json: {
                schema: { type: object, properties: { c: { type: string } } } } } } }"#,
            "crate::types::GetThingResponse",
        ),
        (
            "the same schema under two JSON media types of one response",
            r##"{ "200": { description: OK, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } },
                "application/vnd.x+json": { schema: { $ref: "#/components/schemas/Thing" } } } } }"##,
            THING,
        ),
    ] {
        let ops = answering("answer-agree", responses).expect(label);
        assert!(answers_with(&ops, ty), "{label}:\n{ops}");
    }
}

/// The words of a refusal that names its operation: the `Operation` variant
/// around an `Unsupported` one, with the reason it carries.
fn reason_of(error: &GenerateError, op: &str) -> String {
    let GenerateError::Operation { op: named, source } = error else {
        panic!("not the refusal of an operation: {error:?}");
    };
    assert_eq!(named, op, "{error:?}");
    let GenerateError::Unsupported(reason) = &**source else {
        panic!("not an unsupported element: {source:?}");
    };
    reason.clone()
}

/// A label, success responses whose JSON bodies disagree, and the words of the
/// refusal that names each body, in the order it names them.
const DISAGREEING: [(&str, &str, &[&str]); 7] = [
    (
        "two named schemas, in two responses",
        r##"{ "200": { $ref: "#/components/responses/ThingResponse" },
              "201": { description: Created, content: { application/json: {
                schema: { $ref: "#/components/schemas/Other" } } } } }"##,
        &[
            "`200` under `application/json` in `#/components/responses/ThingResponse` is \
             `#/components/schemas/Thing`",
            "`201` under `application/json` is `#/components/schemas/Other`",
        ],
    ),
    (
        "a named schema and one stated inline, in a response and a range",
        r##"{ "200": { $ref: "#/components/responses/ThingResponse" },
              "2XX": { description: OK, content: { application/json: {
                schema: { type: object, properties: { b: { type: string } } } } } } }"##,
        &[
            "`200` under `application/json` in `#/components/responses/ThingResponse` is \
             `#/components/schemas/Thing`",
            "`2XX` under `application/json` is a schema stated inline",
        ],
    ),
    (
        "a JSON body with no schema and a named schema",
        r##"{ "200": { description: OK, content: { application/json: {} } },
              "201": { $ref: "#/components/responses/ThingResponse" } }"##,
        &[
            "`200` under `application/json` is a JSON body with no schema",
            "`201` under `application/json` in `#/components/responses/ThingResponse` is \
             `#/components/schemas/Thing`",
        ],
    ),
    (
        "two media types of one response",
        r##"{ "200": { description: OK, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } },
                "application/vnd.x+json": { schema: { $ref: "#/components/schemas/Other" } } } } }"##,
        &[
            "`200` under `application/json` is `#/components/schemas/Thing`",
            "`200` under `application/vnd.x+json` is `#/components/schemas/Other`",
        ],
    ),
    (
        "the third media type of one response",
        r##"{ "200": { description: OK, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } },
                "application/vnd.x+json": { schema: { $ref: "#/components/schemas/Thing" } },
                application/hal+json: { schema: { $ref: "#/components/schemas/Other" } } } } }"##,
        &[
            "`200` under `application/json` is `#/components/schemas/Thing`",
            "`200` under `application/hal+json` is `#/components/schemas/Other`",
        ],
    ),
    (
        "two media types of a response component",
        r##"{ "200": { $ref: "#/components/responses/Two" } }"##,
        &[
            "`200` under `application/json` in `#/components/responses/Two` is \
             `#/components/schemas/Thing`",
            "`200` under `application/hal+json` in `#/components/responses/Two` is \
             `#/components/schemas/Other`",
        ],
    ),
    (
        "the second media type of the first response, and a later response",
        r##"{ "200": { description: OK, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } },
                "application/vnd.x+json": { schema: { $ref: "#/components/schemas/Thing" } } } },
              "201": { description: Created, content: {
                application/hal+json: { schema: { $ref: "#/components/schemas/Other" } } } } }"##,
        &[
            "`200` under `application/json` is `#/components/schemas/Thing`",
            "`201` under `application/hal+json` is `#/components/schemas/Other`",
        ],
    ),
];

/// Two JSON bodies that state different schemas leave the wrapper with no one type
/// to return. Picking the first would be the generator choosing the answer for
/// the adopter, so the bless refuses and names the operation, where each body
/// stands (the status and the media type) and what each states. The three arms of
/// what a body states are each read: a named schema, one stated inline, and none.
///
/// The client sends no `Accept` header, so the server may answer with any JSON
/// media type the response lists: the rule holds inside one response as it does
/// across two.
#[test]
fn json_bodies_that_state_different_schemas_are_refused_by_name() {
    for (label, responses, words) in DISAGREEING {
        let error = answering("answer-two", responses).expect_err(label);
        let reason = reason_of(&error, "getThing");
        // The body that came first in the document is named first.
        let mut rest = reason.as_str();
        for word in words {
            let (_, after) = rest.split_once(word).unwrap_or_else(|| {
                panic!("{label}: no `{word}` after the ones before it in\n{reason}")
            });
            rest = after;
        }
        assert!(
            reason.contains("do not agree on one schema"),
            "{label}: {reason}"
        );
    }
}

/// A response that is a reference this crate cannot follow is the document
/// saying something the generator has no reading for. It is refused with the
/// reference, and it is never `NoContent`, which takes an empty body or any JSON
/// and drops it.
#[test]
fn a_success_response_that_cannot_be_followed_is_refused_by_name() {
    for (reference, words) in [
        ("#/components/responses/Nowhere", "does not resolve"),
        ("#/components/schemas/Thing", "does not resolve"),
        (
            "#/components/responses/ThingResponse/content",
            "points inside a component",
        ),
        ("#/components/responses/Loop", "is a reference cycle"),
    ] {
        let responses = format!(r#"{{ "200": {{ $ref: "{reference}" }} }}"#);
        let error = answering("answer-unfollowed", &responses).expect_err(reference);
        let reason = reason_of(&error, "getThing");
        assert!(
            reason.contains("`200`") && reason.contains(words),
            "{reference}: {reason}"
        );
    }
}

/// A reference that is followed but leads to a schema the document never
/// declared is refused by that schema, as it is for a request body.
#[test]
fn a_response_whose_schema_is_not_declared_is_refused_by_its_reference() {
    let error = answering(
        "answer-undeclared",
        r##"{ "200": { description: OK, content: { application/json: {
            schema: { $ref: "#/components/schemas/Nowhere" } } } } }"##,
    )
    .expect_err("the schema is not declared");
    assert!(
        matches!(&error, GenerateError::Operation { op, source }
            if op == "getThing"
                && matches!(&**source, GenerateError::NoType { schema } if schema == "Nowhere")),
        "{error:?}"
    );
}

/// The load refuses no `$ref` in a response, so a response whose schema is a
/// `$ref` outside `#/components/schemas/` reaches the generator, which refuses it
/// by that reference. The places where the load refuses one first are in
/// `the_load_refuses_a_reference_out_of_the_schemas_where_it_reads_the_schema`.
#[test]
fn a_response_schema_that_leads_out_of_the_schemas_is_refused_by_its_reference() {
    for reference in [
        "#/definitions/Thing",
        "#/components/responses/ThingResponse",
    ] {
        let responses = format!(
            r#"{{ "200": {{ description: OK, content: {{ application/json: {{
                schema: {{ $ref: "{reference}" }} }} }} }} }}"#
        );
        let error = answering("answer-outside", &responses).expect_err(reference);
        let reason = reason_of(&error, "getThing");
        assert!(
            reason.contains(&format!("`{reference}`")) && reason.contains("#/components/schemas/"),
            "{reference}: {reason}"
        );
    }
}

/// `NoContent` is the answer when no success response states a JSON body: there
/// is none to type. A body under a status that is not a success, or under
/// `default`, is not the answer of the call, and a body that is not JSON adds
/// nothing to the type: the typed call reads JSON only, so a PDF is an
/// `Error::Decode` whatever the wrapper returns.
#[test]
fn a_wrapper_answers_no_content_when_no_success_response_has_a_json_body() {
    for (label, responses) in [
        ("a 204", r#"{ "204": { description: Nothing } }"#),
        ("a 200 with no content", r#"{ "200": { description: OK } }"#),
        (
            "a success whose content is not JSON",
            r#"{ "200": { description: OK, content: { text/csv: {
                schema: { type: string } } } } }"#,
        ),
        (
            "a JSON body under a status that is not a success",
            r##"{ "200": { description: OK }, "404": { description: Gone, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } } } } }"##,
        ),
        (
            "a JSON body under a redirect",
            r##"{ "204": { description: Nothing }, "301": { description: Moved, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } } } } }"##,
        ),
        (
            "a JSON body under a range of errors",
            r##"{ "204": { description: Nothing }, "4XX": { description: Wrong, content: {
                application/json: { schema: { $ref: "#/components/schemas/Thing" } } } },
              "5XX": { description: Broken, content: { application/json: {
                schema: { $ref: "#/components/schemas/Other" } } } } }"##,
        ),
        (
            "a JSON body under `default`",
            r##"{ "204": { description: Nothing }, default: { description: Oops, content: {
                application/json: { schema: { $ref: "#/components/schemas/Other" } } } } }"##,
        ),
        (
            "a response component with no content",
            r##"{ "200": { $ref: "#/components/responses/Empty" } }"##,
        ),
    ] {
        let ops = answering("answer-nothing", responses).expect(label);
        assert!(answers_with(&ops, "NoContent"), "{label}:\n{ops}");
    }
}

/// A JSON response that states no schema is a body of any shape, as a request
/// body without one is. The answer is `serde_json::Value`: `NoContent` would
/// accept the body and drop it.
#[test]
fn a_json_response_that_states_no_schema_is_any_json() {
    let ops = answering(
        "answer-shapeless",
        r#"{ "200": { description: OK, content: { application/json: {} } } }"#,
    )
    .expect("the document generates");
    assert!(answers_with(&ops, "serde_json::Value"), "{ops}");
}

// ------------------------------------------ a request body behind a component

/// The probe's `A4/docs/reqbody.yaml` behind F-47: `createThing` takes its body
/// from `#/components/requestBodies/`. The reduction follows the reference, so
/// the command line has `--a` and `--json-body`, and the bless follows it too,
/// where a bless that did not would refuse the whole document.
///
/// `BODY` is the component, written as a flow mapping.
const BODIES: &str = r##"
openapi: 3.0.3
info: { title: ReqBody, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things:
    post:
      operationId: createThing
      requestBody: { $ref: "#/components/requestBodies/ThingBody" }
      responses:
        "201": { description: Created }
components:
  requestBodies:
    ThingBody: BODY
    Alias: { $ref: "#/components/requestBodies/ThingBody" }
  schemas:
    Thing: { type: object, properties: { a: { type: string } } }
"##;

/// The component the probe uses.
const THING_BODY: &str = r##"{ required: true, content: { application/json: {
    schema: { $ref: "#/components/schemas/Thing" } } } }"##;

/// What the bless step writes for [`BODIES`] with `body` as the component, as
/// the text of `ops.rs` and `types.rs`.
fn bodied(name: &str, body: &str) -> (String, String) {
    let dir = out(name);
    let document = wrote(&dir, "bodies.yaml", &BODIES.replace("BODY", body));
    Settings::new(document)
        .write_to(&dir)
        .expect("the document generates");
    (
        read(&dir.join("src/ops.rs")),
        read(&dir.join("src/types.rs")),
    )
}

/// The wrapper takes the type of the schema the component names, and the check
/// that holds a `--json-body` file to a type is that type's.
#[test]
fn a_request_body_that_is_a_reference_to_a_component_is_typed() {
    let (ops, _) = bodied("body-ref", THING_BODY);
    assert!(ops.contains("body: &crate::types::Thing"), "{ops}");
    assert!(
        ops.contains("fits::<crate::types::Thing>(\"createThing\", body)"),
        "the check does not hold the file to the type:\n{ops}"
    );
}

/// A chain of references is followed to the end, as the reduction follows it.
#[test]
fn a_request_body_behind_a_chain_of_references_is_typed() {
    let dir = out("body-chain");
    let document = BODIES.replace("BODY", THING_BODY).replace(
        "{ $ref: \"#/components/requestBodies/ThingBody\" }\n      responses",
        "{ $ref: \"#/components/requestBodies/Alias\" }\n      responses",
    );
    Settings::new(wrote(&dir, "bodies.yaml", &document))
        .write_to(&dir)
        .expect("the document generates");
    let ops = read(&dir.join("src/ops.rs"));
    assert!(ops.contains("body: &crate::types::Thing"), "{ops}");
}

/// Whether the body is required is the component's, as it is on the command
/// line: a component that does not say so is an optional body.
#[test]
fn an_optional_body_behind_a_reference_is_an_option_argument() {
    let (ops, _) = bodied(
        "body-optional",
        r##"{ content: { application/json: { schema: { $ref: "#/components/schemas/Thing" } } } }"##,
    );
    assert!(
        ops.contains("body: ::std::option::Option<&crate::types::Thing>"),
        "{ops}"
    );
}

/// A schema the component states inline is a type of its own, named after the
/// operation that takes the body, as a body stated on the operation is.
#[test]
fn a_schema_stated_inline_in_a_component_body_is_a_type_of_the_operation() {
    let (ops, types) = bodied(
        "body-inline",
        r"{ required: true, content: { application/json: {
            schema: { type: object, properties: { c: { type: string } } } } } }",
    );
    assert!(types.contains("pub struct CreateThingBody"), "{types}");
    assert!(
        ops.contains("body: &crate::types::CreateThingBody"),
        "{ops}"
    );
}

/// A JSON body the component states no schema for is any JSON, as it is on the
/// operation.
#[test]
fn a_body_behind_a_reference_that_states_no_schema_is_any_json() {
    let (ops, _) = bodied(
        "body-shapeless",
        r"{ required: true, content: { application/json: {} } }",
    );
    assert!(ops.contains("body: &serde_json::Value"), "{ops}");
}

/// Two operations that take the body of one component, and answer with the
/// response of another, each stating its schema inline under `TITLE`.
const SHARED: &str = r##"
openapi: 3.0.3
info: { title: Shared, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /a:
    post:
      operationId: makeA
      requestBody: { $ref: "#/components/requestBodies/Draft" }
      responses: { "200": { $ref: "#/components/responses/Made" } }
  /b:
    post:
      operationId: makeB
      requestBody: { $ref: "#/components/requestBodies/Draft" }
      responses: { "200": { $ref: "#/components/responses/Made" } }
components:
  requestBodies:
    Draft:
      required: true
      content:
        application/json:
          schema: { TITLE type: object, properties: { c: { type: string } } }
  responses:
    Made:
      description: OK
      content:
        application/json:
          schema: { TITLE type: object, properties: { d: { type: string } } }
"##;

/// A component that two operations share is one schema. Stated inline and
/// untitled, it is a type for each operation, named after the operation, as two
/// operations that state the same schema on their own are. Under a `title` it
/// is one type that both take: typify hands the two the same definition, so the
/// bless writes it once and does not refuse the two for sharing a name.
#[test]
fn an_inline_schema_in_a_component_that_two_operations_share_is_a_type_for_each_or_one_by_its_title()
 {
    let dir = out("shared-component");
    let untitled = wrote(&dir, "untitled.yaml", &SHARED.replace("TITLE", ""));
    Settings::new(untitled)
        .write_to(dir.join("untitled"))
        .expect("the untitled document generates");
    let types = read(&dir.join("untitled/src/types.rs"));
    for expected in ["pub struct MakeABody", "pub struct MakeBBody"] {
        assert!(types.contains(expected), "no `{expected}`:\n{types}");
    }

    // One title for the body and another for the answer: the two are separate
    // schemas, and a title that two of them share is a collision of its own.
    let titled = SHARED
        .replacen("TITLE", "title: Draft,", 1)
        .replacen("TITLE", "title: Made,", 1);
    Settings::new(wrote(&dir, "titled.yaml", &titled))
        .write_to(dir.join("titled"))
        .expect("a component shared by two operations is one type");
    let types = read(&dir.join("titled/src/types.rs"));
    let ops = read(&dir.join("titled/src/ops.rs"));
    assert_eq!(types.matches("pub struct Draft ").count(), 1, "{types}");
    assert_eq!(types.matches("pub struct Made ").count(), 1, "{types}");
    assert_eq!(
        ops.matches("body: &crate::types::Draft").count(),
        4,
        "{ops}"
    );
    assert_eq!(
        ops.matches("-> ::std::result::Result<Call<'_, crate::types::Made>, Error>")
            .count(),
        4,
        "{ops}"
    );
}

/// Sharing is one schema stated once. Two operations that state different shapes
/// under one `title` are two definitions of one name, and typify answers the
/// second with the type it holds for the first, so the second wrapper would take
/// a body of the wrong shape. They are refused.
#[test]
fn two_different_schemas_under_one_title_are_still_refused() {
    let dir = out("shared-title");
    let document = wrote(
        &dir,
        "document.yaml",
        r##"
openapi: 3.0.3
info: { title: Titles, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /a:
    post:
      operationId: makeA
      requestBody: { $ref: "#/components/requestBodies/A" }
      responses: { "200": { description: OK } }
  /b:
    post:
      operationId: makeB
      requestBody: { $ref: "#/components/requestBodies/B" }
      responses: { "200": { description: OK } }
components:
  requestBodies:
    A: { content: { application/json: { schema: { title: Draft, type: object, properties: { c: { type: string } } } } } }
    B: { content: { application/json: { schema: { title: Draft, type: object, properties: { d: { type: string } } } } } }
"##,
    );
    let error = Settings::new(document)
        .write_to(&dir)
        .expect_err("one name cannot be two shapes");
    assert!(
        matches!(&error, GenerateError::OneType { inline: true, rust, .. } if rust == "Draft"),
        "{error:?}"
    );
    // The schema stands in the component, and the repair is made there: the
    // refusal names the component and the operation that takes it.
    assert_eq!(
        error.to_string(),
        "the request body `#/components/requestBodies/A` that `makeA` takes and the \
         request body `#/components/requestBodies/B` that `makeB` takes are both \
         `Draft` in Rust; give an inline schema a `title` in an Overlay"
    );
}

/// A response that a component states is named by the component too, and a chain
/// of references is named by the entry that holds the schema, which is where the
/// Overlay goes.
#[test]
fn a_response_component_that_claims_a_name_is_named_by_the_component() {
    let dir = out("shared-response-title");
    let document = wrote(
        &dir,
        "document.yaml",
        r##"
openapi: 3.0.3
info: { title: Titles, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /a:
    get:
      operationId: getA
      responses: { "200": { $ref: "#/components/responses/AliasOfA" } }
  /b:
    get:
      operationId: getB
      responses: { "200": { $ref: "#/components/responses/B" } }
components:
  responses:
    AliasOfA: { $ref: "#/components/responses/A" }
    A: { description: OK, content: { application/json: { schema: { title: Draft, type: object, properties: { c: { type: string } } } } } }
    B: { description: OK, content: { application/json: { schema: { title: Draft, type: object, properties: { d: { type: string } } } } } }
"##,
    );
    let error = Settings::new(document)
        .write_to(&dir)
        .expect_err("one name cannot be two shapes");
    assert!(
        matches!(&error, GenerateError::OneType { inline: true, rust, .. } if rust == "Draft"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "the response `#/components/responses/A` that `getA` answers with and the \
         response `#/components/responses/B` that `getB` answers with are both \
         `Draft` in Rust; give an inline schema a `title` in an Overlay"
    );
}

/// Two operations that each state a body and a response on the operation itself,
/// in the four slots `BODY_A`, `ANSWER_A`, `BODY_B` and `ANSWER_B`.
const STATED_TWICE: &str = r#"
openapi: 3.0.3
info: { title: Stated, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /a:
    post:
      operationId: makeA
      requestBody:
        required: true
        content: { application/json: { schema: BODY_A } }
      responses:
        "200": { description: OK, content: { application/json: { schema: ANSWER_A } } }
  /b:
    post:
      operationId: makeB
      requestBody:
        required: true
        content: { application/json: { schema: BODY_B } }
      responses:
        "200": { description: OK, content: { application/json: { schema: ANSWER_B } } }
"#;

/// A schema stated inline under a `title`.
const DRAFT: &str = "{ title: Draft, type: object, properties: { c: { type: string } } }";

/// What the bless step writes for [`STATED_TWICE`] with the four slots filled.
fn stating(name: &str, slots: [&str; 4]) -> Result<(String, String), GenerateError> {
    let dir = out(name);
    let [body_a, answer_a, body_b, answer_b] = slots;
    let document = STATED_TWICE
        .replace("BODY_A", body_a)
        .replace("ANSWER_A", answer_a)
        .replace("BODY_B", body_b)
        .replace("ANSWER_B", answer_b);
    Settings::new(wrote(&dir, "stated.yaml", &document)).write_to(&dir)?;
    Ok((
        read(&dir.join("src/ops.rs")),
        read(&dir.join("src/types.rs")),
    ))
}

/// A name that typify holds is answered with the type it holds, so the claim on
/// a Rust name passes when the second claimant states the schema of the first,
/// wherever either is stated: on two operations, in a body and a response, or
/// in a component two operations share. It is one schema, so one type is
/// correct, and the `struct` is written once.
#[test]
fn a_titled_schema_that_is_stated_twice_alike_is_one_type() {
    let other = "{ type: object, properties: { z: { type: string } } }";
    let sets_id = "{ title: Draft, type: object, properties: { id: { type: integer, \
                   readOnly: true } } }";
    for (label, slots) in [
        (
            "a body and a response that state a read-only property they do not require",
            [sets_id, sets_id, other, other],
        ),
        (
            "the same response on two operations",
            [
                other,
                DRAFT,
                "{ type: object, properties: { y: { type: string } } }",
                DRAFT,
            ],
        ),
        (
            "the same body on two operations",
            [
                DRAFT,
                other,
                DRAFT,
                "{ type: object, properties: { y: { type: string } } }",
            ],
        ),
        (
            "the body and the response of one operation",
            [
                DRAFT,
                DRAFT,
                other,
                "{ type: object, properties: { y: { type: string } } }",
            ],
        ),
    ] {
        let (ops, types) = stating("stated-alike", slots).expect(label);
        assert_eq!(
            types.matches("pub struct Draft ").count(),
            1,
            "{label}:\n{types}"
        );
        assert!(ops.contains("crate::types::Draft"), "{label}:\n{ops}");
    }
}

/// A schema that differs, even in a word of its own, is a second shape under the
/// name: the second wrapper would take a type of the wrong shape. The claim is
/// held to the schema typify is given, and the type of a body does not require
/// what the API sets. So one schema whose `required` names a `readOnly` property
/// is two schemas where a body and a response both state it, and they are
/// refused. Each refusal names where the two stand, the response of an operation
/// among them.
#[test]
fn a_titled_schema_that_is_stated_twice_differently_is_refused() {
    let other = "{ type: object, properties: { z: { type: string } } }";
    let required_id = "{ title: Draft, type: object, required: [id], properties: { id: { \
                       type: integer, readOnly: true } } }";
    for (label, slots, sentence) in [
        (
            "another property on a response",
            [
                other,
                DRAFT,
                other,
                "{ title: Draft, type: object, properties: { d: { type: string } } }",
            ],
            "the response stated inline by `makeA` and the response stated inline by \
             `makeB` are both `Draft` in Rust",
        ),
        (
            "a schema that requires a property the API sets, as a body and as a response",
            [required_id, required_id, other, other],
            "the request body stated inline by `makeA` and the response stated inline by \
             `makeA` are both `Draft` in Rust",
        ),
    ] {
        let error = stating("stated-unlike", slots).expect_err(label);
        assert!(
            matches!(&error, GenerateError::OneType { inline: true, rust, .. } if rust == "Draft"),
            "{label}: {error:?}"
        );
        assert!(error.to_string().starts_with(sentence), "{label}: {error}");
    }
}

/// A document with one operation, `paths`, and a component `Thing`: the
/// reference `#/definitions/Thing` leads out of `#/components/schemas/` to a
/// schema that would resolve if it were followed.
fn outside(name: &str, paths: &str) -> Result<(String, String), GenerateError> {
    let dir = out(name);
    let document = wrote(
        &dir,
        "outside.yaml",
        &format!(
            "openapi: 3.0.3\ninfo: {{ title: Outside, version: \"1.0\" }}\n\
             servers: [{{ url: \"http://localhost:9999\" }}]\n{paths}\
             components:\n  schemas:\n    Thing: {{ type: object }}\n"
        ),
    );
    Settings::new(document).write_to(&dir)?;
    Ok((
        read(&dir.join("src/ops.rs")),
        read(&dir.join("src/types.rs")),
    ))
}

/// The paths of [`outside`] for a read with `parameter` and no response body.
fn reading(parameter: &str) -> String {
    format!(
        "paths:\n  /a:\n    get:\n      operationId: getA\n      parameters: [{parameter}]\n      \
         responses: {{ \"200\": {{ description: OK }} }}\n"
    )
}

/// The paths of [`outside`] for a write whose request body has `content`.
fn writing(content: &str) -> String {
    format!(
        "paths:\n  /a:\n    post:\n      operationId: makeA\n      requestBody:\n        content: \
         {content}\n      responses: {{ \"200\": {{ description: OK }} }}\n"
    )
}

/// Where the load reads a schema, a `$ref` outside `#/components/schemas/` is
/// refused before the generator meets it: the schema of a parameter, of the items
/// of a list, of the JSON body that is sent and of a multipart body, and a
/// property of a flat body.
#[test]
fn the_load_refuses_a_reference_out_of_the_schemas_where_it_reads_the_schema() {
    let out_of_the_schemas = "$ref: '#/definitions/Thing'";
    for (label, paths) in [
        (
            "a parameter",
            reading(&format!(
                "{{ name: p, in: query, schema: {{ {out_of_the_schemas} }} }}"
            )),
        ),
        (
            "the items of a list",
            reading(&format!(
                "{{ name: p, in: query, schema: {{ type: array, items: {{ {out_of_the_schemas} }} }} }}"
            )),
        ),
        (
            "the JSON body that is sent",
            writing(&format!(
                "{{ application/json: {{ schema: {{ {out_of_the_schemas} }} }} }}"
            )),
        ),
        (
            "a multipart body",
            writing(&format!(
                "{{ multipart/form-data: {{ schema: {{ {out_of_the_schemas} }} }} }}"
            )),
        ),
        (
            "a property of a flat body",
            writing(&format!(
                "{{ application/json: {{ schema: {{ type: object, properties: {{ t: {{ \
                 {out_of_the_schemas} }} }} }} }} }}"
            )),
        ),
    ] {
        let error = outside("outside-refused", &paths).expect_err(label);
        assert!(
            matches!(
                &error,
                GenerateError::Unusable(LoadError::Unresolvable { .. })
            ),
            "{label}: {error:?}"
        );
    }
}

/// Where the load reads no schema, the reference goes on to typify: a body that
/// is sent as bytes and a body under a key that is not sent are not read, and a
/// property of a response's schema is typify's own. typify reads a reference by
/// its last segment, so it types the component of that name.
#[test]
fn a_reference_out_of_the_schemas_where_the_load_reads_no_schema_is_typifys() {
    let out_of_the_schemas = "$ref: '#/definitions/Thing'";
    for (label, paths, argument) in [
        (
            "a body sent as bytes",
            writing(&format!(
                "{{ application/octet-stream: {{ schema: {{ {out_of_the_schemas} }} }} }}"
            )),
            "body: ::std::option::Option<::std::vec::Vec<u8>>",
        ),
        (
            "a body under a key that is not sent",
            writing(&format!(
                "{{ application/json: {{ schema: {{ type: object, properties: {{ a: {{ type: \
                 string }} }} }} }}, application/xml: {{ schema: {{ {out_of_the_schemas} }} }} }}"
            )),
            "body: ::std::option::Option<&crate::types::MakeABody>",
        ),
    ] {
        let (ops, _) = outside("outside-blessed", &paths).expect(label);
        assert!(ops.contains(argument), "{label}:\n{ops}");
    }

    let nested = format!(
        "paths:\n  /a:\n    get:\n      operationId: getA\n      responses:\n        \"200\": {{ \
         description: OK, content: {{ application/json: {{ schema: {{ type: object, properties: \
         {{ t: {{ {out_of_the_schemas} }} }} }} }} }} }}\n"
    );
    let (_, types) = outside("outside-nested", &nested).expect("typify reads it");
    assert!(
        types.contains("pub t: ::std::option::Option<Thing>"),
        "the component named by the last segment:\n{types}"
    );
}

// ------------------------------------------- a refused bless writes nothing

/// The vendor's document as the first bless saw it: one operation, one type.
///
/// The revision that [`REVISION`] adds is refused when its `201` states `Thing`,
/// for a reason that is the generator's own: `getOther` has two success responses
/// that state two schemas, and the wrapper cannot answer with both. The refusal
/// comes from the step that writes `ops.rs`, after the step that writes
/// `types.rs`, so a bless that wrote each file as it went would have replaced
/// `types.rs` by then. When its `201` states `Other`, as its `200` does, the
/// revision blesses.
const THINGS: &str = r##"
openapi: 3.0.3
info: { title: Things, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things/{id}:
    get:
      operationId: getThing
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Thing" }
  # PATHS
components:
  schemas:
    Thing: { type: object, properties: { a: { type: string } } }
    # SCHEMAS
"##;

/// What the vendor adds in the next revision: a schema, and an operation whose
/// `201` is `RESPONSE`.
const REVISION: &str = r##"
  /others/{id}:
    get:
      operationId: getOther
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Other" }
        "201":
          description: Created
          content:
            application/json:
              schema: { $ref: "#/components/schemas/RESPONSE" }
"##;

/// [`THINGS`] with [`REVISION`] in it, whose `201` states `response`: `Other` is
/// the schema the `200` states too, and `Thing` is another.
fn things(revised: bool, response: &str) -> String {
    if !revised {
        return THINGS.to_owned();
    }
    THINGS
        .replace("  # PATHS\n", &REVISION.replace("RESPONSE", response))
        .replace(
            "    # SCHEMAS",
            "    Other: { type: object, properties: { b: { type: string } } }",
        )
}

/// Every file under `dir`, by path, with what it holds.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).expect("a directory the bless made") {
            let path = entry.expect("an entry of that directory").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let held = std::fs::read(&path).expect("a file the bless made");
                found.insert(path, held);
            }
        }
    }
    found
}

/// The files that one of the two trees lacks or that hold other bytes in them.
fn differing(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| before.get(*path) != after.get(*path))
        .map(|path| file_name(path))
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .expect("a file has a name")
        .to_string_lossy()
        .into_owned()
}

/// A bless that is refused writes nothing: the artefacts of the last bless are
/// what they were, byte for byte, and no file is left beside them.
///
/// A vendor revision is the case that matters. The adoption has committed the
/// last bless, the vendor publishes a revision the generator cannot write
/// wrappers for, and the bless stops half-way. A tree that holds the new
/// `types.rs` beside the old `ops.rs` and the old blob is one that `cargo build`
/// takes, and the binary then describes an API that the code does not call.
///
/// The control is the same revision with its two answers agreeing: it blesses, and
/// it changes every file, so the refused one is unchanged for the refusal and not
/// because there was nothing to write.
#[test]
fn a_bless_the_document_refuses_leaves_the_committed_artefacts_as_they_were() {
    let dir = out("refused-bless");
    let vendor = out("refused-bless-vendor");
    let document = wrote(&vendor, "things.yaml", &things(false, ""));
    Settings::new(&document)
        .write_to(&dir)
        .expect("the first revision blesses");
    let committed = tree(&dir);
    assert_eq!(
        committed.len(),
        4,
        "the four artefacts: {:?}",
        committed.keys()
    );

    wrote(&vendor, "things.yaml", &things(true, "Thing"));
    let error = Settings::new(&document)
        .write_to(&dir)
        .expect_err("the answers of `getOther` disagree");
    assert!(
        matches!(&error, GenerateError::Operation { op, source }
            if op == "getOther" && matches!(&**source, GenerateError::Unsupported(_))),
        "{error:?}"
    );
    assert_eq!(
        differing(&committed, &tree(&dir)),
        Vec::<String>::new(),
        "a refused bless changed these"
    );

    wrote(&vendor, "things.yaml", &things(true, "Other"));
    Settings::new(&document)
        .write_to(&dir)
        .expect("the revision with one answer blesses");
    assert_eq!(
        differing(&committed, &tree(&dir)),
        [
            "things.overlaid.yaml",
            "model.postcard",
            "ops.rs",
            "types.rs"
        ],
        "the revision changes every artefact"
    );
}

/// A bless that the file system stops places nothing either. Every artefact is
/// written beside its destination before the first is put in place, so the last
/// one failing leaves the first four where they were, and removes what was
/// written beside them.
///
/// A file stands where the summary page's directory would be made, which is the
/// last artefact and the only one that fails.
#[test]
fn a_bless_the_file_system_stops_places_no_artefact() {
    let dir = out("unplaced-bless");
    let vendor = out("unplaced-bless-vendor");
    let document = wrote(&vendor, "things.yaml", &things(false, ""));
    Settings::new(&document)
        .write_to(&dir)
        .expect("the first revision blesses");
    std::fs::write(dir.join("notes"), "a file").expect("a file where a directory would go");
    let committed = tree(&dir);

    wrote(&vendor, "things.yaml", &things(true, "Other"));
    let error = Settings::new(&document)
        .summary_page("notes/page.md")
        .write_to(&dir)
        .expect_err("`notes` is no directory");
    assert!(
        matches!(&error, GenerateError::Write { path, .. } if path.ends_with("notes")),
        "{error:?}"
    );
    assert_eq!(
        differing(&committed, &tree(&dir)),
        Vec::<String>::new(),
        "a stopped bless placed these"
    );
}

/// A destination that is a directory is refused before any file is replaced,
/// where the three artefacts in front of it would have been placed already.
///
/// A directory stands where the blob goes, which is the fourth artefact.
#[test]
fn a_bless_whose_destination_is_a_directory_places_no_artefact() {
    let dir = out("directory-bless");
    let vendor = out("directory-bless-vendor");
    let document = wrote(&vendor, "things.yaml", &things(false, ""));
    Settings::new(&document)
        .write_to(&dir)
        .expect("the first revision blesses");
    let model = dir.join("src/model.postcard");
    std::fs::remove_file(&model).expect("the blob of the first bless");
    std::fs::create_dir(&model).expect("a directory where the blob goes");
    std::fs::write(model.join("kept"), "a file in it").expect("a file in that directory");
    let committed = tree(&dir);

    wrote(&vendor, "things.yaml", &things(true, "Other"));
    let error = Settings::new(&document)
        .write_to(&dir)
        .expect_err("a directory stands where the blob goes");
    assert!(
        matches!(&error, GenerateError::Write { path, .. } if path.ends_with("model.postcard")),
        "{error:?}"
    );
    assert_eq!(
        differing(&committed, &tree(&dir)),
        Vec::<String>::new(),
        "a bless that a directory stopped placed these"
    );
}

/// A bless that `rustfmt` refuses writes nothing: the Rust is formatted in
/// memory, so the step that can still fail after every emitter has passed comes
/// before the first file.
///
/// The refusal is the adoption's own: a `rustfmt.toml` that `rustfmt` cannot
/// parse, which is in the directory the files go in and which `rustfmt` reads
/// from where the file will stand.
#[test]
fn a_bless_that_rustfmt_refuses_places_no_artefact() {
    let dir = out("unformatted-bless");
    let vendor = out("unformatted-bless-vendor");
    let document = wrote(&vendor, "things.yaml", &things(false, ""));
    Settings::new(&document)
        .write_to(&dir)
        .expect("the first revision blesses");
    std::fs::write(dir.join("rustfmt.toml"), "max_width = \"wide\"\n")
        .expect("a configuration that rustfmt cannot read");
    let committed = tree(&dir);

    wrote(&vendor, "things.yaml", &things(true, "Other"));
    let error = Settings::new(&document)
        .write_to(&dir)
        .expect_err("rustfmt cannot read its configuration");
    assert!(
        matches!(&error, GenerateError::RustfmtFailed { path } if path.ends_with("types.rs")),
        "{error:?}"
    );
    assert_eq!(
        differing(&committed, &tree(&dir)),
        Vec::<String>::new(),
        "a bless that rustfmt refused changed these"
    );
}

// ----------------------------------------------- a nullable schema admits null

/// The vendor's own words for "or `null`": `nullable: true`, on every shape a
/// schema has. `note` is the input of the probe `A4/docs/shapes.yaml` behind
/// F-17, a property that is required and nullable, and the rest are the shapes
/// sevdesk's document states it on.
const NULLABLE: &str = r##"
openapi: 3.0.3
info: { title: Nullable, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /holders/{id}:
    put:
      operationId: putHolder
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      requestBody:
        required: true
        content:
          application/json:
            schema: { $ref: "#/components/schemas/Holder" }
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema:
                type: object
                nullable: true
                properties:
                  answer: { type: string }
components:
  schemas:
    Holder:
      type: object
      required:
        - note
        - count
        - flag
        - kind
        - address
        - owner
        - tags
        - when
        - money
        - anything
        - plain
        - denied
        - beside
        - nullable
      properties:
        note: { type: string, nullable: true, description: A required nullable string }
        count: { type: integer, nullable: true }
        flag: { type: boolean, nullable: true }
        kind: { type: string, enum: [a, b], nullable: true }
        address:
          type: object
          nullable: true
          properties:
            street: { type: string }
        owner: { allOf: [{ $ref: "#/components/schemas/Owner" }], nullable: true }
        tags:
          type: array
          nullable: true
          items: { type: string, nullable: true }
        when: { type: string, format: date-time, nullable: true }
        money: { type: string, format: money, nullable: true }
        anything: { nullable: true }
        plain: { type: string }
        denied: { type: string, nullable: false }
        beside: { $ref: "#/components/schemas/Owner", nullable: true }
        nullable: { type: boolean }
        maybe: { type: string, nullable: true }
        list: { type: array, nullable: true, items: { type: string } }
        table: { type: object, nullable: true, additionalProperties: { type: string } }
        ratio: { type: integer, nullable: true, default: 3 }
        plain_list: { type: array, items: { type: string } }
        plain_table: { type: object, additionalProperties: { type: string } }
        plain_ratio: { type: integer, default: 3 }
    Owner:
      type: object
      properties:
        name: { type: string }
    Maybe:
      type: string
      nullable: true
"##;

/// The types and the wrappers the bless step writes for [`NULLABLE`], in a
/// directory named for the test that reads them: a test is a process of its own
/// under nextest and a thread of a shared one under `cargo test`, and neither may
/// empty the directory another is reading.
fn nullable(name: &str) -> (String, String) {
    let dir = out(name);
    let document = wrote(&dir, "nullable.yaml", NULLABLE);
    Settings::new(document)
        .replace("money", "crate::Money")
        .write_to(&dir)
        .expect("the document blesses");
    (
        read(&dir.join("src/types.rs")),
        read(&dir.join("src/ops.rs")),
    )
}

/// The type a generated struct gives a field, as its tokens print with no space
/// and no trailing comma, which `rustfmt` writes in a list of arguments that runs
/// over several lines.
fn field_type(types: &str, name: &str, field: &str) -> String {
    let file: syn::File = syn::parse_str(types).expect("the generated types parse");
    let found = file.items.iter().find_map(|item| {
        if let syn::Item::Struct(found) = item
            && found.ident == name
        {
            Some(found)
        } else {
            None
        }
    });
    found
        .expect("the struct is generated")
        .fields
        .iter()
        .find(|candidate| candidate.ident.as_ref().is_some_and(|ident| ident == field))
        .expect("the field is generated")
        .ty
        .to_token_stream()
        .to_string()
        .replace(' ', "")
        .replace(",>", ">")
}

/// A field and the type the bless step gives it.
const NULLABLE_FIELDS: [(&str, &str); 21] = [
    ("note", "::std::option::Option<::std::string::String>"),
    ("count", "::std::option::Option<i64>"),
    ("flag", "::std::option::Option<bool>"),
    ("kind", "::std::option::Option<HolderKind>"),
    ("address", "::std::option::Option<HolderAddress>"),
    ("owner", "::std::option::Option<Owner>"),
    (
        "tags",
        "::std::option::Option<::std::vec::Vec<::std::option::Option<::std::string::String>>>",
    ),
    (
        "when",
        "::std::option::Option<::chrono::DateTime<::chrono::offset::Utc>>",
    ),
    ("money", "::std::option::Option<crate::Money>"),
    // Not required, so an `Option` already, and not an `Option` of one.
    ("maybe", "::std::option::Option<::std::string::String>"),
    // Not required. Without the word the same three are a `Vec`, a map and an
    // integer that takes its default, and with it each one is an `Option`.
    (
        "list",
        "::std::option::Option<::std::vec::Vec<::std::string::String>>",
    ),
    (
        "table",
        "::std::option::Option<::std::collections::HashMap<::std::string::String,::std::string::String>>",
    ),
    ("ratio", "::std::option::Option<i64>"),
    ("plain_list", "::std::vec::Vec<::std::string::String>"),
    (
        "plain_table",
        "::std::collections::HashMap<::std::string::String,::std::string::String>",
    ),
    ("plain_ratio", "i64"),
    // What admits `null` already stays what it was.
    ("anything", "::serde_json::Value"),
    // What says nothing about `null` is not an `Option`.
    ("plain", "::std::string::String"),
    ("denied", "::std::string::String"),
    // OpenAPI 3.0.3: any properties beside a `$ref` are ignored.
    ("beside", "Owner"),
    ("nullable", "bool"),
];

/// F-17. `nullable: true` is how a vendor says "or `null`", and typify reads it
/// as a keyword it does not know, so the generator says it in a spelling that
/// typify turns into an `Option`. Every shape that states it is an `Option` of
/// what the schema describes, and a required one stays required.
#[test]
fn a_nullable_schema_is_an_option_wherever_the_type_is_written() {
    let (types, _) = nullable("nullable-fields");
    for (field, said) in NULLABLE_FIELDS {
        assert_eq!(field_type(&types, "Holder", field), said, "`{field}`");
    }
}

/// A required, nullable property is a key that must be there, and may hold
/// `null`. typify writes the `Option` so that a missing key is still refused.
#[test]
fn a_required_nullable_property_is_still_a_key_that_must_be_there() {
    let (types, _) = nullable("nullable-required");
    assert!(
        types.contains(
            "#[serde(deserialize_with = \"::std::option::Option::deserialize\")]\n    pub note:"
        ),
        "{types}"
    );
    assert!(
        types.contains(
            "#[serde(skip_serializing_if = \"::std::option::Option::is_none\")]\n    pub maybe:"
        ),
        "{types}"
    );
}

/// A schema that a component names and that is nullable is a newtype over an
/// `Option`, which is the way typify spells a named type that is not a struct,
/// and every use of it takes `null`. A nullable response is an `Option` too.
#[test]
fn a_named_nullable_schema_and_a_nullable_answer_are_options() {
    let (types, ops) = nullable("nullable-named");
    assert!(
        types.contains("pub struct Maybe(pub ::std::option::Option<::std::string::String>);"),
        "{types}"
    );
    assert!(
        ops.contains("Call<'_, ::std::option::Option<crate::types::PutHolderResponse>>"),
        "{ops}"
    );
}

// -------------------------------------- what typify cannot build is refused by name

/// A document with one operation that touches no schema, and whatever schemas the
/// case puts under `components`, each on a line of its own indented by four
/// spaces.
fn with_schemas(schemas: &str) -> String {
    format!(
        "openapi: 3.0.3\ninfo: {{ title: Schemas, version: \"1.0\" }}\nservers: [{{ url: \
         \"http://localhost:9999\" }}]\npaths:\n  /things:\n    get:\n      operationId: \
         listThings\n      responses:\n        \"200\": {{ description: OK }}\ncomponents:\n  \
         schemas:\n{schemas}"
    )
}

/// What a refused bless leaves in the tree: nothing, because every step that can
/// refuse comes before the first file is written.
fn assert_nothing_written(dir: &Path, what: &str) {
    assert!(
        !dir.join("src").exists() && !dir.join("spec").exists(),
        "{what}: a refused bless wrote into the tree"
    );
}

/// typify reads the type of `Settings::replace` as a path and panics inside its
/// conversion when it is not one, with a message about its own source and none
/// about the adopter's call. The generator reads the same text as a path first, so
/// the refusal names the format and the type the adopter wrote, and comes before
/// any schema is converted.
#[test]
fn a_replacement_that_is_no_rust_path_is_refused_by_name_before_typify_runs() {
    for (label, rust) in [
        ("words", "not a path"),
        ("symbols", "cents::Cents!!"),
        ("trailing", "cents::"),
        ("reference", "&str"),
        ("empty", ""),
    ] {
        let dir = out(&format!("replace-bad-{label}"));
        let failure = Settings::new(wrote(&dir, "document.yaml", OWNED))
            .replace("money", rust)
            .write_to(&dir)
            .expect_err("the text is no Rust path");
        assert!(
            matches!(&failure, GenerateError::Unsupported(text)
            if text.starts_with(&format!(
                "`{rust}`, named for `format: money`, is not a Rust path"
            ))),
            "{label}: {failure:?}"
        );
        assert_nothing_written(&dir, label);
    }
}

/// A path that typify takes is still taken, whatever its shape: a name, a rooted
/// name, a name under `crate` and a name with arguments.
#[test]
fn a_replacement_that_is_a_rust_path_generates() {
    for (label, rust) in [
        ("plain", "cents::Cents"),
        ("rooted", "::cents::Cents"),
        ("crate", "crate::cents::Cents"),
        ("arguments", "std::vec::Vec<u8>"),
    ] {
        let dir = out(&format!("replace-good-{label}"));
        Settings::new(wrote(&dir, "document.yaml", OWNED))
            .replace("money", rust)
            .write_to(&dir)
            .unwrap_or_else(|error| panic!("{label}: `{rust}` is a path: {error:?}"));
    }
}

/// Enum values that differ only in case or in a separator, and two that are
/// symbols with no letter in them. typify turns each value into a variant name and
/// panics when two of them are one, after the reduction has accepted the document.
const ENUMS_THAT_COLLIDE: [(&str, &str, &str); 3] = [
    ("case", "[active, ACTIVE]", "ACTIVE"),
    ("separator", "[a-b, a_b]", "a_b"),
    ("symbols", "['+', '-']", "-"),
];

/// typify's panic is not a refusal, and it names no schema: the process ends with
/// exit 101 and a line of typify's own source. The generator catches it and says
/// which schema was being converted, whether the enum is the schema, a property of
/// it, or stated inline by an operation, and the cause is what typify said.
#[test]
fn enum_values_that_become_one_rust_name_are_refused_naming_the_schema() {
    for (label, values, second) in ENUMS_THAT_COLLIDE {
        for (place, schemas, said) in [
            (
                "named",
                format!("    Thing: {{ type: string, enum: {values} }}\n"),
                "the schema `Thing`",
            ),
            (
                "property",
                format!(
                    "    Thing: {{ type: object, properties: {{ status: {{ type: string, enum: \
                     {values} }} }} }}\n"
                ),
                "the schema `Thing`",
            ),
        ] {
            let dir = out(&format!("enum-{label}-{place}"));
            let failure = Settings::new(wrote(&dir, "document.yaml", &with_schemas(&schemas)))
                .write_to(&dir)
                .expect_err("two values are one variant");
            assert!(
                matches!(&failure, GenerateError::Panicked { schema, .. } if schema == said),
                "{label}, {place}: {failure:?}"
            );
            assert_eq!(
                failure.to_string(),
                format!("typify panicked while it built the Rust type of {said}"),
                "{label}, {place}"
            );
            assert!(
                reported(&failure).contains(second),
                "{label}, {place}: the cause does not say which values:\n{}",
                reported(&failure)
            );
            assert_nothing_written(&dir, &format!("{label}, {place}"));
        }
    }
}

/// An enum that an operation states inline is converted after the named schemas,
/// and the refusal names the operation's body and not a schema of the document.
#[test]
fn an_enum_stated_inline_that_typify_cannot_name_is_refused_naming_the_body() {
    const STATED: &str = r#"openapi: 3.0.3
info: { title: Stated, version: "1.0" }
servers: [{ url: "http://localhost:9999" }]
paths:
  /things:
    post:
      operationId: createThing
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                status: { type: string, enum: [active, ACTIVE] }
      responses:
        "201": { description: Created }
"#;
    let dir = out("enum-inline");
    let failure = Settings::new(wrote(&dir, "document.yaml", STATED))
        .write_to(&dir)
        .expect_err("two values are one variant");

    assert!(
        matches!(&failure, GenerateError::Panicked { schema, .. }
            if schema == "the request body stated inline by `createThing`"),
        "{failure:?}"
    );
    assert_nothing_written(&dir, "inline");
}

/// A `$ref` that leads nowhere, in a property and in an `allOf`, panics inside
/// typify too, at two different places of its source. A schema that no operation
/// reaches is where the reduction never meets it, so the bless is where it is
/// refused, and the refusal names the schema that holds the reference.
#[test]
fn any_other_panic_of_typify_is_a_refusal_naming_the_schema_too() {
    for (label, schema) in [
        (
            "property",
            "    Lonely: { type: object, properties: { x: { $ref: '#/components/schemas/Missing' } } }\n",
        ),
        (
            "allOf",
            "    Lonely: { allOf: [ { $ref: '#/components/schemas/Missing' }, { type: object, \
             properties: { y: { type: string } } } ] }\n",
        ),
    ] {
        let dir = out(&format!("panic-{label}"));
        let failure = Settings::new(wrote(&dir, "document.yaml", &with_schemas(schema)))
            .write_to(&dir)
            .expect_err("a reference to nowhere");
        assert!(
            matches!(&failure, GenerateError::Panicked { schema, .. }
                if schema == "the schema `Lonely`"),
            "{label}: {failure:?}"
        );
        assert_nothing_written(&dir, label);
    }
}

/// Every refusal of the cycle test names the schemas that make the cycle, in the
/// order `allOf` leads from one to the next, starting with the one that comes
/// first in the document. `Tail` leads into a cycle and is not part of it.
const CYCLES: [(&str, &str, &[&str]); 8] = [
    (
        "the pair of SEV-282's review",
        "    Left:  { allOf: [{ $ref: '#/components/schemas/Right' }, { properties: { id: { type: \
         integer, readOnly: true } } }] }\n    Right: { allOf: [{ $ref: '#/components/schemas/Left' \
         }, { properties: { name: { type: string } } }] }\n",
        &["Left", "Right"],
    ),
    (
        "a schema that lists itself beside another",
        "    Mirror: { allOf: [{ $ref: '#/components/schemas/Mirror' }, { properties: { a: { type: \
         string } } }] }\n",
        &["Mirror"],
    ),
    (
        "three schemas",
        "    A: { allOf: [{ $ref: '#/components/schemas/B' }, { properties: { x: { type: string } } \
         }] }\n    B: { allOf: [{ $ref: '#/components/schemas/C' }, { properties: { y: { type: \
         string } } }] }\n    C: { allOf: [{ $ref: '#/components/schemas/A' }, { properties: { z: \
         { type: string } } }] }\n",
        &["A", "B", "C"],
    ),
    (
        "a schema that leads into a cycle",
        "    Tail: { allOf: [{ $ref: '#/components/schemas/B' }, { properties: { x: { type: string } \
         } }] }\n    B: { allOf: [{ $ref: '#/components/schemas/C' }, { properties: { y: { type: \
         string } } }] }\n    C: { allOf: [{ $ref: '#/components/schemas/B' }, { properties: { z: \
         { type: string } } }] }\n",
        &["B", "C"],
    ),
    (
        "a reference inside a nested allOf",
        "    A: { allOf: [{ allOf: [{ $ref: '#/components/schemas/B' }, { properties: { x: { type: \
         string } } }] }, { properties: { y: { type: string } } }] }\n    B: { allOf: [{ $ref: \
         '#/components/schemas/A' }, { properties: { z: { type: string } } }] }\n",
        &["A", "B"],
    ),
    (
        "a reference inside a oneOf of an allOf",
        "    A: { allOf: [{ oneOf: [{ $ref: '#/components/schemas/B' }, { properties: { q: { type: \
         string } } }] }, { properties: { y: { type: string } } }] }\n    B: { allOf: [{ $ref: \
         '#/components/schemas/A' }, { properties: { z: { type: string } } }] }\n",
        &["A", "B"],
    ),
    (
        "a wrapper of one element on one side",
        "    A: { allOf: [{ $ref: '#/components/schemas/B' }] }\n    B: { allOf: [{ $ref: \
         '#/components/schemas/A' }, { properties: { z: { type: string } } }] }\n",
        &["A", "B"],
    ),
    (
        "a oneOf that leads back to an allOf",
        "    A: { oneOf: [{ $ref: '#/components/schemas/B' }, { properties: { q: { type: string } } \
         }] }\n    B: { allOf: [{ $ref: '#/components/schemas/A' }, { properties: { z: { type: \
         string } } }] }\n",
        &["A", "B"],
    ),
];

/// typify merges the members of an `allOf` by following each `$ref` to the schema
/// it names, and a cycle of references has no end: its source says so in a `TODO`
/// at the arm that follows one (`merge::try_merge_schema`), and the process dies
/// of a stack overflow, which no `catch_unwind` sees. So the cycle is found in the
/// document, before typify runs, and refused naming the schemas that make it.
#[test]
fn a_cycle_of_allof_among_named_schemas_is_refused_naming_the_schemas() {
    for (at, (label, schemas, cycle)) in CYCLES.iter().enumerate() {
        let dir = out(&format!("cycle-{at}"));
        let failure = Settings::new(wrote(&dir, "document.yaml", &with_schemas(schemas)))
            .write_to(&dir)
            .expect_err("a cycle of allOf has no finite type");
        assert!(
            matches!(&failure, GenerateError::CyclicAllOf { schemas } if schemas == cycle),
            "{label}: {failure:?}"
        );
        assert_nothing_written(&dir, label);
    }
}

/// The words of the refusal for one schema and for several, and the repair.
#[test]
fn the_refusal_of_a_cycle_of_allof_says_what_leads_to_what() {
    let refused = |name: &str, at: usize| {
        let schemas = CYCLES[at].1;
        let dir = out(name);
        Settings::new(wrote(&dir, "document.yaml", &with_schemas(schemas)))
            .write_to(&dir)
            .expect_err("a cycle")
            .to_string()
    };
    assert_eq!(
        refused("cycle-words-pair", 0),
        "the schemas `Left` and `Right` are made of each other through `allOf`; break the cycle \
         in an Overlay"
    );
    assert_eq!(
        refused("cycle-words-one", 1),
        "the schema `Mirror` is made of itself through `allOf`; break the cycle in an Overlay"
    );
    assert_eq!(
        refused("cycle-words-three", 2),
        "the schemas `A`, `B` and `C` are made of each other through `allOf`; break the cycle in \
         an Overlay"
    );
}

/// A reference that closes a loop is a recursive type where the loop does not run
/// through a merge, and typify writes it: a property that points back, a `oneOf`
/// that does, an array of the schema, and the idiom that puts a sentence beside a
/// reference (an `allOf` of the schema and a description). Wrappers of one element
/// that name each other are no merge either.
#[test]
fn a_loop_that_is_no_merge_still_generates() {
    for (label, schemas) in [
        (
            "a property that points back",
            "    A: { allOf: [{ $ref: '#/components/schemas/B' }, { properties: { x: { type: \
             string } } }] }\n    B: { type: object, properties: { y: { $ref: \
             '#/components/schemas/A' } } }\n",
        ),
        (
            "a oneOf that points back",
            "    A: { oneOf: [{ $ref: '#/components/schemas/B' }, { type: string }] }\n    B: { \
             oneOf: [{ $ref: '#/components/schemas/A' }, { type: integer }] }\n",
        ),
        (
            "a list of the schema",
            "    A: { allOf: [{ $ref: '#/components/schemas/B' }, { type: object, properties: { x: \
             { type: string } } }] }\n    B: { type: array, items: { allOf: [{ $ref: \
             '#/components/schemas/A' }, { type: object, properties: { z: { type: string } } \
             }] } }\n",
        ),
        (
            "a sentence beside the reference",
            "    Node: { type: object, properties: { parent: { allOf: [{ $ref: \
             '#/components/schemas/Node' }, { description: the parent }] }, name: { type: string \
             } } }\n",
        ),
        (
            "wrappers of one element",
            "    A: { allOf: [{ $ref: '#/components/schemas/B' }] }\n    B: { allOf: [{ $ref: \
             '#/components/schemas/A' }] }\n",
        ),
    ] {
        let dir = out(&format!("loop-{}", label.replace(' ', "-")));
        Settings::new(wrote(&dir, "document.yaml", &with_schemas(schemas)))
            .write_to(&dir)
            .unwrap_or_else(|error| panic!("{label}: typify converts it: {error:?}"));
    }
}
