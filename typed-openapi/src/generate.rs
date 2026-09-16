//! The bless step: a vendor's OpenAPI document and an adopter's Overlay in,
//! one corrected document and the Rust an adopter compiles against out.
//!
//! An adoption runs this once per vendor revision and commits everything it
//! writes. That is what makes a vendor change reviewable: the diff after a
//! bless run is the answer to "what did the vendor do?", in Rust rather than in
//! YAML.
//!
//! # What it writes
//!
//! Four artefacts under one directory, all derived from a single Overlay
//! application so that none of them can describe a different API:
//!
//! - `spec/<name>.overlaid.yaml` — the corrected document, and the reviewable
//!   record of what everything below it came from.
//! - `src/types.rs` — `components.schemas` as Rust types, from typify, with the
//!   adopter's own types substituted wherever [`Settings::replace`] says.
//! - `src/ops.rs` — one typed wrapper per operation, the closed `OperationId`
//!   set, and the `(operationId, method, path)` inventory a hand-written
//!   operation asserts against.
//! - `src/model.postcard` — that same document already reduced to the facts a
//!   command line needs, so a shipped binary parses no YAML and enables no
//!   feature that could.
//!
//! An adoption that quotes a count in its own prose asks for a fifth.
//! [`Settings::summary_page`] names where it goes, and the bless step renders
//! [`Summary`](crate::Summary) there: the operations, the groups, the reads and
//! writes, what stands behind each named gate, and the parameters carried
//! without a flag. The prose quotes the page and a test asserts against
//! `Summary`, so the count is measured rather than remembered. An adoption that
//! quotes no count names no path, and its tree gains no file.
//!
//! # Using it
//!
//! ```no_run
//! # fn main() -> Result<(), typed_openapi::generate::GenerateError> {
//! typed_openapi::generate::Settings::new("spec/vendor.yaml")
//!     .overlay("spec/corrections.yaml")
//!     .overlay("spec/cli.yaml")
//!     .replace("money", "money::Money")
//!     .summary_page("src/summary.md")
//!     .write_to("api-generated")?;
//! # Ok(())
//! # }
//! ```
//!
//! `examples/toy/xtask` is that call in a binary, written to be copied.
//!
//! # Corrections come in layers
//!
//! [`Settings::overlay`] may be called more than once, and the order of the
//! calls is the order the Overlays are applied: each one corrects the document
//! the ones before it produced. What an adoption puts in which layer is its
//! own affair — this crate reads an ordered list of standard Overlay documents
//! and nothing more. `docs/overlay.md` recommends a split, and the example
//! keeps it.
//!
//! # Every Overlay is applied strictly
//!
//! [`overlay::apply`] uses `ErrorOnZeroMatch`, so a correction whose target the
//! vendor has renamed or retyped fails here rather than lapsing quietly. A
//! correction that stops applying is the loudest thing a vendor revision can
//! do, and this is where it is heard. The failure names the layer it is in.

mod names;
mod ops;
mod types;

use std::io;
use std::path::{Path, PathBuf};

use syn::visit_mut::VisitMut;
use thiserror::Error;

use crate::model::DocumentError;
use crate::overlay::OverlayError;
use crate::{ConfirmationError, Document, LoadError, Loading, overlay};

/// The command a generated file tells its reader to run.
///
/// `cargo run -p xtask -- bless` is the convention this crate documents; an
/// adoption that spells it differently says so with
/// [`Settings::regenerated_by`], because the line is a promise to whoever opens
/// the file next.
const DEFAULT_COMMAND: &str = "cargo run -p xtask -- bless";

/// The one exemption generated source gets from an adopter's lints.
///
/// It is an inner attribute rather than a wrapping module so that the exemption
/// travels with the file it exempts, and it is the same block in every
/// generated file so that an adopter can grep for it.
const ALLOW: &str = "\
#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    missing_debug_implementations,
    unreachable_pub,
    unused,
    rustdoc::all,
    reason = \"generated source is not graded on style; the allow covers this \\
              module and nothing else\"
)]
";

