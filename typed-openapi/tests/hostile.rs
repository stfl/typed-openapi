//! What this crate makes of a document that was not written for it.
//!
//! `tests/fixtures/toy.yaml` is a worked example: written here, out of this
//! crate's own vocabulary, converging on the shapes the crate handles well. Its
//! consistency is invisible, so a reduction that leans on that consistency
//! passes against it. `tests/fixtures/hostile.yaml` is the control — every
//! shape in it is one a vendor writes by accident, in a vocabulary this
//! repository uses nowhere else, so a name that does not survive the trip from
//! the document to a flag and to Rust has nowhere to hide.
//!
//! # The split
//!
//! The fixture is the half that *loads*. Some of these shapes are ones this
//! crate refuses, and a document holding one cannot also demonstrate the shapes
//! that reduce — so each refusal is its own minimal document, written beside
//! the assertion that names it. A refusal that fires for the wrong reason
//! passes a careless test, which is why every one of them is asserted on the
//! whole message rather than on a substring.
//!
//! # Outcomes that are pinned rather than endorsed
//!
//! A shape that produces something this crate would not choose is pinned
//! exactly as it stands, with the test saying so, because a characterisation is
//! what makes a repair visible: the day one is fixed, the test naming it goes
//! red and whoever fixed it reads what the old behaviour was. Nothing here is
//! pinned that way at present — every assertion below says what this crate
//! means, not what it happens to do.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use typed_openapi::generate::{GenerateError, Settings};
use typed_openapi::model::Body;
use typed_openapi::scalar::ScalarError;
use typed_openapi::{
    Carrier, CommandName, Document, LoadError, Operation, Scalar, Shape, Unsupported,
};

/// The message of the cause under `error`. An error describes its own layer, so
/// the thing that could not be read is named one level down.
fn cause(error: &dyn Error) -> String {
    error
        .source()
        .expect("the layer has a cause under it")
        .to_string()
}

/// The control document, read the way an adopter reads it.
const HOSTILE: &str = include_str!("fixtures/hostile.yaml");

/// The same file by path, for the bless step, which reads documents itself.
const HOSTILE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hostile.yaml");

/// The Rust type an adopter of this document already owns for its cone numbers.
///
/// A path of the crate that the generated files are compiled in
/// ([`PYROMETRY`]), so the same bless serves the tests that read the text and the
/// one that builds it.
const OWNED: (&str, &str) = ("cone-number", "crate::pyrometry::Cone");

fn document() -> Document {
    Document::load(HOSTILE, &[]).expect("the control document reduces")
}

fn operation(doc: &Document, id: &str) -> Operation {
    doc.get(id)
        .unwrap_or_else(|| panic!("the document declares `{id}`"))
        .clone()
}

/// The flag one parameter grows, or `None` where it grows none.
fn flag_of(op: &Operation, name: &str) -> Option<String> {
    match op
        .param(name)
        .unwrap_or_else(|| panic!("`{}` declares `{name}`", op.id()))
        .shape()
    {
        Shape::Flag { flag, .. } => Some(flag.clone()),
        Shape::Unreachable(_) => None,
    }
}

/// What `--help` says about one subcommand, rendered the way a user sees it.
fn long_help(op: &Operation) -> String {
    typed_openapi::tree::command(op)
        .render_long_help()
        .to_string()
}

