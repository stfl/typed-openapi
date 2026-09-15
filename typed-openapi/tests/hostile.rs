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
//! Three shapes here produce something this crate would not choose. They are
//! pinned exactly as they are, with the test saying so, because a
//! characterisation is what makes a repair visible: the day one is fixed, the
//! test naming it goes red and whoever fixed it reads what the old behaviour
//! was. They are
//! [`two_parameters_that_spell_one_identifier_collide_in_the_wrapper`],
//! [`a_ref_chain_longer_than_the_hop_limit_is_refused_as_a_cycle_it_is_not`]
//! and [`a_property_whose_name_has_no_letters_becomes_a_flag_with_no_name`].

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::path::{Path, PathBuf};

use typed_openapi::generate::Settings;
use typed_openapi::model::Body;
use typed_openapi::{
    Carrier, CommandName, Document, Field, LoadError, Operation, Scalar, Shape, Unsupported,
};

/// The control document, read the way an adopter reads it.
const HOSTILE: &str = include_str!("fixtures/hostile.yaml");

/// The same file by path, for the bless step, which reads documents itself.
const HOSTILE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hostile.yaml");

/// The Rust type an adopter of this document already owns for its cone numbers.
///
/// A path that names no crate anywhere in this workspace, on purpose: the
/// generated file is read as text and never compiled, and a path that resolved
/// would invite somebody to compile it and then to keep it compiling.
const OWNED: (&str, &str) = ("cone-number", "pyrometry::Cone");

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
        .split_once(") -> Result")
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
/// aside — the same rule a body field follows.
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
        ("Self", "self_"),
    ] {
        assert!(
            ops.contains(&format!(".maybe(\"{word}\", {spelled})")),
            "`{word}` does not reach the request as `{spelled}`"
        );
    }
}

/// **A defect, pinned as it stands.**
///
/// Two parameters whose names snake-case onto one word bind one identifier in
/// the generated wrapper. `self` and `Self` both become `self_`, and the `ref`
/// in a path and the `ref` in a query are both `r#ref` — so the emitted file
/// carries `fn list_firings(…, self_: …, crate_: …, self_: …)`, which is
/// `E0415`, and `fn append_log_entry(r#ref: &str, r#ref: Option<&str>, …)`,
/// which is the same. A bless step reports success and the adopter's crate does
/// not compile; were it to compile, the second `.maybe` would send the first
/// argument's value under the second's wire name.
///
/// The command line has an answer for exactly this — `Namespace::claim` moves
/// the second claimant aside, which is why `--self` and `--param-self` are two
/// flags above — and the wrapper has none, so the two consumers disagree about
/// one operation.
///
/// What this test pins is the collision, so that a repair turns it red rather
/// than passing silently.
#[test]
fn two_parameters_that_spell_one_identifier_collide_in_the_wrapper() {
    let ops = read(&blessed("hostile-collision").join("src/ops.rs"));

    let listing = arguments(&ops, "list_firings");
    assert_eq!(
        listing.iter().filter(|name| *name == "self_").count(),
        2,
        "`self` and `Self` are expected to collide today: {listing:?}"
    );

    let appending = arguments(&ops, "append_log_entry");
    assert_eq!(
        appending.iter().filter(|name| *name == "r#ref").count(),
        2,
        "the path `ref` and the query `ref` are expected to collide today: {appending:?}"
    );
}