/// What a bless step generates, and the things only the adopter can say.
///
/// The vendor's document and the adopter's Overlays are the input and a
/// directory is the output; everything between them is derived. What is left
/// over are the facts no document carries: which Rust types the adopter already
/// owns for which vendor formats, what command regenerates the result, and
/// whether a summary page is wanted and where it goes.
#[derive(Debug, Clone)]
pub struct Settings {
    document: PathBuf,
    overlays: Vec<PathBuf>,
    replacements: Vec<(String, String)>,
    command: String,
    summary: Option<PathBuf>,
    loading: Loading,
}

impl Settings {
    /// Generate from the vendor's document, uncorrected.
    ///
    /// A path rather than contents: the generated files name every document
    /// they came from so that a reader can find them, and the corrected
    /// document is written under the vendor document's own name.
    ///
    /// Corrections are layers over it — [`Settings::overlay`], once per layer.
    #[must_use]
    pub fn new(document: impl Into<PathBuf>) -> Self {
        Self {
            document: document.into(),
            overlays: Vec::new(),
            replacements: Vec::new(),
            command: DEFAULT_COMMAND.to_owned(),
            summary: None,
            loading: Loading::new(),
        }
    }

    /// Lay one Overlay over the document, after every Overlay already named.
    ///
    /// Call it once per layer. The order of the calls is the order the layers
    /// are applied, because a later layer corrects the document the earlier
    /// ones produced — so two layers that touch the same node are not
    /// interchangeable, and the last one wins.
    ///
    /// A layer that fails names itself, which is the practical reason to have
    /// more than one: a tripwire that stops the bless says which file to open.
    #[must_use]
    pub fn overlay(mut self, overlay: impl Into<PathBuf>) -> Self {
        self.overlays.push(overlay.into());
        self
    }