/// The same page with every run of whitespace collapsed to one space.
///
/// clap wraps a help page to the terminal, so a sentence long enough to be
/// worth checking is a sentence that arrives in two pieces. Collapsing first is
/// what lets a test assert on the sentence rather than on the width it happened
/// to be rendered at.
fn unwrapped(op: &Operation) -> String {
    long_help(op)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A directory of this test's own, emptied first so that nothing it asserts
/// about can be left over from a previous run.
fn out(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    drop(std::fs::remove_dir_all(&dir));
    dir
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("a file the bless step reported writing")
}

/// A document this test owns outright, written where the output goes.
///
/// A refusal wants the smallest document that produces it, beside the
/// assertion, where a reader sees both at once.
fn wrote(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(dir).expect("a directory to write the document into");
    std::fs::write(&path, contents).expect("a document this test writes");
    path
}

/// The control document through a bless step, with the adopter's type declared.
///
/// One run per test that reads a generated file, into a directory of its own,
/// so that a failure names the test it came from.
fn blessed(name: &str) -> PathBuf {
    let dir = out(name);
    Settings::new(HOSTILE_PATH)
        .replace(OWNED.0, OWNED.1)
        .write_to(&dir)
        .expect("the control document is hostile and legal: it blesses");
    dir
}

/// Every typed wrapper the generated file declares, in its order.
///
/// A wrapper is the `pub fn` that takes `&self`, which is what tells one from
/// the inventory's own `pub fn`s beside them.
fn wrappers(ops: &str) -> Vec<String> {
    ops.split("pub fn ")
        .skip(1)
        .filter_map(|after| after.split_once('('))
        .filter(|(_, args)| args.trim_start().starts_with("&self"))
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// The argument list of one generated wrapper, as the identifiers it binds.
///
/// Read out of the emitted text rather than off a token stream, because what
/// this is about is the file an adopter compiles.
fn arguments(ops: &str, wrapper: &str) -> Vec<String> {
    let opened = format!("pub fn {wrapper}(");
    let (_, after) = ops
        .split_once(&opened)
        .unwrap_or_else(|| panic!("the generated file declares `{wrapper}`"));
    let (signature, _) = after
        .split_once(") -> ::std::result::Result")
        .expect("every wrapper answers with a Result");
    signature
        .split(',')
        .filter_map(|argument| argument.split_once(':'))
        .map(|(name, _)| name.trim().to_owned())
        .collect()
}

// ---------------------------------------------------------------- names

/// `kilnLog`, `glaze_recipe` and `PYROCone` are three casings and three types,
/// and this crate spells none of them: typify does, and the generator reads its
/// answer rather than keeping a copy of the rule. `Cone` is beside them as the
/// plain case, so a change that only handled the awkward spellings would show.
///
/// This is the half of the casing story that is legal. The half that is not is
/// the test below.
#[test]
fn schema_names_in_three_casings_each_reduce_to_a_type_of_their_own() {
    let types = read(&blessed("hostile-casing").join("src/types.rs"));
    for declared in [
        "pub struct KilnLog {",
        "pub struct GlazeRecipe {",
        "pub struct PyroCone(",
        "pub struct Cone(",
    ] {
        assert!(
            types.contains(declared),
            "`{declared}` is not in the generated types:\n{types}"
        );
    }
}

/// A document declaring `kilnLog` and `kiln_log` is saying one Rust name twice,
/// and typify uniquifies nothing — so emitting both is a file that defines the
/// same struct twice. The refusal names both schemas and the name they collide
/// on, because an adopter has to know which two to go and look at.
#[test]
fn two_schema_names_that_reduce_to_one_type_are_refused_and_name_both() {
    const TWICE: &str = r##"
openapi: 3.0.3
info: { title: Twice, version: "1.0" }
servers: [{ url: "http://localhost:9411" }]
paths:
  /firings:
    get:
      operationId: listFirings
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema: { $ref: "#/components/schemas/kilnLog" }
components:
  schemas:
    kilnLog:
      type: object
      properties:
        ref: { type: string }
    kiln_log:
      type: object
      properties:
        cone: { type: string }
"##;

    let dir = out("hostile-one-type");
    let refused = Settings::new(wrote(&dir, "document.yaml", TWICE))
        .write_to(&dir)
        .expect_err("`kilnLog` and `kiln_log` are one Rust name");
    assert_eq!(
        refused.to_string(),
        "the schema `kilnLog` and the schema `kiln_log` are both `KilnLog` in Rust; \
         rename one of them in an Overlay"
    );
}

// ------------------------------------------------------------- keywords

/// A parameter named after a Rust word is a spelling problem in one place and
/// nothing at all in the other: the flag is the document's word kebab-cased,
/// and the wire name travels beside the argument as a literal, so what the
/// request carries does not depend on how Rust had to spell it.
///
/// `self`, `crate` and `Self` are the three that cannot be written raw. `Self`
/// kebab-cases onto `self`, which the subcommand has already spent, so it moves
/// aside — the same rule a body field follows. It snake-cases onto `self` as
/// well, and moves aside in the wrapper for the same reason in the other
/// alphabet; what it *sends* is `Self` either way, which is what these rows
/// read.
#[test]
fn parameters_named_after_rust_words_keep_the_documents_word_on_the_flag() {
    let doc = document();
    let op = operation(&doc, "listFirings");
    for word in ["type", "match", "ref", "move", "self", "crate"] {
        assert_eq!(
            flag_of(&op, word).as_deref(),
            Some(word),
            "`{word}` is a keyword in Rust and a flag name here"
        );
    }
    assert_eq!(
        flag_of(&op, "Self").as_deref(),
        Some("param-self"),
        "`Self` kebab-cases onto a flag `self` already spent"
    );

    let ops = read(&blessed("hostile-keywords").join("src/ops.rs"));
    for (word, spelled) in [
        ("type", "r#type"),
        ("match", "r#match"),
        ("ref", "r#ref"),
        ("move", "r#move"),
        ("self", "self_"),
        ("crate", "crate_"),
        ("Self", "self_2"),
    ] {
        assert!(
            ops.contains(&format!(".maybe(\"{word}\", {spelled})")),
            "`{word}` does not reach the request as `{spelled}`"
        );
    }
}

/// Two parameters whose names snake-case onto one word bind two arguments.
///
/// `self` and `Self` both want `self_`, and the `ref` in a path and the `Ref` in
/// a query both want `r#ref`. Two arguments under one identifier is `E0415` —
/// and `syn` parses such a signature without complaint, so a bless step would
/// report success over a crate that does not compile. The later claimant moves
/// aside, which is the answer the command line gives the same pair two tests up.
///
/// What each argument *sends* is the half that has to survive the move: the wire
/// name travels beside the argument as a literal, so `ref_2` still reaches the
/// request under `Ref`. Sending the first argument's value under the second's
/// wire name is the failure the collision threatened, and the builder chain is
/// where it would show.
#[test]
fn two_parameters_that_spell_one_identifier_bind_two_arguments() {
    let ops = read(&blessed("hostile-collision").join("src/ops.rs"));

    assert_eq!(
        arguments(&ops, "list_firings"),
        [
            "r#type",
            "r#match",
            "r#ref",
            "r#move",
            "self_",
            "crate_",
            "self_2",
            "kiln_state",
            "crazing",
            "saggar_tally",
            "help",
        ],
        "`Self` moves aside from the `self_` the document's `self` spent"
    );

    assert_eq!(
        arguments(&ops, "append_log_entry"),
        ["r#ref", "ref_2", "body"],
        "the query `Ref` moves aside from the path `ref`"
    );
    for pairing in [r#".param("ref", r#ref)"#, r#".maybe("Ref", ref_2)"#] {
        assert!(
            ops.contains(pairing),
            "`{pairing}` is not how the request is built:\n{ops}"
        );
    }
}

/// No wrapper in the generated file binds one identifier twice.
///
/// The assertion above names the two operations that would have; this one is the
/// statement without the names, so a third collision the fixture grows is caught
/// where it happens rather than wherever it first fails to compile.
#[test]
fn no_generated_wrapper_binds_one_identifier_twice() {
    let ops = read(&blessed("hostile-bound").join("src/ops.rs"));
    let named = wrappers(&ops);
    assert_eq!(
        named.len(),
        18,
        "two wrappers per operation — the plain one and the builder that \
         delegates to it — over nine operations: {named:?}"
    );
    for wrapper in named {
        let bound = arguments(&ops, &wrapper);
        let mut once = bound.clone();
        once.sort_unstable();
        once.dedup();
        assert_eq!(
            once.len(),
            bound.len(),
            "`{wrapper}` binds an identifier twice: {bound:?}"
        );
    }
}

/// An argument that moved aside says which wire name it carries.
///
/// `self_2` and `ref_2` are nowhere in the document, and under the `builder`
/// feature they are setters an adopter types — so a signature carrying one is a
/// name they cannot look up. The command line meets the same collision and
/// answers it, with `(sends `Ref`)` on the flag that moved; a wrapper that said
/// nothing would leave the two consumers disagreeing about whether a rename is
/// worth mentioning, which is the asymmetry the renaming rule exists to end.
///
/// The count is the other half: an argument that kept the plain spelling
/// accounts for itself, so a note beside one would be noise.
#[test]
fn a_wrapper_argument_that_moved_aside_says_what_it_sends() {
    let ops = read(&blessed("hostile-moved").join("src/ops.rs"));
    for said in [
        "`self_2` sends the document's `Self`, under a name of its own",
        "`ref_2` sends the document's `Ref`, under a name of its own",
        // Not moved aside: the document's word starts with a digit, so an `x` stands
        // in front of it, which is a spelling nobody wrote either.
        "`x3ds` sends the document's `3ds`, under a name of its own",
    ] {
        assert!(
            ops.contains(said),
            "the wrapper does not account for the argument:\n{ops}"
        );
    }
    assert_eq!(
        ops.matches("under a name of its own").count(),
        3,
        "two arguments move and one is spelled with an `x`, and nothing else is remarked on"
    );
}

/// The request body claims its name last, so a document that spends `body` on a
/// parameter keeps the parameter's name and moves the body — which then says
/// what it is, because a `body_2` beside a `body` is exactly the signature an
/// adopter would otherwise read as a generator's mistake.
#[test]
fn a_body_argument_the_parameters_crowded_out_says_what_it_is() {
    const CROWDED: &str = r#"
openapi: 3.0.3
info: { title: Crowded, version: "1.0" }
servers: [{ url: "http://localhost:9411" }]
paths:
  /firings:
    post:
      operationId: startFiring
      parameters:
        - name: body
          in: query
          schema: { type: string }
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                cone: { type: string }
      responses:
        "201": { description: Created }
"#;

    let dir = out("hostile-crowded");
    Settings::new(wrote(&dir, "document.yaml", CROWDED))
        .write_to(&dir)
        .expect("a parameter named `body` is legal and blesses");
    let ops = read(&dir.join("src/ops.rs"));

    assert_eq!(
        arguments(&ops, "start_firing"),
        ["body", "body_2"],
        "the document's own `body` keeps the name and the request body moves"
    );
    assert!(
        ops.contains("`body_2` is the request body, under a name of its own"),
        "the wrapper does not account for its body argument:\n{ops}"
    );
}

/// Two wire names that spell one flag are two flags, and the one that moved says
/// which wire name it carries — the only thing left to go by once the document
/// is gone.
#[test]
fn two_names_that_spell_one_flag_are_two_flags_and_the_second_says_so() {
    let doc = document();
    let op = operation(&doc, "appendLogEntry");
    let flags: Vec<Option<String>> = op
        .params()
        .iter()
        .map(|param| match param.shape() {
            Shape::Flag { flag, .. } => Some(flag.clone()),
            Shape::Unreachable(_) => None,
        })
        .collect();
    assert_eq!(
        flags,
        [Some("ref".to_owned()), Some("param-ref".to_owned())],
        "the path parameter claims the plain name and the query parameter moves aside"
    );
    assert!(
        long_help(&op).contains("(sends `Ref`)"),
        "the flag that moved has to name the wire name it carries:\n{}",
        long_help(&op)
    );
}

// ------------------------------------------------------------ the prose

/// A vendor's list survives into the generated page as a list, and the gap
/// between a marker and its content is capped at four columns rather than
/// three.
///
/// Markdown measures a list item's content from the end of its marker, so five
/// columns after one open an indented code block *inside* the item — which
/// rustdoc then compiles and `cargo test --doc` then fails on. Four is the
/// widest gap that opens nothing, and capping to three would narrow a nested
/// list for no reason. The vendor wrote five; the page carries four.
///
/// The line indentation is the other cap and is one narrower, because it is
/// measured from the margin: the vendor's four-space example block lands on
/// three and stops being a block.
#[test]
fn a_hanging_list_marker_keeps_its_item_at_a_gap_of_four_columns() {
    let types = read(&blessed("hostile-prose").join("src/types.rs"));

    // A line with text in it of a description of several lines is a `///` line
    // with one space after the slashes, which rustdoc takes off again.
    let doc_lines: Vec<&str> = types
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("/// "))
        .collect();
    assert!(
        doc_lines.contains(&"*    bisque, twenty minutes"),
        "the item lost its marker or its gap:\n{types}"
    );
    assert!(
        !doc_lines.iter().any(|line| line.starts_with("*   bisque")),
        "the gap is four columns, not three"
    );
    assert!(
        !doc_lines
            .iter()
            .any(|line| line.starts_with("*     bisque")),
        "five columns after a marker open a code block inside the item"
    );
    assert!(
        doc_lines.contains(&"   soak=90m"),
        "the vendor's four-space block has to land on three:\n{types}"
    );
    assert!(
        !doc_lines.contains(&"    soak=90m"),
        "four columns of indentation open a code block rustdoc compiles"
    );
}

/// The taming belongs to the generator, which is what puts the prose where
/// rustdoc runs it. Nothing touches the sentence on its way to a flag, so what
/// `--help` shows is the vendor's own bytes.
#[test]
fn the_flag_carries_the_vendors_sentence_exactly_as_written() {
    let doc = document();
    let op = operation(&doc, "startFiring");
    let Body::JsonFields { fields, .. } = op.body() else {
        panic!("`startFiring` states a flat body inline");
    };
    let soak = fields
        .iter()
        .find(|field| field.name() == "soak")
        .expect("the body declares `soak`");
    assert!(
        soak.description()
            .is_some_and(|text| text.contains("*     bisque, twenty minutes")),
        "the model carries the description untouched: {:?}",
        soak.description()
    );
    assert!(
        long_help(&op).contains("*     bisque, twenty minutes"),
        "and so does the flag's help"
    );
}

// ------------------------------------------------- bodies stated inline

/// A body an operation states where it uses it carries everything a named one
/// carries. The `pattern` behind a `$ref` reaches the flag, the sentence behind
/// another reaches its help, and the `format` on a bare property travels beside
/// the rules without becoming one.
#[test]
fn a_body_stated_inline_keeps_every_rule_its_refs_carry() {
    let doc = document();
    let op = operation(&doc, "startFiring");
    let Body::JsonFields { fields, .. } = op.body() else {
        panic!("every property of this body is a value, so it is flat");
    };
    let named: Vec<(&str, &str, bool, Option<&str>)> = fields
        .iter()
        .map(|field| (field.name(), field.flag(), field.required(), field.format()))
        .collect();
    assert_eq!(
        named,
        [
            ("cone", "cone", true, Some("cone-number")),
            ("soak", "soak", true, None),
            ("bare_cone", "bare-cone", false, Some("cone-number")),
            ("help", "body-help", false, None),
        ]
    );

    assert!(
        long_help(&op).contains("a match for ^0[1-9][0-9]?$"),
        "the `pattern` the `$ref` leads to is the rule the flag is held to:\n{}",
        long_help(&op)
    );
}

/// One nested property sends the whole body through a file — no sibling gets a
/// flag the request builder would then throw away — and the walk that decided
/// it writes down what it saw, as the skeleton `--json-body-template` prints.
///
/// The skeleton carries the required keys and nothing else, and the empty
/// string under `cone` is a value that schema's own `pattern` refuses: a
/// template nobody filled in is a body the server turns away rather than one it
/// acts on.
#[test]
fn one_nested_ref_sends_the_body_whole_and_leaves_a_skeleton_behind() {
    let doc = document();
    let op = operation(&doc, "appendLogEntry");
    let Body::JsonWhole {
        required, template, ..
    } = op.body()
    else {
        panic!("`entry` is an object, so the body goes out whole");
    };
    assert!(*required);
    assert_eq!(
        template.as_deref(),
        Some("{\n  \"entry\": {\n    \"cone\": \"\"\n  }\n}")
    );
}

/// A body that is a list at the top level is not an object, so it has no
/// per-field flags either — and its skeleton holds one element, because what
/// goes in the list is what a caller came to find out.
#[test]
fn a_body_that_is_a_list_goes_whole_and_its_skeleton_holds_one_element() {
    let doc = document();
    let body = operation(&doc, "tallyWadding").body().clone();
    let Body::JsonWhole {
        required, template, ..
    } = body
    else {
        panic!("a list is not an object");
    };
    assert!(required);
    assert_eq!(template.as_deref(), Some("[\n  \"\"\n]"));
}

/// An object that declares no properties is a flat body with nothing in it. The
/// subcommand grows no per-field flag and keeps `--json-body`, which is what
/// leaves the operation reachable: an object the document describes no keys of
/// is one a caller still has to be able to send.
#[test]
fn an_object_with_no_properties_is_a_flat_body_with_no_flags() {
    let doc = document();
    let op = operation(&doc, "fileGlazeRecipe");
    let Body::JsonFields { fields, .. } = op.body() else {
        panic!("an object is a flat body whether or not it declares anything");
    };
    assert_eq!(fields.as_slice(), []);
    let help = long_help(&op);
    assert!(help.contains("--json-body <FILE>"), "{help}");
}

/// A property named `*` kebab-cases to nothing, so there is no flag to give it
/// — and the body it is in goes out whole.
///
/// A flag is the one name in this crate a *user types*, and `names::spelled` is
/// the rule it passes, the same one a command name and a gate pass. A property
/// outside that rule has no flag, and the body's question is whether a property
/// has one and never why it has none: a name nothing can spell reaches the same
/// answer a nested schema reaches, which is `--json-body` for the lot.
///
/// `grog` loses a flag it could have carried, which is the price the nested rule
/// already pays. What it buys is that the key is written down: the skeleton
/// `--json-body-template` prints names `*`, where a flag called `""` names it
/// nowhere — clap renders one as `-- <STRING>`, the end-of-options marker, and
/// the only spelling that ever reached it was `--=VALUE`.
#[test]
fn a_property_whose_name_has_no_letters_sends_the_body_whole() {
    let doc = document();
    let op = operation(&doc, "packWadding");
    let Body::JsonWhole {
        required, template, ..
    } = op.body()
    else {
        panic!("`*` is a property with no flag, so the body goes out whole");
    };
    assert!(*required);
    assert_eq!(
        template.as_deref(),
        Some("{\n  \"*\": \"\",\n  \"grog\": \"\"\n}"),
        "the template is the one place the key nothing can spell is named"
    );

    let help = long_help(&op);
    assert!(help.contains("--json-body <FILE>"), "{help}");
    assert!(
        !help.contains("-- <STRING>"),
        "no flag stands where the end-of-options marker is written:\n{help}"
    );
    assert!(
        !help.contains("--grog"),
        "and no sibling keeps a flag beside a body that goes whole:\n{help}"
    );
}

/// A parameter named the same way is carried the way every other parameter this
/// crate cannot spell is: named on the subcommand's long help, given nothing,
/// and counted.
///
/// It is the shape with the least room around it. A body keeps `--json-body` as
/// a second door, where a parameter has none at all — so a *required* one is an
/// operation that could never build a correct request, which is the line the
/// four shapes beside it are already held to.
#[test]
fn a_parameter_whose_name_has_no_letters_is_carried_without_a_flag() {
    fn starred(required: bool) -> String {
        format!(
            r#"
openapi: 3.0.3
info: {{ title: Starred, version: "1.0" }}
servers: [{{ url: "http://localhost:9411" }}]
paths:
  /firings:
    get:
      operationId: listFirings
      parameters:
        - name: "*"
          in: query
          required: {required}
          schema: {{ type: string }}
      responses:
        "200": {{ description: OK }}
"#
        )
    }

    let doc = Document::load(&starred(false), &[]).expect("an optional one costs one value");
    let op = operation(&doc, "listFirings");
    assert_eq!(
        op.param("*").expect("declared").shape(),
        &Shape::Unreachable(Unsupported::Unspellable)
    );
    assert!(
        unwrapped(&op).contains(
            "`*` has no flag: it is named with a character outside ASCII, or in a way that does \
             not kebab-case into [a-z0-9-]."
        ),
        "the long help does not say why it has none:\n{}",
        unwrapped(&op)
    );
    assert_eq!(doc.summary().unreachable().len(), 1);

    let refused =
        Document::load(&starred(true), &[]).expect_err("a required one could never be supplied");
    assert_eq!(
        refused.to_string(),
        "listFirings: parameter `*` is named with a character outside ASCII, or in a way that \
         does not kebab-case into [a-z0-9-], and the document requires it; correct the \
         parameter in an Overlay, or drop its `required`"
    );
}

// ------------------------------------------------------- a type the adopter owns

/// A named schema carrying a replaced format is a newtype over the adopter's
/// type whether or not its name is what their path ends in. `Cone` against
/// `pyrometry::Cone` and `PYROCone` against the same type both wrap, because
/// the rewrite is keyed on the format and goes through a reference — so which
/// of the two an adopter gets does not turn on a coincidence between a name the
/// vendor chose and a name they chose.
///
/// A property that merely states the format without a name of its own is the
/// adopter's type standing alone, which is the other half of the same rule.
#[test]
fn a_replaced_format_wraps_whatever_the_schema_holding_it_is_called() {
    let types = read(&blessed("hostile-replaced").join("src/types.rs"));
    assert!(
        types.contains("pub struct Cone(pub crate::pyrometry::Cone);"),
        "the schema whose name the path ends in still wraps:\n{types}"
    );
    assert!(
        types.contains("pub struct PyroCone(pub crate::pyrometry::Cone);"),
        "and so does the one whose name it does not:\n{types}"
    );
    assert!(
        types.contains("pub bare_cone: ::std::option::Option<crate::pyrometry::Cone>"),
        "a property stating the format without a name of its own is the type itself:\n{types}"
    );
}

/// The rule the document states about a replaced value is still the rule the
/// command line enforces. The generated newtype hands its reading to the
/// adopter's `FromStr`; the flag is held to the `pattern` the document wrote,
/// and the two are the same bytes.
#[test]
fn a_replaced_value_keeps_the_documents_rule_on_the_command_line() {
    let doc = document();
    let op = operation(&doc, "startFiring");
    let Body::JsonFields { fields, .. } = op.body() else {
        panic!("`startFiring` states a flat body");
    };
    let cone = fields
        .iter()
        .find(|field| field.name() == "cone")
        .expect("the body declares `cone`");
    assert!(cone.scalar().parse("04").is_ok());
    assert!(
        cone.scalar().parse("4").is_err(),
        "the document's `pattern` wants a leading zero"
    );
}

// ------------------------------------------------ parameters with no flag

/// Every shape this crate cannot put on a flag costs the operation that
/// declares it and nothing more. All five are carried, named on the
/// subcommand's long help where a flag would have been, given no flag, and
/// counted by the summary — so an adopter is told what the document asks for
/// and what the request will go out without.
#[test]
fn every_parameter_this_crate_cannot_spell_is_carried_named_and_counted() {
    let doc = document();
    let op = operation(&doc, "withdrawFiring");
    let unspellable: Vec<(&str, &Unsupported)> = op
        .params()
        .iter()
        .filter_map(|param| match param.shape() {
            Shape::Unreachable(why) => Some((param.name(), why)),
            Shape::Flag { .. } => None,
        })
        .collect();
    assert_eq!(
        unspellable,
        [
            ("saggarSeal", &Unsupported::Cookie),
            ("rampProfile", &Unsupported::Encoded),
            ("soakWindow", &Unsupported::Structured),
            ("waddingMix", &Unsupported::Structured),
            (
                "crazingMarks",
                &Unsupported::Style("pipeDelimited".to_owned())
            ),
        ]
    );

    let help = unwrapped(&op);
    for (name, why) in &unspellable {
        assert!(
            help.contains(&format!("`{name}` has no flag: it is {why}.")),
            "the long help does not say why `{name}` has none:\n{help}"
        );
    }
    assert_eq!(
        doc.summary().unreachable().len(),
        5,
        "the count comes off the model rather than from beside it"
    );
    assert_eq!(
        flag_of(&op, "ref").as_deref(),
        Some("ref"),
        "the flags the operation can spell are untouched by the ones it cannot"
    );
}

/// The one unspellable shape that is worth refusing a document over: a
/// parameter a caller must supply and this CLI cannot. Every other one costs
/// the document nothing, so the difference has to be `required` and nothing
/// else.
#[test]
fn a_required_parameter_with_no_flag_is_refused_and_names_the_operation() {
    const DEMANDED: &str = r#"
openapi: 3.0.3
info: { title: Demanded, version: "1.0" }
servers: [{ url: "http://localhost:9411" }]
paths:
  /firings:
    get:
      operationId: listFirings
      parameters:
        - name: soakWindow
          in: query
          required: true
          schema:
            type: object
            properties:
              from: { type: string }
      responses:
        "200": { description: OK }
"#;

    let refused = Document::load(DEMANDED, &[]).expect_err("the operation could never be invoked");
    assert_eq!(
        refused.to_string(),
        "listFirings: parameter `soakWindow` is neither a value nor a list of values, \
         and the document requires it; correct the parameter in an Overlay, or drop \
         its `required`"
    );
    assert!(
        matches!(refused, LoadError::Parameter { .. }),
        "and it is the refusal about a parameter, not some other one: {refused:?}"
    );
}

// ------------------------------------------------------ what a value carries

/// An enumeration of one member is a flag whose only legal value is the one it
/// would have had anyway, and an enumeration holding the empty string is a flag
/// whose legal value is nothing at all. Both are values the document lists, so
/// both are values the flag takes.
#[test]
fn an_enumeration_of_one_and_an_enumeration_holding_nothing_are_both_offered() {
    let doc = document();
    let op = operation(&doc, "listFirings");
    let choices = |name: &str| match op.param(name).expect("declared").shape() {
        Shape::Flag {
            scalar: Scalar::Choice(values),
            ..
        } => values.clone(),
        other @ (Shape::Flag { .. } | Shape::Unreachable(_)) => {
            panic!("`{name}` is an enumeration, not {other:?}")
        }
    };
    assert_eq!(choices("kilnState"), ["bisque"]);
    assert_eq!(choices("crazing"), ["", "crazed"]);

    let matches = typed_openapi::tree::command(&op)
        .try_get_matches_from(["list", "--crazing", ""])
        .expect("the empty string is one of the values the document lists");
    let values = typed_openapi::tree::values(&op, &matches).expect("and it reads back");
    assert!(
        format!("{values:?}").contains(r#"("crazing", "")"#),
        "{values:?}"
    );
    assert!(
        typed_openapi::tree::command(&op)
            .try_get_matches_from(["list", "--kiln-state", "glaze"])
            .is_err(),
        "a value the enumeration does not list is refused"
    );
}

/// A `format` no specification defines is carried in the document's own
/// spelling and acted on nowhere. It reaches [`Operation::carrying`], which is
/// how an adopter asks which of the values it is about to send are of a kind,
/// and it reaches neither the rule the flag is held to nor the help line that
/// renders the rules.
#[test]
fn a_format_no_specification_names_travels_and_changes_nothing() {
    let doc = document();
    let op = operation(&doc, "listFirings");
    assert_eq!(
        op.param("saggarTally").expect("declared").format(),
        Some("sagger-count")
    );

    let of_a_kind: Vec<&str> = op.carrying("sagger-count").map(Carrier::name).collect();
    assert_eq!(of_a_kind, ["saggarTally"]);

    let help = long_help(&op);
    assert!(
        !help.contains("sagger-count"),
        "a format is not a rule, so it says nothing on a help page:\n{help}"
    );

    // The two doors the same question is asked through, on an operation whose
    // values are declared in two halves.
    let starting = operation(&doc, "startFiring");
    let both: Vec<&str> = starting
        .carrying("cone-number")
        .map(Carrier::name)
        .collect();
    assert_eq!(both, ["cone", "bare_cone"]);
    assert!(
        starting
            .carrying("cone-number")
            .all(|carrier| matches!(carrier, Carrier::Field(_))),
        "both are body fields on this operation"
    );
}

// ------------------------------------------------------------- refusals

/// A `required` naming a key nothing declares is the document saying two things
/// at once. Every such node is named at once, spelled the way an Overlay names
/// a target, and an inline body is reached as readily as a named schema —
/// a scan restricted to `components.schemas` would report the class closed
/// while instances of it stand in bodies.
#[test]
fn a_required_key_nothing_declares_is_refused_and_names_every_node() {
    const PHANTOM: &str = r#"
openapi: 3.0.3
info: { title: Phantom, version: "1.0" }
servers: [{ url: "http://localhost:9411" }]
paths:
  /firings:
    post:
      operationId: startFiring
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [cone, soakMinutes]
              properties:
                cone: { type: string }
      responses:
        "201": { description: Created }
components:
  schemas:
    LogEntry:
      type: object
      required: [cone, spyhole]
      properties:
        cone: { type: string }
"#;

    let refused =
        Document::load(PHANTOM, &[]).expect_err("two nodes require what nothing declares");
    assert_eq!(
        refused.to_string(),
        "the document requires keys that nothing declares; declare each key in an \
         Overlay, or drop its name from the `required` that asks for it:\n  \
         $.paths['/firings'].post.requestBody.content['application/json'].schema \
         requires `soakMinutes`\n  $.components.schemas.LogEntry requires `spyhole`"
    );
}

/// A `pattern` no engine here reads refuses every value, so a document stating
/// one describes a flag nothing can satisfy. `(?P<name>…)` is the shape a
/// vendor writing against a Python or PCRE engine produces, and the refusal
/// names the operation, the value and the pattern.
#[test]
fn a_pattern_no_engine_runs_is_refused_and_names_the_operation_and_the_value() {
    const UNRUNNABLE: &str = r#"
openapi: 3.0.3
info: { title: Unrunnable, version: "1.0" }
servers: [{ url: "http://localhost:9411" }]
paths:
  /firings:
    get:
      operationId: listFirings
      parameters:
        - name: cone
          in: query
          schema: { type: string, pattern: '(?P<cone>0[1-9])' }
      responses:
        "200": { description: OK }
"#;

    let refused = Document::load(UNRUNNABLE, &[]).expect_err("the engine cannot read the pattern");
    assert!(
        matches!(&refused, LoadError::Unrunnable { op, name, source: ScalarError::Pattern { .. } }
            if op == "listFirings" && name == "cone"),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "listFirings: `cone` states a rule that cannot run"
    );
    assert_eq!(
        cause(&refused),
        "`(?P<cone>0[1-9])` is not a regular expression: Invalid group modifier"
    );
}

/// The same pattern on a body property, refused in the same words.
///
/// A rule is stated about a *value*, and a property of a flat body is a value
/// exactly as a query parameter is: the reduction reads both through one door,
/// and the refusal names the operation and the property that stated it.
///
/// What a document that blessed here would produce is two failures rather than
/// one. The property's flag refuses every value it is given, because the rule
/// it runs is one the engine cannot read — and the same `pattern` travels into
/// the newtype typify writes for the property, where the engine is reached
/// through a `Regex::new` that panics. So a rule nobody can run reaches a user
/// on the command line and a Rust caller through a panic, and neither of them
/// can see that the document is what was wrong.
#[test]
fn a_pattern_no_engine_runs_on_a_body_property_is_refused_and_names_the_property() {
    const UNRUNNABLE_BODY: &str = r#"
openapi: 3.0.3
info: { title: Unrunnable, version: "1.0" }
servers: [{ url: "http://localhost:9411" }]
paths:
  /firings:
    post:
      operationId: startFiring
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [cone]
              properties:
                cone: { type: string, pattern: '(?P<cone>0[1-9])' }
      responses:
        "201": { description: Created }
"#;

    let refused =
        Document::load(UNRUNNABLE_BODY, &[]).expect_err("the engine cannot read the pattern");
    assert!(
        matches!(&refused, LoadError::Unrunnable { op, name, source: ScalarError::Pattern { .. } }
            if op == "startFiring" && name == "cone"),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "startFiring: `cone` states a rule that cannot run"
    );
    assert_eq!(
        cause(&refused),
        "`(?P<cone>0[1-9])` is not a regular expression: Invalid group modifier"
    );
}

/// A schema whose `allOf` holds itself describes a value of no finite depth, so
/// following it terminates rather than hanging — and the refusal names the
/// operation, the parameter that led to the schema, and the reference the walk
/// came back to. A document of a thousand operations is not searchable by the
/// reference alone, and every other refusal here names both.
#[test]
fn a_schema_that_composes_itself_is_refused_and_names_where() {
    const OUROBOROS: &str = r##"
openapi: 3.0.3
info: { title: Ouroboros, version: "1.0" }
servers: [{ url: "http://localhost:9411" }]
paths:
  /firings:
    get:
      operationId: listFirings
      parameters:
        - name: saggar
          in: query
          schema: { $ref: "#/components/schemas/Saggar" }
      responses:
        "200": { description: OK }
components:
  schemas:
    Saggar:
      allOf:
        - $ref: "#/components/schemas/Saggar"
"##;

    let refused = Document::load(OUROBOROS, &[]).expect_err("the composition never bottoms out");
    assert!(
        matches!(&refused, LoadError::Unresolvable { op, name, .. }
            if op == "listFirings" && name == "saggar"),
        "and it is the refusal about a reference, not some other one: {refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "listFirings: `saggar` has a `$ref` that cannot be followed"
    );
    assert_eq!(
        cause(&refused),
        "`#/components/schemas/Saggar` is a reference cycle"
    );
}

/// A chain of `$ref`s resolves however long it is, and what refuses one is its
/// returning rather than its length.
///
/// The two documents differ in one character — what the last hop names — so the
/// pair pins that the edge is the shape of the chain and not a count of it. A
/// limit would be a floor under how deep a legal document may go, and a
/// document refused for passing it would be told it holds a cycle it does not.
///
/// Forty is past any limit anyone would have written down, and it is a document
/// a vendor produces by generating one `$ref` per version of a schema.
#[test]
fn a_ref_chain_resolves_however_long_it_is_and_only_a_cycle_refuses() {
    /// `hops` references leading to `last`, reached through one parameter.
    fn chained(hops: usize, last: &str) -> String {
        use std::fmt::Write as _;

        let mut schemas = String::new();
        for hop in 0..hops {
            let _ = writeln!(
                schemas,
                "    Hop{hop}: {{ $ref: \"#/components/schemas/Hop{}\" }}",
                hop + 1
            );
        }
        let _ = writeln!(schemas, "    Hop{hops}: {last}");
        format!(
            r##"
openapi: 3.0.3
info: {{ title: Chained, version: "1.0" }}
servers: [{{ url: "http://localhost:9411" }}]
paths:
  /firings:
    get:
      operationId: listFirings
      parameters:
        - name: cone
          in: query
          schema: {{ $ref: "#/components/schemas/Hop0" }}
      responses:
        "200": {{ description: OK }}
components:
  schemas:
{schemas}"##
        )
    }

    const ENDS: &str = "{ type: string }";
    const RETURNS: &str = r##"{ $ref: "#/components/schemas/Hop0" }"##;

    for hops in [1, 6, 8, 9, 40] {
        assert!(
            Document::load(&chained(hops, ENDS), &[]).is_ok(),
            "a chain of {hops} hops is finite and resolves"
        );
    }

    let refused =
        Document::load(&chained(8, RETURNS), &[]).expect_err("the chain returns to its start");
    assert!(
        matches!(&refused, LoadError::Unresolvable { op, name, .. }
            if op == "listFirings" && name == "cone"),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "listFirings: `cone` has a `$ref` that cannot be followed"
    );
    assert_eq!(
        cause(&refused),
        "`#/components/schemas/Hop0` is a reference cycle"
    );
}

/// The two gates a document passes are different gates, and a document can pass
/// the first and fail the second. An `operationId` starting with a digit is a
/// command line this crate builds without difficulty — the command name comes
/// off the path — and a Rust identifier it cannot spell, so the refusal happens
/// at the bless step and names the operation.
///
/// Naming it is the whole point: a generated file is thousands of lines, and a
/// `syn` position in a token stream is not something an adopter can open.
#[test]
fn an_operation_id_that_is_no_rust_identifier_reduces_and_refuses_to_bless() {
    const DIGITS: &str = r#"
openapi: 3.0.3
info: { title: Digits, version: "1.0" }
servers: [{ url: "http://localhost:9411" }]
paths:
  /firings:
    get:
      operationId: 3rdFiring
      responses:
        "200": { description: OK }
"#;

    let doc = Document::load(DIGITS, &[]).expect("a command name comes off the path");
    let op = operation(&doc, "3rdFiring");
    assert_eq!(
        (op.group().as_str(), op.command().as_str()),
        ("firings", "list")
    );

    let dir = out("hostile-digits");
    let refused = Settings::new(wrote(&dir, "document.yaml", DIGITS))
        .write_to(&dir)
        .expect_err("`3rdFiring` starts an identifier nowhere");
    assert!(
        matches!(&refused, GenerateError::Operation { op, .. } if op == "3rdFiring"),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "the wrapper for 3rdFiring cannot be generated"
    );
    assert_eq!(
        cause(&refused),
        "the operationId has no spelling as a Rust identifier"
    );
}

// ------------------------------------------------------------ clap's own word

/// `help` is clap's before it is anyone's. A vendor who spells a parameter or a
/// property that way gets `--param-help` and `--body-help`, a vendor whose path
/// begins with it gets the group the marker names, and `--help` still prints a
/// help page and runs nothing.
#[test]
fn a_vendors_help_moves_aside_and_the_help_flag_still_prints_help() {
    let doc = document();
    let listing = operation(&doc, "listFirings");
    assert_eq!(flag_of(&listing, "help").as_deref(), Some("param-help"));
    assert!(
        unwrapped(&listing)
            .contains("--param-help <STRING> The `help` query parameter (sends `help`)"),
        "the flag that moved says which wire name it carries:\n{}",
        long_help(&listing)
    );

    let starting = operation(&doc, "startFiring");
    let Body::JsonFields { fields, .. } = starting.body() else {
        panic!("every property of this body is a value, so it is flat");
    };
    let help = fields
        .iter()
        .find(|field| field.name() == "help")
        .expect("the document declares the property");
    assert_eq!(help.flag(), "body-help");
    assert!(help.renamed(), "and it says so in its help line");

    let notes = operation(&doc, "listKilnNotes");
    assert_eq!(
        (notes.group().as_str(), notes.command().as_str()),
        ("kiln-help", "kiln-notes"),
        "the marker names the group that the path would have called `help`"
    );

    let root = clap::Command::new("saggar")
        .subcommand_required(true)
        .subcommands(typed_openapi::tree::commands(&doc));
    for args in [
        ["saggar", "firings", "list", "--help"],
        ["saggar", "firings", "create", "--help"],
        ["saggar", "kiln-help", "kiln-notes", "--help"],
    ] {
        let asked = root
            .clone()
            .try_get_matches_from(args)
            .expect_err("`--help` asks for a page and runs no command");
        assert_eq!(
            asked.kind(),
            clap::error::ErrorKind::DisplayHelp,
            "{args:?}"
        );
    }
}

/// The three spellings the control document cannot hold, because a document
/// holding one is refused. Each is the control document itself with one edit,
/// so what is refused is the control document's own vocabulary and not a toy
/// beside it.
#[test]
fn the_control_document_is_refused_for_a_help_that_carries_consent_or_names_a_subcommand() {
    // The document with `from` written as `to`. `from` is in it exactly once, so
    // an edit that stopped matching fails here and not as a quiet pass.
    let edited = |from: &str, to: &str| {
        assert_eq!(HOSTILE.matches(from).count(), 1, "`{from}` in the fixture");
        HOSTILE.replace(from, to)
    };

    // The group `help`: the path's own first segment, without the marker.
    let group = Document::load(&edited("      x-cli-group: kiln-help\n", ""), &[])
        .expect_err("a path whose first segment is clap's own subcommand");
    assert!(
        matches!(&group, LoadError::NameSpent { op, what: "group", name }
            if op == "listKilnNotes" && name == "help"),
        "{group:?}"
    );
    assert_eq!(
        group.to_string(),
        "listKilnNotes: the group `help` is a subcommand that clap declares itself; \
         name it with `x-cli-group` in an Overlay"
    );

    // The command `help`: the last literal segment of a path below a group.
    let command = Document::load(&edited("/wadding/tallies:", "/wadding/help:"), &[])
        .expect_err("a command named for clap's own subcommand");
    assert!(
        matches!(&command, LoadError::NameSpent { op, what: "command", name }
            if op == "tallyWadding" && name == "help"),
        "{command:?}"
    );

    // The gate `help`: `--help` would answer the gate that stands in front of a
    // write, and `--commit --help` would send it.
    let gate = Document::load(
        &edited(
            "      operationId: startFiring\n",
            "      operationId: startFiring\n      x-cli-gates: [help]\n",
        ),
        &[],
    )
    .expect_err("a gate that `--help` answers");
    assert!(
        matches!(&gate, LoadError::ReservedGate { op, gate }
            if op == "startFiring" && gate.as_str() == "help"),
        "{gate:?}"
    );
    assert_eq!(
        gate.to_string(),
        "startFiring: the gate `help` is one of the flags every subcommand already spends"
    );
}

// --------------------------------------------------------- the whole thing

/// The command tree the control document mounts is one clap will not panic on.
///
/// `debug_assert` is where clap reports a tree it would refuse at run time — a
/// duplicate argument id, two subcommands under one name — and a document whose
/// names collide is exactly what would produce one.
#[test]
fn the_control_document_mounts_as_a_command_tree_clap_accepts() {
    let doc = document();
    clap::Command::new("saggar")
        .subcommand_required(true)
        .subcommands(typed_openapi::tree::commands(&doc))
        .debug_assert();
}

/// Every count an adopter would be tempted to write down, taken off the model.
///
/// The numbers are here so that a shape added to the fixture has to be
/// accounted for rather than absorbed: a tenth operation or a sixth
/// unspellable parameter fails this test before it reaches whichever test was
/// meant to cover it.
#[test]
fn the_reduction_of_the_control_document_counts_what_is_in_it() {
    let summary = document().summary();
    assert_eq!(summary.operations(), 9);
    assert_eq!(
        summary
            .groups()
            .iter()
            .map(CommandName::as_str)
            .collect::<Vec<_>>(),
        [
            "firings",
            "wadding",
            "glaze-recipes",
            "payments",
            "kiln-help"
        ]
    );
    assert_eq!(summary.reads(), 3);
    assert_eq!(summary.writes(), 6);
    assert_eq!(summary.bodiless(), 4);
    // Four rather than the three bodies that are `JsonWhole`: a flat body of no
    // properties offers a command line `--json-body` and nothing else, which is
    // what the count is about.
    assert_eq!(summary.whole_bodies(), 4);
    assert_eq!(summary.unreachable().len(), 5);
}

/// Every awkward name survives the blob, because a shipped binary meets the
/// blob and never the document. A flag that moved aside, a key no flag could
/// carry and a command name taken off a path are all decided while the document
/// is reduced, so all three have to come back off the bytes unchanged.
#[test]
fn every_awkward_name_comes_back_off_the_blob_as_it_went_in() {
    let doc = document();
    let blob = doc.to_blob().expect("the reduction encodes");
    let shipped = Document::from_blob(&blob).expect("and decodes");

    let listing = operation(&shipped, "listFirings");
    assert_eq!(flag_of(&listing, "Self").as_deref(), Some("param-self"));
    assert_eq!(flag_of(&listing, "crate").as_deref(), Some("crate"));

    let appending = operation(&shipped, "appendLogEntry");
    let carried: Vec<Option<String>> = appending
        .params()
        .iter()
        .map(|param| match param.shape() {
            Shape::Flag { flag, .. } => Some(flag.clone()),
            Shape::Unreachable(_) => None,
        })
        .collect();
    assert_eq!(
        carried,
        [Some("ref".to_owned()), Some("param-ref".to_owned())]
    );

    let packing = operation(&shipped, "packWadding");
    let Body::JsonWhole { template, .. } = packing.body() else {
        panic!("a body goes whole across the blob as readily as it goes flat");
    };
    assert_eq!(
        template.as_deref(),
        Some("{\n  \"*\": \"\",\n  \"grog\": \"\"\n}"),
        "the key no flag could carry comes back off the bytes"
    );

    assert_eq!(
        operation(&shipped, "fileGlazeRecipe").group().as_str(),
        "glaze-recipes"
    );
}

// ------------------------------------------------ the generated code compiles

/// The crate root that the generated files are compiled under: the smallest one
/// that `docs/generating.md` gives, with a sentence on every public item, because
/// the crate is built under the strictest lint set an adopter can configure and
/// its own lines are no exception.
const ROOT: &str = r#"//! The crate that holds the generated files of the hostile document.

pub mod ops;
pub mod pyrometry;
pub mod types;

use typed_openapi::client::Client;
pub use typed_openapi::client::{Call, Error, NoContent, to_json};
use typed_openapi::{Document, Values};

use crate::ops::{OPERATIONS, OperationId};

const MODEL: &[u8] = include_bytes!("model.postcard");

/// The handle that every generated wrapper is a method of.
pub struct Api(Client);

impl Api {
    /// Pairs the blob with the generated inventory.
    pub fn new() -> Result<Self, Error> {
        let client = Client::over(Document::from_blob(MODEL)?);
        client.document().matches(OPERATIONS)?;
        Ok(Self(client))
    }

    /// The document that the handle was made over.
    pub fn document(&self) -> &Document {
        self.0.document()
    }

    /// Names an operation and the values for it.
    pub fn call<T>(&self, op: OperationId, values: Values) -> Result<Call<'_, T>, Error> {
        let operation = &self.0.document().operations()[op as usize];
        self.0.call(operation, values)
    }
}
"#;

/// The Rust type that [`OWNED`] names, in the crate root's module of that name.
const PYROMETRY: &str = r"//! The type an adopter of the hostile document owns for its cone numbers.

use std::fmt;
use std::str::FromStr;

/// A pyrometric cone number.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct Cone(String);

impl fmt::Display for Cone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Cone {
    type Err = std::convert::Infallible;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self(text.to_owned()))
    }
}
";

