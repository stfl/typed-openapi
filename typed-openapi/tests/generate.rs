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

use std::path::{Path, PathBuf};

use typed_openapi::Document;
use typed_openapi::generate::{GenerateError, Settings};

const TOY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/toy.yaml");
const CORRECTIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/corrections.yaml"
);
const CLI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/cli.yaml");

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

/// The four artefacts are one Overlay application seen four ways, so the
/// reduction a binary loads has to be the reduction of the document that was
/// committed beside it. Reducing the written YAML again is the only check that
/// they left the generator together.
#[test]
fn the_written_model_is_the_written_documents_reduction() {
    let dir = out("agree");
    let written = layered().write_to(&dir).expect("the fixtures generate");

    assert_eq!(
        written,
        vec![
            dir.join("spec/toy.overlaid.yaml"),
            dir.join("src/types.rs"),
            dir.join("src/ops.rs"),
            dir.join("src/model.postcard"),
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
        ops.contains("tag: Vec<&str>"),
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
        .write_to(&dir)
        .expect("the fixtures generate");

    for file in ["spec/toy.overlaid.yaml", "src/types.rs", "src/ops.rs"] {
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
    for (name, document) in [("awkward", AWKWARDLY_NAMED), ("patterned", PATTERNED)] {
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