    /// Confirm writes with this word rather than with `commit`.
    ///
    /// The confirmation is this crate's word and not the vendor's, so a
    /// document whose own schema declares a property called `commit` is not
    /// wrong — the two simply cannot both have `--commit`, and a generation
    /// that found them fighting refuses rather than renaming the vendor's
    /// property behind its author's back. This is the way out on the side that
    /// owns the word; renaming a gate in `x-cli-gates` is the way out on the
    /// other.
    ///
    /// The word is held to the spelling rule every flag is held to, and the
    /// refusal comes back here rather than at generation time:
    ///
    /// ```
    /// # use typed_openapi::generate::Settings;
    /// let settings = Settings::new("toy.yaml").commit_word("yes")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn commit_word(mut self, word: &str) -> Result<Self, ConfirmationError> {
        self.loading = self.loading.commit(word)?;
        Ok(self)
    }

    /// Emit `rust_type` wherever the document declares `format`.
    ///
    /// The adopter owns a Rust type for a vendor format — an amount of money, a
    /// customer number, a posting key — and wants it in the generated structs
    /// rather than the `String` the document would otherwise produce.
    ///
    /// Keying on the format rather than on a schema name is what keeps the
    /// substitution honest: the shape being replaced is read out of the
    /// document, so the rule the CLI validates against and the rule the Rust
    /// type stands for are the same bytes. `rust_type` is written into the
    /// generated source verbatim, so it is a path the generated crate can name.
    ///
    /// A named schema carrying the format becomes a `#[serde(transparent)]`
    /// newtype *over* `rust_type`, whatever the schema is called, and that
    /// wrapper's impls are written in terms of it: `Display` forwards to it,
    /// `FromStr` parses into it and names `<rust_type as FromStr>::Err` as its
    /// own error. The generated types assert both traits against `rust_type`,
    /// so a missing one is a single named error rather than the wrapper's own
    /// impls failing. Where the format is declared without a name — on a
    /// property, on a list's items — the type stands alone and needs only the
    /// `Serialize`, `Deserialize`, `Clone`, `Debug` and `PartialEq` every
    /// generated type has. `docs/generating.md` says why the document's
    /// `pattern` and the type's own reading are two rules that a test has to
    /// hold together.
    #[must_use]
    pub fn replace(mut self, format: impl Into<String>, rust_type: impl Into<String>) -> Self {
        self.replacements.push((format.into(), rust_type.into()));
        self
    }

    /// Name the command that regenerates, for the header of every written file.
    ///
    /// The default is `cargo run -p xtask -- bless`.
    #[must_use]
    pub fn regenerated_by(mut self, command: impl Into<String>) -> Self {
        self.command = command.into();
        self
    }

    /// Render the summary page too, at `path`.
    ///
    /// The page is [`Summary`](crate::Summary) as Markdown — the tallies, the
    /// groups by name, a row per named gate, a row per parameter carried
    /// without a flag — under a header naming the command that counts it again
    /// and every document it was counted from. An adoption that states a count
    /// in a doc comment, a README or a reference page quotes the page and holds
    /// the prose to [`Document::summary`] in a test, so the number is a
    /// measurement rather than a memory.
    ///
    /// Asking is the whole of it: an adoption that quotes no count names no
    /// path, and the bless step writes no file. The count is available either
    /// way — `Document::summary` is derived from the reduction, so a shipped
    /// binary that loaded a blob answers it with no page, no document and no
    /// feature.
    ///
    /// **The path is the adopter's, because nothing the generator writes reads
    /// this page.** The four artefacts embed each other by relative path, which
    /// is why [`Settings::write_to`] owns where *they* go; a summary page is
    /// reached only by whatever the adoption points at it — an `include_str!`
    /// in a hand-written `lib.rs`, a `docs/` page, a link out of a README — and
    /// only the adopter knows which. So `src/summary.md` beside the blob and
    /// `../docs/api-summary.md` two directories up are equally sensible, and
    /// naming one would have been this crate choosing the shape of a tree it
    /// does not own. A relative path lands under `crate_dir`; an absolute one
    /// lands where it says.
    #[must_use]
    pub fn summary_page(mut self, path: impl Into<PathBuf>) -> Self {
        self.summary = Some(path.into());
        self
    }

    /// Write the artefacts under `crate_dir`, and answer with their paths.
    ///
    /// The four the generated crate is built from, always; the summary page
    /// after them where [`Settings::summary_page`] asked for one. The answer is
    /// what was written, so a caller who asked for no page is handed no page.
    ///
    /// The sink is a directory rather than four values the caller places,
    /// because the layout is not the caller's to choose: the generated crate
    /// embeds the corrected document and the reduced model by relative path,
    /// and the header of each Rust file states where the others are. One
    /// argument buys all four in the arrangement they have to be in.
    ///
    /// Every Rust file is handed to `rustfmt` after it is written, so what
    /// lands in the tree is what `cargo fmt --check` expects and the bless step
    /// stays one command.
    pub fn write_to(&self, crate_dir: impl AsRef<Path>) -> Result<Vec<PathBuf>, GenerateError> {
        let dir = crate_dir.as_ref();
        let corrected = self.correct()?;

        let spec = dir.join("spec").join(self.corrected_name());
        let types = dir.join("src/types.rs");
        let ops = dir.join("src/ops.rs");
        let model = dir.join("src/model.postcard");
        // A relative path lands under the directory the rest do; an absolute
        // one lands where it says, which is `Path::join`'s own rule.
        let page = self.summary.as_ref().map(|path| dir.join(path));

        let header = self.rust_header(&spec);
        let document = format!("{}{}", self.document_header(), corrected.yaml);

        // The types are emitted first because the wrappers name them, and
        // `names` is how they are named: `ops` looks a schema up in what
        // `types` wrote rather than deriving a spelling of its own.
        let (source, names) = types::emit(
            &corrected.api,
            &corrected.model,
            &header,
            &self.replacements,
        )?;

        write_bytes(&spec, document.as_bytes())?;
        write_rust(&types, &source)?;
        write_rust(
            &ops,
            &ops::emit(&corrected.api, &corrected.model, &header, &names)?,
        )?;
        write_model(&model, &corrected.model)?;

        let mut written = vec![spec, types, ops, model];
        if let Some(page) = page {
            let counted = format!("{}{}", self.summary_header(), corrected.model.summary());
            write_bytes(&page, counted.as_bytes())?;
            written.push(page);
        }
        Ok(written)
    }

    /// Every layer, laid over the document in order, in the three views the
    /// artefacts need.
    fn correct(&self) -> Result<Corrected, GenerateError> {
        let fault = |path: &Path| {
            let path = path.to_path_buf();
            move |source| GenerateError::Overlay { path, source }
        };

        let mut overlaid = overlay::parse(&read(&self.document)?).map_err(fault(&self.document))?;
        for layer in &self.overlays {
            overlaid = overlay::apply(overlaid, &read(layer)?).map_err(fault(layer))?;
        }
        let yaml = serde_yaml_ng::to_string(&overlaid).map_err(GenerateError::Yaml)?;

        // Refuse to emit against a document this crate cannot build a CLI from.
        // Everything below trusts that this succeeded.
        let model =
            Document::load_with(&yaml, &[], &self.loading).map_err(GenerateError::Unusable)?;
        let api = serde_json::from_value(overlaid).map_err(GenerateError::NotOpenApi)?;

        Ok(Corrected { yaml, model, api })
    }

    /// The corrected document's file name: the vendor's, with `overlaid` in it.
    fn corrected_name(&self) -> String {
        let stem = self
            .document
            .file_stem()
            .unwrap_or(self.document.as_os_str())
            .to_string_lossy();
        format!("{stem}.overlaid.yaml")
    }

    /// What the corrected document says above its first line: the command
    /// that rewrites it, and every document it was built from in the order
    /// they were applied — which is what a reader needs to reproduce it.
    fn document_header(&self) -> String {
        format!(
            "# Generated by `{}` from {}.\n\
             # Do not edit: every correction belongs in an Overlay.\n",
            self.command,
            listed(&self.inputs(file_name)),
        )
    }

    /// What the summary page says above its first line: the command that
    /// measures it again, and every document it was measured from in the order
    /// they were applied.
    ///
    /// This is the difference between a measurement and a claim. A page of bare
    /// numbers is indistinguishable from a page of remembered ones — and a
    /// reader who cannot tell them apart will either trust a stale number or
    /// "correct" a true one. Naming what was counted and from what leaves the
    /// reader one command away from taking the measurement themselves, which is
    /// the same promise [`Settings::regenerated_by`] makes in every other file
    /// this step writes.
    ///
    /// An HTML comment, because the page is Markdown: the header is for whoever
    /// opens the file, and a rendered view of it shows the numbers alone.
    fn summary_header(&self) -> String {
        format!(
            "<!--\n\
             Generated by `{}` from {}.\n\
             Do not edit: every number below is counted off that document's \
             reduction, and\n\
             that command counts them again.\n\
             -->\n\n",
            self.command,
            listed(&self.inputs(file_name)),
        )
    }

    /// What every generated Rust file says above its first line: how to rewrite
    /// it, where a correction belongs instead, and the one exemption generated
    /// source gets from an adopter's lints.
    fn rust_header(&self, corrected: &Path) -> String {
        let belongs = match self.overlays.as_slice() {
            [] => format!("is generated from `{}`", locator(&self.document)),
            layers => {
                let named: Vec<String> = layers
                    .iter()
                    .map(|path| format!("`{}`", locator(path)))
                    .collect();
                format!("belongs in {}", listed(&named))
            }
        };
        format!(
            "//! Generated by `{}` from `{}`.\n\
             //! Do not edit: every correction {belongs}.\n\
             {ALLOW}",
            self.command,
            locator(corrected),
        )
    }

    /// The vendor's document and every layer over it, in the order they are
    /// applied, named the way `name` names a path.
    fn inputs(&self, name: impl Fn(&Path) -> String) -> Vec<String> {
        std::iter::once(&self.document)
            .chain(&self.overlays)
            .map(|path| name(path))
            .collect()
    }
}