/// Where cargo builds every crate of this file, so that a dependency is built once
/// for all of them. A test is a process of its own under nextest and cargo locks
/// the directory, so the crates are built one after another.
fn blessed_target() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("blessed-target")
}

/// The lockfile that the crate below is resolved against, with no network: the
/// workspace's, beside the manifest of a checkout, and the package's own in the
/// tarball that `just package` unpacks, which carries it.
fn lockfile() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    [manifest.join("Cargo.lock"), manifest.join("../Cargo.lock")]
        .into_iter()
        .find(|path| path.is_file())
        .expect("a Cargo.lock beside the manifest, or in the workspace above it")
}

/// The bless output in `dir` as a crate that cargo builds: a manifest that holds
/// the strictest lint set an adopter can set (`-D warnings` over `missing_docs`
/// and `unused_qualifications`, which the generated files' own `#![allow]` has to
/// cover), the root, the owned type and the lockfile.
fn made_a_crate(dir: &Path) {
    let manifest = format!(
        "[package]\nname = \"blessed\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = \
         false\n\n[workspace]\n\n[features]\nbuilder = [\"typed-openapi/builder\"]\n\n\
         [dependencies]\ntyped-openapi = {{ path = {path:?}, default-features = false }}\nserde = \
         {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\n\n[lints.rust]\n\
         warnings = \"deny\"\nmissing_docs = \"warn\"\nunused_qualifications = \"warn\"\n",
        path = env!("CARGO_MANIFEST_DIR"),
    );
    for (name, contents) in [
        ("Cargo.toml", manifest.as_str()),
        ("src/lib.rs", ROOT),
        ("src/pyrometry.rs", PYROMETRY),
    ] {
        std::fs::write(dir.join(name), contents).expect("a file of the crate");
    }
    std::fs::copy(lockfile(), dir.join("Cargo.lock")).expect("the lockfile");
}