/// One wire name in two places is two flags, and the one that moved says which
/// wire name it carries — the only thing left to go by once the document is
/// gone.
#[test]
fn one_wire_name_in_two_places_is_two_flags_and_the_second_says_so() {
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
        long_help(&op).contains("(sends `ref`)"),
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

    assert!(
        types.contains("-    bisque, twenty minutes"),
        "the item lost its marker or its gap:\n{types}"
    );
    assert!(
        !types.contains("-   bisque"),
        "the gap is four columns, not three"
    );
    assert!(
        !types.contains("*     bisque"),
        "a `*` bullet is eaten out of a block comment, so it is written as `-`"
    );
    assert!(
        types.contains("\n   soak=90m\n"),
        "the vendor's four-space block has to land on three:\n{types}"
    );
    assert!(
        !types.contains("\n    soak=90m\n"),
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
    let Body::JsonFields(fields) = op.body() else {
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
    let Body::JsonFields(fields) = op.body() else {
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
        ]
    );

    assert!(
        long_help(&op).contains("matches ^0[1-9][0-9]?$"),
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
    let Body::JsonWhole { required, template } = op.body() else {
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
    let Body::JsonWhole { required, template } = body else {
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
    let Body::JsonFields(fields) = op.body() else {
        panic!("an object is a flat body whether or not it declares anything");
    };
    assert_eq!(fields.as_slice(), []);
    let help = long_help(&op);
    assert!(help.contains("--json-body <FILE>"), "{help}");
}

/// **A defect, pinned as it stands.**
///
/// A property named `*` kebab-cases to nothing, and nothing is what the flag is
/// called. `--help` prints it as `-- <STRING>`, which is the end-of-options
/// marker and not a flag anybody would try; the one spelling that reaches it is
/// `--=VALUE`, and the help line does not say which key it carries, because a
/// flag is only told to name its wire name when it *moved*.
///
/// The value does arrive under the right key, so nothing is sent wrongly. What
/// is wrong is that a name the command line cannot spell is spelled anyway:
/// `names::spelled` exists and refuses exactly this for a command name and for
/// a gate, and the flag a parameter or a body field claims never goes through
/// it. Carrying such a value as unreachable — the answer a parameter this crate
/// cannot spell already gets — would say so at bless time instead.
///
/// A *parameter* named this way lands in the same place with less room around
/// it: a body keeps `--json-body` as a second door, where a parameter has only
/// the `--=VALUE` spelling — which a required one then makes compulsory.
#[test]
fn a_property_whose_name_has_no_letters_becomes_a_flag_with_no_name() {
    use clap::ArgMatches;

    let doc = document();
    let op = operation(&doc, "packWadding");
    let Body::JsonFields(fields) = op.body() else {
        panic!("both properties are values, so the body is flat");
    };
    let starred = fields
        .iter()
        .find(|field| field.name() == "*")
        .expect("the document declares a property named `*`");
    assert_eq!(starred.flag(), "", "expected today: the flag has no name");
    assert!(starred.required());
    assert!(
        !starred.renamed(),
        "expected today: nothing tells the user which key this flag carries"
    );

    let help = long_help(&op);
    assert!(
        help.contains("-- <STRING>"),
        "expected today: the help prints the end-of-options marker as a flag:\n{help}"
    );

    let parsed = |args: &[&str]| -> Result<ArgMatches, clap::Error> {
        typed_openapi::tree::command(&op).try_get_matches_from(args)
    };
    assert!(
        parsed(&["create", "--grog", "silica", "--", "kaolin"]).is_err(),
        "expected today: `--` is not a way to reach it"
    );
    let matches = parsed(&["create", "--grog", "silica", "--=kaolin"])
        .expect("expected today: `--=VALUE` is the one spelling that reaches it");
    let values = typed_openapi::tree::values(&op, &matches).expect("the values are read");
    assert_eq!(
        format!("{values:?}"),
        r#"Values { params: [], body: Some(Json(Object {"*": String("kaolin"), "grog": String("silica")})) }"#,
        "whatever the flag is called, the key on the wire is the document's"
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
        types.contains("pub struct Cone(pub pyrometry::Cone);"),
        "the schema whose name the path ends in still wraps:\n{types}"
    );
    assert!(
        types.contains("pub struct PyroCone(pub pyrometry::Cone);"),
        "and so does the one whose name it does not:\n{types}"
    );
    assert!(
        types.contains("pub bare_cone: ::std::option::Option<pyrometry::Cone>"),
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
    let Body::JsonFields(fields) = op.body() else {
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
    assert_eq!(
        refused.to_string(),
        "listFirings: `cone`: `(?P<cone>0[1-9])` is not a regular expression: \
         Invalid group modifier"
    );
}

/// A schema whose `allOf` holds itself describes a value of no finite depth.
/// Following it terminates rather than hanging, which is the part that matters.
///
/// **A defect in what it says, pinned as it stands.** The refusal names neither
/// the operation nor the parameter that led to the schema, where every other
/// refusal here names both — so an adopter meeting this on a document of a
/// thousand operations is told a cycle exists and not where.
#[test]
fn a_schema_that_composes_itself_is_refused_without_naming_where() {
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
    assert_eq!(
        refused.to_string(),
        "`a reference cycle` does not resolve",
        "expected today: the refusal names neither `listFirings` nor `saggar`"
    );
}

/// **A defect, pinned as it stands.**
///
/// Following a `$ref` stops after eight hops, and a chain that runs past the
/// limit is reported as a cycle. A chain of eight is legal and finite and
/// resolves to a string; the document is refused all the same, and the sentence
/// an adopter reads describes something the document does not contain.
///
/// The document below the assertion is the one hop shorter that loads, so the
/// pair pins where the edge is as well as what is said at it.
#[test]
fn a_ref_chain_longer_than_the_hop_limit_is_refused_as_a_cycle_it_is_not() {
    fn chained(hops: usize) -> String {
        use std::fmt::Write as _;

        let mut schemas = String::new();
        for hop in 0..hops {
            let _ = writeln!(
                schemas,
                "    Hop{hop}: {{ $ref: \"#/components/schemas/Hop{}\" }}",
                hop + 1
            );
        }
        let _ = writeln!(schemas, "    Hop{hops}: {{ type: string }}");
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

    assert!(
        Document::load(&chained(6), &[]).is_ok(),
        "a chain inside the limit resolves"
    );
    let refused = Document::load(&chained(8), &[]).expect_err("a chain past the limit does not");
    assert_eq!(
        refused.to_string(),
        "`a reference cycle` does not resolve",
        "expected today: a finite chain is reported as a cycle"
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
    assert_eq!(
        refused.to_string(),
        "3rdFiring: the operationId has no spelling as a Rust identifier"
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
/// accounted for rather than absorbed: a seventh operation or a sixth
/// unspellable parameter fails this test before it reaches whichever test was
/// meant to cover it.
#[test]
fn the_reduction_of_the_control_document_counts_what_is_in_it() {
    let summary = document().summary();
    assert_eq!(summary.operations(), 7);
    assert_eq!(
        summary
            .groups()
            .iter()
            .map(CommandName::as_str)
            .collect::<Vec<_>>(),
        ["firings", "wadding", "glaze-recipes"]
    );
    assert_eq!(summary.reads(), 1);
    assert_eq!(summary.writes(), 6);
    assert_eq!(summary.bodiless(), 2);
    // Three rather than the two bodies that are `JsonWhole`: a flat body of no
    // properties offers a command line `--json-body` and nothing else, which is
    // what the count is about.
    assert_eq!(summary.whole_bodies(), 3);
    assert_eq!(summary.unreachable().len(), 5);
}

/// Every awkward name survives the blob, because a shipped binary meets the
/// blob and never the document. A flag that moved aside, a flag with no name at
/// all and a command name taken off a path are all decided while the document
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
    let Body::JsonFields(fields) = packing.body() else {
        panic!("a flat body stays flat across the blob");
    };
    assert_eq!(
        fields.iter().map(Field::flag).collect::<Vec<_>>(),
        ["", "grog"],
        "every field comes back, the one with no name included"
    );

    assert_eq!(
        operation(&shipped, "fileGlazeRecipe").group().as_str(),
        "glaze-recipes"
    );
}