/// A list as prose: `a`, then `a and b`, then `a, b and c`.
fn listed(items: &[String]) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// One Overlay application, in the three views every artefact is emitted from.
///
/// Producing all three from one application is the whole point: the YAML that
/// is committed, the reduction a binary reads and counts itself off, and the
/// object model the Rust is emitted from are the same correction, so no two
/// artefacts can describe a different API.
struct Corrected {
    /// The corrected document, as it is committed.
    yaml: String,
    /// That document reduced to the facts a command line needs.
    model: Document,
    /// That document as the OpenAPI object model the emitters read.
    api: openapiv3::OpenAPI,
}

/// The shortest form of a path a reader can act on: the directory holding it
/// and its name.
///
/// A bare file name is ambiguous once an adoption has more than one `spec`
/// directory, and an absolute path is true only on the machine that generated.
fn locator(path: &Path) -> String {
    match path.parent().and_then(Path::file_name) {
        Some(parent) => format!("{}/{}", parent.to_string_lossy(), file_name(path)),
        None => file_name(path),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn read(path: &Path) -> Result<String, GenerateError> {
    std::fs::read_to_string(path).map_err(|source| GenerateError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// A vendor's description, kept where rustdoc will not run it.
///
/// Every description in the document becomes a doc comment, and rustdoc
/// compiles and executes the code blocks in a doc comment. Markdown makes a
/// code block out of three ordinary shapes of prose: a run of lines indented
/// four spaces or more, a list item whose content sits five columns from its
/// marker, and a fence that names no language. A vendor writing a nested list,
/// a column of items lined up under the widest marker, or a hanging example
/// writes all three without meaning any of them, and what the adopter gets is
/// `cargo test --doc` failing on the vendor's sentences — which no lint
/// allowance reaches, because a doctest is executed rather than linted.
///
/// So each line is made unrunnable where it would otherwise be run, and left
/// alone everywhere else. The generator is what put the prose where rustdoc
/// would execute it, so the generator is where it is made safe; an adopter
/// switching doctests off for the whole crate would be hiding this and taking
/// their own hand-written files with it.
///
/// # What may be edited, and what may not
///
/// **The generated page has to render what the vendor wrote.** Every edit here
/// is chosen against that: it changes bytes the vendor has no stake in, so that
/// the rendering is the one they meant. Capping an indentation keeps a nested
/// list nested instead of letting it become a code block; pulling a hanging
/// item's content back keeps it an item; writing a bullet as `-` keeps the
/// bullet, which rustc would otherwise eat. What none of them may do is change
/// what the page says — the inside of a fence is the vendor's sample and is
/// passed through untouched, and a fence's language is renamed rather than
/// dropped, because the word is what a reader's highlighting goes by.
struct Prose;

impl VisitMut for Prose {
    fn visit_attribute_mut(&mut self, attr: &mut syn::Attribute) {
        let syn::Meta::NameValue(pair) = &mut attr.meta else {
            return;
        };
        if !pair.path.is_ident("doc") {
            return;
        }
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(text),
            ..
        }) = &mut pair.value
        else {
            return;
        };
        *text = syn::LitStr::new(&unrunnable(&text.value()), text.span());
    }
}

/// How wide Markdown counts a tab when it measures indentation.
const TAB: usize = 4;

/// The deepest indentation that cannot open a code block.
///
/// Four opens one, so three is what is left. It is enough to keep a nested list
/// nested — a list marker needs only to reach its parent's content column — so
/// what capping costs is the depth of an unusually deep one, and what it buys
/// is a crate whose tests run.
const KEEP: usize = 3;

/// The widest gap between a list marker and its content that cannot open a
/// code block.
///
/// Markdown measures a list item's content from the end of its marker rather
/// than from the start of the line, so this is one wider than [`KEEP`]: five
/// columns after a marker open a code block inside the item, four do not.
/// Measured on `-`, `+`, `*` and an ordered marker alike.
///
/// Four is safe only where the marker survives to hold the gap off the margin,
/// which is [`dashed`]'s doing; [`unhung`] says what happens where it does
/// not.
const HANG: usize = 4;

/// The bullet a list marker is written with.
///
/// `-` rather than `*`, and [`dashed`] says why.
const BULLET: char = '-';

/// The most digits Markdown reads as one ordered list marker.
const ORDERED: usize = 9;

/// `prose` with nothing in it that rustdoc would compile.
fn unrunnable(prose: &str) -> String {
    let capped = capped_lines(prose);
    if !capped.contains('\n') {
        // One line is one `///`, where nothing is indented and there is
        // nothing to strip.
        return capped;
    }
    // More than one line is a `/* */` comment, and `rustfmt` indents its body
    // to the item it sits on — every line but the first, which stays flush
    // against the opening `/*`. rustc strips the indentation a comment's lines
    // share, and a flush first line holds that strip down to almost nothing, so
    // the rest of the body arrives still carrying the column the item sits at:
    // a paragraph the vendor indented three spaces lands six in, which is a
    // code block. Opening on a blank line puts the first line inside the
    // indented body with the others, where the strip reaches it, and that same
    // paragraph lands back on its three.
    format!("\n{capped}\n")
}

/// `prose` with no line indented deeply enough to open a code block, no list
/// item hanging its content far enough to open one, no bullet rustc will eat,
/// and no fence rustdoc would read as Rust.
///
/// A line is capped, then unhung, then dashed, then read as a fence, and the
/// first step's place in that order is the rule: Markdown reads a fence at
/// three columns or fewer, so an indented one is not a fence at all but the
/// start of an indented code block — capping it first is what turns it into the
/// fence the vendor meant, rather than leaving a block whose first line happens
/// to be three backticks. The two list steps cannot disturb that, because a run
/// of backticks is not a list marker.
fn capped_lines(prose: &str) -> String {
    let mut fenced = false;
    let lines: Vec<String> = prose
        .split('\n')
        .map(|line| {
            // Inside a fence the content is the vendor's sample, kept as they
            // wrote it — the fence above it already says nobody will run it.
            if fenced && fence(line).is_none() {
                return line.to_owned();
            }
            let tamed = dashed(&unhung(&capped(line)));
            let Some((marker, language)) = fence(&tamed) else {
                return tamed;
            };
            if fenced {
                fenced = false;
                return tamed;
            }
            fenced = true;
            if compiled(language) {
                let indent: String = tamed.chars().take_while(|c| c.is_whitespace()).collect();
                format!("{indent}{marker}{INERT}")
            } else {
                tamed
            }
        })
        .collect();
    lines.join("\n")
}

/// The language named on a fence rustdoc will not compile.
///
/// Any word it does not recognise does, and this one says what the block is.
const INERT: &str = "text";

/// The words rustdoc reads above a code block as *attributes of Rust* rather
/// than as the name of a language.
///
/// A fence carrying one of them — or carrying nothing — is a block rustdoc
/// compiles and runs, so a vendor who wrote `rust` over a line of pseudocode
/// has written a doctest without meaning to. Every other word names a language
/// rustdoc leaves alone, and the vendor's own is worth keeping: it is what a
/// reader's syntax highlighting goes by.
const COMPILED: [&str; 7] = [
    "compile_fail",
    "ignore",
    "no_run",
    "rust",
    "should_panic",
    "standalone_crate",
    "test_harness",
];

/// Would rustdoc compile a block a fence naming `language` opens?
fn compiled(language: &str) -> bool {
    let word = language.split([',', ' ', '\t']).next().unwrap_or(language);
    word.is_empty() || word.starts_with("edition") || COMPILED.contains(&word)
}

/// The marker this line fences with and the language it names, if it is a
/// fence.
///
/// A fence is three or more backticks or tildes. Whether it is indented too
/// far to be one is settled before this is asked, by capping the line.
fn fence(line: &str) -> Option<(&str, &str)> {
    let body = line.trim_start();
    let mark = ['`', '~']
        .into_iter()
        .find(|mark| body.chars().take(3).filter(|c| c == mark).count() == 3)?;
    let run = body.len() - body.trim_start_matches(mark).len();
    let (marker, language) = body.split_at(run);
    Some((marker, language.trim()))
}

/// `line` with its indentation capped at [`KEEP`].
fn capped(line: &str) -> String {
    let content = line.trim_start();
    if content.is_empty() || columns(line) <= KEEP {
        return line.to_owned();
    }
    format!("{}{content}", " ".repeat(KEEP))
}

/// How many columns the whitespace at the start of `text` is worth.
///
/// Markdown counts a tab as [`TAB`], and both caps are stated in columns, so
/// this is the one place that reading is made.
fn columns(text: &str) -> usize {
    text.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { TAB } else { 1 })
        .sum()
}

/// `line` with the gap between a list marker and its content capped at
/// [`HANG`].
///
/// This is the code block [`capped`] cannot see. A vendor lining the text of
/// several items up under the widest marker writes five spaces after a short
/// one, and Markdown reads content that far from a marker as an indented code
/// block *inside* the item — while the line's own indentation, which is all
/// [`capped`] measures, is nothing at all.
///
/// The cap holds at four only because [`dashed`] has taken `*` off the front of
/// every bullet. Where a `*` survives, rustc eats it out of a `/* */` comment
/// and leaves the gap standing alone as ordinary indentation, and four columns
/// of that under a blank line opens a block of its own. The two rules are one
/// rule, and `PROSE`'s list set off by a blank line is what holds them
/// together.
///
/// The line is left alone when the marker carries no content: a marker on its
/// own opens nothing, and there is no gap to measure.
fn unhung(line: &str) -> String {
    let content = line.trim_start();
    let Some(width) = list_marker(content) else {
        return line.to_owned();
    };
    let (indent, rest) = line.split_at(line.len() - content.len());
    let (marker, after) = rest.split_at(width);
    let text = after.trim_start();
    if text.is_empty() || columns(after) <= HANG {
        return line.to_owned();
    }
    format!("{indent}{marker}{}{text}", " ".repeat(HANG))
}

/// `line` with a `*` bullet written as [`BULLET`].
///
/// rustc rebuilds a `/* */` comment by stripping a leading `*` off every line
/// whenever all of the lines it weighs carry one at the same column, which is
/// the shape a vendor's bullet list takes exactly. What reaches the reader is
/// then a run of sentences where the vendor wrote a list, and nothing on the
/// page says a marker went missing.
///
/// Markdown draws the same bullet for `-` and `*`, so this is one character the
/// vendor has no stake in and the rendering is the one they wrote. `+` and the
/// ordered markers are left alone: rustc eats none of them.
fn dashed(line: &str) -> String {
    let content = line.trim_start();
    if !content.starts_with('*') || list_marker(content).is_none() {
        return line.to_owned();
    }
    let (indent, rest) = line.split_at(line.len() - content.len());
    let (_, after) = rest.split_at(1);
    format!("{indent}{BULLET}{after}")
}

/// How wide the list marker at the start of `content` is, if it is one.
///
/// Markdown's markers are `-`, `+` or `*`, and up to [`ORDERED`] digits
/// followed by `.` or `)`. Whitespace after it is what makes it a marker at
/// all, which is also what keeps `*emphasis*` at the start of a line out of
/// this.
fn list_marker(content: &str) -> Option<usize> {
    let width = if content.starts_with(['-', '+', '*']) {
        1
    } else {
        let digits = content.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || digits > ORDERED {
            return None;
        }
        if !content.get(digits..)?.starts_with(['.', ')']) {
            return None;
        }
        digits + 1
    };
    content
        .get(width..)?
        .starts_with([' ', '\t'])
        .then_some(width)
}

/// Write a file, creating the directory it goes in if it is missing.
fn write_bytes(path: &Path, contents: &[u8]) -> Result<(), GenerateError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| GenerateError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(path, contents).map_err(|source| GenerateError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Write the reduction a binary loads, after reading it straight back.
///
/// `model` is the one [`Document::load`] a bless step makes — the same value
/// the generated types and wrappers were emitted from — so encoding it is the
/// only way the blob and the document can come apart. The read-back proves they
/// have not: what a binary will deserialise is what this step reduced.
fn write_model(path: &Path, model: &Document) -> Result<(), GenerateError> {
    let blob = model.to_blob().map_err(GenerateError::Blob)?;
    let read_back = Document::from_blob(&blob).map_err(GenerateError::Blob)?;
    if &read_back != model {
        return Err(GenerateError::RoundTrip);
    }
    write_bytes(path, &blob)
}

fn write_rust(path: &Path, source: &str) -> Result<(), GenerateError> {
    write_bytes(path, source.as_bytes())?;
    rustfmt(path)
}

/// Hand a written file to `rustfmt`.
///
/// Formatting the file on disk rather than piping the source through keeps the
/// result identical to what an adopter's own `cargo fmt` would produce, which
/// is the only reason a generated file can sit under `cargo fmt --check` at
/// all.
fn rustfmt(path: &Path) -> Result<(), GenerateError> {
    let status = std::process::Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .arg(path)
        .status()
        .map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                GenerateError::RustfmtMissing
            } else {
                GenerateError::RustfmtSpawn {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(GenerateError::RustfmtFailed {
            path: path.to_path_buf(),
        })
    }
}

/// Why a bless step stopped.
///
/// Most variants are the document saying something this generator has no Rust
/// spelling for; the rest are the two documents, the filesystem, or `rustfmt`.
/// A bless step reports and exits, so nothing here is meant to be branched on —
/// it is meant to name the thing to go and fix.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GenerateError {
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A document or a layer over it that does not read, or does not apply.
    /// `path` is the file to go and open: with corrections split across
    /// layers, which one stopped the bless is the first thing to know.
    #[error("{path}: {source}")]
    Overlay {
        path: PathBuf,
        #[source]
        source: OverlayError,
    },
    #[error("the overlaid document is not representable as YAML: {0}")]
    Yaml(#[source] serde_yaml_ng::Error),
    #[error("the overlaid document does not describe a usable CLI: {0}")]
    Unusable(#[source] LoadError),
    #[error("the overlaid document is not an OpenAPI 3 document: {0}")]
    NotOpenApi(#[source] serde_json::Error),
    #[error("schema `{name}` is not representable as JSON: {source}")]
    Schema {
        name: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("typify cannot build Rust types from the document's schemas: {0}")]
    Typify(#[source] typify::Error),
    /// A wrapper would have to name the type of a schema the document's
    /// `components.schemas` never declared. Emitting an identifier for it
    /// anyway is how an adopter ends up bisecting a generated file, so the
    /// reference is named here instead.
    #[error(
        "`#/components/schemas/{schema}` is referenced but not declared, so no \
         wrapper can name the type it would be"
    )]
    NoType { schema: String },
    /// Two of the document's shapes reduce to one Rust type. typify writes a
    /// definition per shape and uniquifies nothing, so emitting them is a file
    /// that defines the same type twice; renaming one here would be a generator
    /// choosing a public name nobody asked for.
    ///
    /// `first` and `second` are sentences naming where each shape stands — a
    /// schema the document declares, or a body or response an operation states
    /// inline — because the two are refused together and a reader needs to know
    /// which is which.
    #[error("{first} and {second} are both `{rust}` in Rust; rename one of them in an Overlay")]
    OneType {
        first: String,
        second: String,
        rust: String,
    },
    /// A schema whose Rust name is one this crate declares a replaced type
    /// under. Every other named schema becomes a type of its own, so this one
    /// would silently become whatever [`Settings::replace`] substitutes —
    /// wherever the document uses it, and with no type of its own left to
    /// carry what it declares.
    #[error(
        "the schema `{schema}` reduces to a Rust name this crate declares a \
         type `Settings::replace` substitutes under, so it generates no type \
         of its own; rename it in an Overlay"
    )]
    Reserved { schema: String },
    #[error("{0}")]
    Unsupported(String),
    /// A failure while emitting one operation's wrapper, named by the
    /// operation it came from. Everything else this crate refuses says which
    /// operation it is about, and a generated file is too large to bisect by
    /// hand for one that does not.
    #[error("{op}: {source}")]
    Operation {
        op: String,
        #[source]
        source: Box<GenerateError>,
    },
    #[error("the generated {file} is not valid Rust: {source}")]
    NotRust {
        file: &'static str,
        #[source]
        source: syn::Error,
    },
    #[error("the reduced model does not survive a round trip: {0}")]
    Blob(#[source] DocumentError),
    #[error("the reduced model is not the document's reduction after a round trip")]
    RoundTrip,
    #[error("rustfmt is not on PATH, and a bless step formats every Rust file it writes")]
    RustfmtMissing,
    #[error("running rustfmt on {path}: {source}")]
    RustfmtSpawn {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("rustfmt rejected the generated {path}")]
    RustfmtFailed { path: PathBuf },
}