/// One run of cargo over the crate in `dir`, with no network, and what it said on
/// standard error where it failed.
fn cargo(dir: &Path, args: &[&str]) -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(args)
        .args(["--offline", "--color", "never", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(blessed_target())
        .output()
        .expect("cargo is on PATH beside the rustfmt a bless step already needs");
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "`cargo {}` failed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// Whether the bless output in `dir` is a crate that compiles: without the
/// `builder` feature, with it, and whether its doctests run.
///
/// The doctests are the ones that a vendor's prose would add (spec F-2) and the
/// generator's own. They are run with the feature on, which holds every doc
/// comment that the generator writes.
fn compiles(dir: &Path) -> Result<(), String> {
    made_a_crate(dir);
    cargo(dir, &["check"])?;
    cargo(dir, &["check", "--features", "builder"])?;
    cargo(dir, &["test", "--doc", "--features", "builder"])
}

/// The control document, blessed with the adopter's type, is a crate that
/// compiles with and without `builder`, under `-D warnings` over the lints that
/// an adopter would add, and whose doctests run. Nothing in the suite compiled
/// generated code for any document but the toy before this (F-69), so a compile
/// failure here is the failure of every shape the fixture holds: a description
/// that hides a code block (F-2), a schema named after a standard item (F-12), a
/// parameter whose name starts with a digit (F-48) and the lints (F-49).
#[test]
fn the_generated_code_of_the_control_document_compiles_with_and_without_the_builder() {
    let dir = blessed("hostile-compiles");
    if let Err(compiler) = compiles(&dir) {
        panic!("the bless of the control document is not a crate that compiles:\n{compiler}");
    }
}

// ------------------------------------------- a name that two items of the code share

/// A document made of `paths` and, where there are any, `schemas`, each on lines
/// of their own indented by two and four spaces.
fn collision(paths: &str, schemas: &str) -> String {
    let components = if schemas.is_empty() {
        String::new()
    } else {
        format!("components:\n  schemas:\n{schemas}")
    };
    format!(
        "openapi: 3.0.3\ninfo: {{ title: Collision, version: \"1.0\" }}\nservers: [{{ url: \
         \"http://localhost:9411\" }}]\npaths:\n{paths}{components}"
    )
}

/// One document in which two items of the generated code would have one name.
struct Collision {
    /// What is shared.
    label: &'static str,
    /// The paths of the document, and its schemas.
    document: (&'static str, &'static str),
    /// Whether the refusal is the variant that names it.
    variant: fn(&GenerateError) -> bool,
    /// The refusal's own words, which name the two items.
    says: &'static str,
}

/// Two operations of one `operationId`, and the three pairs that one Rust name
/// reaches from the other side: a wrapper beside a wrapper, a wrapper beside the
/// `_builder` method of another operation, and a wrapper beside a method of the
/// handle.
const COLLISIONS: [Collision; 11] = [
    Collision {
        label: "two operations of one operationId (probes/A3a, `same`)",
        document: (
            "  /things: { get: { operationId: same, responses: { '200': { description: OK } } } }\n  \
             /v2/things: { get: { operationId: same, responses: { '200': { description: OK } } } }\n",
            "",
        ),
        variant: |error| {
            matches!(error, GenerateError::Unusable(LoadError::DuplicateOperationId { id, .. })
                if id == "same")
        },
        says: "`same` is the operationId of both `get /things` and `get /v2/things`; an Overlay \
               gives one of them another",
    },
    Collision {
        label: "`listThings` beside `list_things` (probes/A7, `ids`)",
        document: (
            "  /things: { get: { operationId: listThings, responses: { '200': { description: OK } } } }\n  \
             /other: { get: { operationId: list_things, responses: { '200': { description: OK } } } }\n",
            "",
        ),
        variant: |error| matches!(error, GenerateError::DuplicateIdentifier { name, .. } if name == "list_things"),
        says: "`list_things` would be defined twice in the generated ops.rs, as the wrapper of \
               `listThings` and as the wrapper of `list_things`; correct one of them in an Overlay",
    },
    Collision {
        label: "an operation called `<x>Builder` beside `<x>` (probes/A4, `opbuilder`)",
        document: (
            "  /reports: { get: { operationId: getReport, responses: { '200': { description: OK } } } }\n  \
             /reports/builder: { get: { operationId: getReportBuilder, responses: { '200': { \
             description: OK } } } }\n",
            "",
        ),
        variant: |error| {
            matches!(error, GenerateError::DuplicateIdentifier { name, .. }
                if name == "get_report_builder")
        },
        says: "`get_report_builder` would be defined twice in the generated ops.rs, as the \
               `_builder` method of `getReport` and as the wrapper of `getReportBuilder`; correct \
               one of them in an Overlay",
    },
    Collision {
        label: "an operation called `call`, the handle's own method (probes/A7, `apimethod`)",
        document: (
            "  /calls: { post: { operationId: call, responses: { '200': { description: OK } } } }\n",
            "",
        ),
        variant: |error| matches!(error, GenerateError::DuplicateIdentifier { name, .. } if name == "call"),
        says: "`call` would be defined twice in the generated ops.rs, as the handle's own `call` \
               and as the wrapper of `call`; correct one of them in an Overlay",
    },
    Collision {
        label: "an operation called `document`, the handle's own method (probes/A7, `apimethod`)",
        document: (
            "  /documents: { get: { operationId: document, responses: { '200': { description: OK } } } }\n",
            "",
        ),
        variant: |error| matches!(error, GenerateError::DuplicateIdentifier { name, .. } if name == "document"),
        says: "`document` would be defined twice in the generated ops.rs, as the handle's own \
               `document` and as the wrapper of `document`; correct one of them in an Overlay",
    },
    Collision {
        label: "two operationIds that are one variant of `OperationId`",
        document: (
            "  /a: { get: { operationId: a1, responses: { '200': { description: OK } } } }\n  \
             /b: { get: { operationId: a_1, responses: { '200': { description: OK } } } }\n",
            "",
        ),
        variant: |error| matches!(error, GenerateError::DuplicateIdentifier { name, .. } if name == "A1"),
        says: "`A1` would be defined twice in the generated ops.rs, as the variant of `a1` and as \
               the variant of `a_1`; correct one of them in an Overlay",
    },
    Collision {
        label: "a parameter called `call`, which is the method that ends a builder (probes/A4, `bon`)",
        document: (
            "  /calls:\n    get:\n      operationId: listCalls\n      parameters:\n        - { name: call, in: query, schema: { type: string } }\n      \
             responses: { '200': { description: OK } }\n",
            "",
        ),
        variant: |error| matches!(error, GenerateError::DuplicateIdentifier { name, .. } if name == "call"),
        says: "`call` would be defined twice in the generated ops.rs, as the method that finishes \
               the builder of `listCalls` and as the setter `call` of the parameter `call`; \
               correct one of them in an Overlay",
    },
    Collision {
        label: "`limit` beside `maybe_limit`, which bon derives for the first (probes/A4, `bon`)",
        document: (
            "  /calls:\n    get:\n      operationId: listCalls\n      parameters:\n        - { name: limit, in: query, schema: { type: integer } }\n        \
             - { name: maybe_limit, in: query, schema: { type: integer } }\n      responses: { '200': { description: OK } }\n",
            "",
        ),
        variant: |error| matches!(error, GenerateError::DuplicateIdentifier { name, .. } if name == "maybe_limit"),
        says: "`maybe_limit` would be defined twice in the generated ops.rs, as the setter \
               `maybe_limit` of the parameter `limit` and as the setter `maybe_limit` of the \
               parameter `maybe_limit`; correct one of them in an Overlay",
    },
    Collision {
        label: "a schema that is the type typify derives for a nested object (probes/A4, `nested2`)",
        document: (
            "  /foos: { get: { operationId: listFoos, responses: { '200': { description: OK, \
             content: { application/json: { schema: { $ref: '#/components/schemas/Foo' } } } } } } }\n",
            "    Foo: { type: object, properties: { bar: { type: object, properties: { x: { type: string } } } } }\n    \
             FooBar: { type: object, properties: { y: { type: integer } } }\n",
        ),
        variant: |error| matches!(error, GenerateError::OneType { inline: true, rust, .. } if rust == "FooBar"),
        says: "the schema `FooBar` and a schema nested in `Foo` are both `FooBar` in Rust; give an \
               inline schema a `title` in an Overlay",
    },
    Collision {
        label: "two properties that are one field (probes/A4, `propcase`)",
        document: (
            "  /things: { get: { operationId: listThings, responses: { '200': { description: OK, \
             content: { application/json: { schema: { $ref: '#/components/schemas/Thing' } } } } } } }\n",
            "    Thing: { type: object, properties: { fooBar: { type: string }, foo_bar: { type: string } } }\n",
        ),
        variant: |error| matches!(error, GenerateError::DuplicateField { ty, field } if ty == "Thing" && field == "foo_bar"),
        says: "the type `Thing` would declare the field `foo_bar` twice, because two of its \
               properties are one name in Rust; correct one of them in an Overlay",
    },
    Collision {
        label: "two properties that are one field of an object that a oneOf holds",
        document: (
            "  /things: { get: { operationId: listThings, responses: { '200': { description: OK, \
             content: { application/json: { schema: { $ref: '#/components/schemas/Either' } } } } } } }\n",
            "    Either:\n      oneOf:\n        - { type: object, title: Left, required: [fooBar, foo_bar], properties: { fooBar: { type: string }, \
             foo_bar: { type: string } } }\n        - { type: integer }\n",
        ),
        variant: |error| matches!(error, GenerateError::DuplicateField { ty, field } if ty == "Either" && field == "foo_bar"),
        says: "the type `Either` would declare the field `foo_bar` twice, because two of its \
               properties are one name in Rust; correct one of them in an Overlay",
    },
];

/// What blessing a document refuses with. A bless that succeeds has to have written
/// a crate that compiles, and where it did not, the test fails with what the
/// compiler said: that is a collision the generator let through, the defect that
/// this table exists to find, and the one a removed refusal turns back into.
fn refused_by_name(name: &str, document: &str) -> GenerateError {
    let dir = out(name);
    let blessed = Settings::new(wrote(&dir, "document.yaml", document))
        .replace(OWNED.0, OWNED.1)
        .write_to(&dir);
    match blessed {
        Err(error) => error,
        Ok(_) => match compiles(&dir) {
            Err(compiler) => panic!(
                "{name}: the bless succeeded over a crate that does not compile:\n{compiler}"
            ),
            Ok(()) => {
                panic!("{name}: the bless succeeded, and the crate compiles: it holds no collision")
            }
        },
    }
}

/// The bless never succeeds over a crate that does not compile (F-12): each of the
/// documents above is refused, by the variant that names it and in words that name
/// both items. Each one blessed with exit 0 at `74b77b9` and was a compile error in
/// the generated file, so a refusal that is removed turns its row into the
/// compiler's output.
#[test]
fn two_items_of_the_generated_code_that_share_a_name_refuse_the_bless_by_name() {
    for (at, collision) in COLLISIONS.iter().enumerate() {
        let (paths, schemas) = collision.document;
        let name = format!("hostile-collision-{at}");
        let refused = refused_by_name(&name, &self::collision(paths, schemas));
        assert!(
            (collision.variant)(&refused),
            "{}: {refused:?}",
            collision.label
        );
        let said = if let GenerateError::Unusable(cause) = &refused {
            cause.to_string()
        } else {
            refused.to_string()
        };
        assert_eq!(said, collision.says, "{}", collision.label);
        let written = out(&name).join("src");
        assert!(
            !written.exists(),
            "{}: a refused bless wrote into the tree",
            collision.label
        );
    }
}
