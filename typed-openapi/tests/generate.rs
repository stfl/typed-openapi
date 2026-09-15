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

use std::fmt::Write as _;
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
        ops.contains("r#type: Option<&str>") && ops.contains("r#ref: Option<&str>"),
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
    assert_eq!(
        failure.to_string(),
        "2listVouchers: the operationId has no spelling as a Rust identifier",
        "the failure has to name the operation: a generated file is six thousand \
         lines and a `syn` position is not something an adopter can open"
    );
}

/// The same naming, for a parameter: what the adopter needs is the operation
/// and the parameter, not a position in a token stream.
#[test]
fn a_parameter_that_cannot_be_spelled_names_itself_and_its_operation() {
    let dir = out("unspellable-param");
    let document = KEYWORDED.replace("- name: type", "- name: \"2\"");
    let failure = Settings::new(wrote(&dir, "document.yaml", &document))
        .write_to(&dir)
        .expect_err("`2` has no spelling as a Rust identifier");
    assert_eq!(
        failure.to_string(),
        "listVouchers: parameter `2` has no spelling as a Rust identifier",
    );
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
/// that Markdown turns into code and rustdoc then compiles.
///
/// The hanging list marker is carried three times because no one position
/// stands for the others: under a paragraph line and under a blank line in a
/// property's description, and under a paragraph line in an operation's
/// summary. rustc strips a leading `*` out of some `/* */` comments and leaves
/// it in others, so what the gap between marker and content has to survive is
/// not the same in all three.
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
        aside:
          type: string
          description: |
            A description that mentions //! in passing.
"##;

/// Every doc comment in `source`, verbatim and in its own column, on items
/// that depend on nothing.
///
/// The column is the point. rustc reconstructs a `/* */` comment's value by
/// stripping the indentation its lines share, so where the lines sit is half of
/// what decides whether the vendor's second paragraph is prose or a code block.
/// Copying the bytes and their positions is what makes this a fair question to
/// put to the doctest runner; generating a crate it could compile directly
/// would mean handing it serde as well, and the answer would be the same.
fn doc_comments_alone(source: &str) -> String {
    let mut out = String::from("pub struct Docs {\n");
    let mut fields = 0;
    let mut block = false;
    let mut carrying = false;
    for line in source.lines() {
        let trimmed = line.trim_start();
        let is_doc = block || trimmed.starts_with("/**") || trimmed.starts_with("///");
        if is_doc {
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
/// allowance — a doctest is executed, not linted. So the comments are handed to
/// the doctest runner, which is the only thing that answers the question
/// properly.
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
        let path = wrote(&dir, alone, &comments);
        let ran = std::process::Command::new("rustdoc")
            .args(["--test", "--edition", "2024"])
            .arg(&path)
            .output()
            .expect("rustdoc is on PATH beside the rustfmt a bless step already needs");
        let said = String::from_utf8_lossy(&ran.stdout);
        let complained = String::from_utf8_lossy(&ran.stderr);
        assert!(
            said.contains("running 0 tests"),
            "rustdoc found something to run in {file}:\n{said}{complained}\n{comments}"
        );
    }
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
        types.contains("*   the content of that item."),
        "a hanging list item lost its marker, or kept the gap that opens a code \
         block inside it:\n{types}"
    );
    assert!(
        types.contains("1.   the content of that item as well."),
        "an ordered marker is a list marker too:\n{types}"
    );

    // The same rule reaching a wrapper's doc, where the vendor's prose is a
    // summary rather than a property's description. The two positions differ in
    // what rustc does with them afterwards, so neither stands in for the other.
    let ops = read(&dir.join("src/ops.rs"));
    assert!(
        ops.contains("*   A summary the vendor hung from a marker."),
        "a hanging list item in a summary was left as the vendor wrote it:\n{ops}"
    );
}

/// A named schema the adopter owns the type for, reached from a wrapper.
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
        "200": { description: OK }
components:
  schemas:
    Cents:
      type: string
      format: money
"##;

/// A schema `Settings::replace` substituted has no type in the generated
/// module, because typify defined none — the adopter's own path is the answer,
/// and it is good anywhere the generated crate compiles. A wrapper reaching for
/// `crate::types::Cents` would name a module the type was never in, and a rule
/// that derived the name instead of asking would have no way to know that.
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
    let ops = read(&dir.join("src/ops.rs"));

    assert!(
        ops.contains("over: Option<cents::Cents>"),
        "the wrapper does not take the type the adopter owns:\n{ops}"
    );
    assert!(
        !types.contains("struct Cents"),
        "typify defined a type the adopter owns:\n{types}"
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
    let said = failure.to_string();

    assert!(
        said.contains("`voucher-summary`")
            && said.contains("`Voucher_Summary`")
            && said.contains("`VoucherSummary`"),
        "the refusal does not name both schemas and the type they share: {said}"
    );
}
