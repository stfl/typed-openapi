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
mod prose;
mod types;

use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::DocumentError;
use crate::openapi::Misfit;
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
        // The vendor's version, before the first layer, as `Document::load_with`
        // asks it: this road lays the layers itself, and a layer that does not
        // apply to a 3.1 document is a refusal for the document, not for the
        // layer.
        crate::openapi::require_3_0(&overlaid).map_err(GenerateError::Unusable)?;
        for layer in &self.overlays {
            overlaid = overlay::apply(overlaid, &read(layer)?).map_err(fault(layer))?;
        }
        // The emitter keeps its defaults, which quote every string a YAML 1.1
        // reader would misread (`"N"`, `"yes"`, `"1.0"`), so a reader that
        // follows 1.1 reads back the strings that were written.
        let yaml = serde_saphyr::to_string(&overlaid)
            .map_err(|error| GenerateError::Yaml(Box::new(error)))?;

        // Refuse to emit against a document this crate cannot build a CLI from.
        // Everything below trusts that this succeeded.
        let model =
            Document::load_with(&yaml, &[], &self.loading).map_err(GenerateError::Unusable)?;
        let api = crate::openapi::model_of(&overlaid)
            .map_err(|Misfit { at, source }| GenerateError::NotOpenApi { at, source })?;

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

/// What [`GenerateError::OneType`] asks an adopter to change.
///
/// A `title` on an inline schema gives its type a name of the document's own, so
/// it is the repair wherever one of the two is inline. Two schemas the document
/// declares have names the vendor wrote, and one of them is renamed.
fn repair(inline: bool) -> &'static str {
    if inline {
        "give an inline schema a `title` in an Overlay"
    } else {
        "rename one of them in an Overlay"
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
    /// The document or an Overlay file could not be read.
    #[error("cannot read {path}")]
    Read {
        /// The path as the adopter gave it to [`Settings::new`] or
        /// [`Settings::overlay`].
        path: PathBuf,
        /// The error the operating system returned.
        #[source]
        source: io::Error,
    },
    /// A generated file, or a directory it goes in, could not be written.
    #[error("cannot write {path}")]
    Write {
        /// The file the generator was writing, or the directory above it that
        /// it was making: under the directory given to [`Settings::write_to`],
        /// or where [`Settings::summary_page`] points.
        path: PathBuf,
        /// The error the operating system returned.
        #[source]
        source: io::Error,
    },
    /// A document or a layer over it that does not read, does not apply, or
    /// changes the `openapi` of the document. `path` is the file to go and
    /// open: with corrections split across layers, which one stopped the bless
    /// is the first thing to know.
    #[error("cannot use {path}")]
    Overlay {
        /// The path as the adopter gave it to [`Settings::new`] or
        /// [`Settings::overlay`].
        path: PathBuf,
        /// What stopped the file: its syntax, its shape, an action that matched
        /// nothing, or a layer that changed the `openapi` of the document.
        #[source]
        source: OverlayError,
    },
    /// The YAML emitter refused the corrected document. The cause is boxed so
    /// that no type of the emitter is part of this crate's API.
    #[error("the overlaid document is not representable as YAML")]
    Yaml(#[source] Box<dyn Error + Send + Sync>),
    /// The document could not be reduced to a command tree. The cause is the
    /// refusal that [`Document::load_with`] gives for it. A version of OpenAPI
    /// that this crate does not read is refused for the vendor's document before
    /// the first layer is laid, and every other cause is a refusal of the
    /// document with its layers laid.
    #[error("the document does not describe a usable CLI")]
    Unusable(#[source] LoadError),
    /// The corrected document is not an OpenAPI 3 document, so the generator
    /// has no object model of it.
    #[error("the overlaid document is not an OpenAPI 3 document: refused at `{at}`")]
    NotOpenApi {
        /// The first node that the OpenAPI model refuses, from the root of the
        /// document: `info.title`, `components.schemas.Voucher`, a path item as
        /// `paths[/vouchers]`, or a node inside a path item as
        /// `paths[/vouchers].get.deprecated`. `.` is the document itself.
        at: String,
        /// `serde_json`'s message.
        #[source]
        source: serde_json::Error,
    },
    /// A schema that cannot be written as the JSON typify takes in.
    #[error("schema `{name}` is not representable as JSON")]
    Schema {
        /// The schema's name in `components.schemas`, or for an inline schema a
        /// sentence naming the body or response that states it.
        name: String,
        /// `serde_json`'s reason.
        #[source]
        source: serde_json::Error,
    },
    /// typify refused a schema. The cause is boxed so that no type of typify
    /// is part of this crate's API.
    #[error("typify cannot build Rust types from the document's schemas")]
    Typify(#[source] Box<dyn Error + Send + Sync>),
    /// A wrapper would have to name the type of a schema the document's
    /// `components.schemas` never declared. Emitting an identifier for it
    /// anyway is how an adopter ends up bisecting a generated file, so the
    /// reference is named here instead.
    #[error(
        "`#/components/schemas/{schema}` is referenced but not declared, so no \
         wrapper can name the type it would be"
    )]
    NoType {
        /// The name the reference ends in, as the document spells it.
        schema: String,
    },
    /// Two of the document's shapes reduce to one Rust type. typify writes a
    /// definition per shape and uniquifies nothing, so emitting them is a file
    /// that defines the same type twice; renaming one here would be a generator
    /// choosing a public name nobody asked for.
    ///
    /// `first` and `second` are sentences naming where each shape stands — a
    /// schema the document declares, or a body or response an operation states
    /// inline — because the two are refused together and a reader needs to know
    /// which is which.
    ///
    /// The cheap repair is a `title` on an inline schema, which typify names the
    /// type after, so the message names it where one of the two is inline and
    /// asks for a rename of a schema the document declares where neither is.
    #[error(
        "{first} and {second} are both `{rust}` in Rust; {}",
        repair(*.inline)
    )]
    OneType {
        /// The sentence naming the shape that claimed the Rust name first.
        first: String,
        /// The sentence naming the shape that claimed it second.
        second: String,
        /// The Rust name both reduce to, in typify's spelling.
        rust: String,
        /// Whether either of the two is stated inline by an operation, and not
        /// declared under `components.schemas`.
        inline: bool,
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
    Reserved {
        /// The schema's name in `components.schemas`, as the document spells it.
        schema: String,
    },
    /// A document element this generator has no Rust spelling for, or a Rust
    /// type the adopter named that does not parse. The text names the element
    /// and says why.
    #[error("{0}")]
    Unsupported(String),
    /// A failure while emitting one operation's wrapper, named by the
    /// operation it came from. Everything else this crate refuses says which
    /// operation it is about, and a generated file is too large to bisect by
    /// hand for one that does not.
    #[error("the wrapper for {op} cannot be generated")]
    Operation {
        /// The `operationId` of the operation whose wrapper was being emitted.
        op: String,
        /// The refusal itself.
        #[source]
        source: Box<GenerateError>,
    },
    /// Tokens the generator wrote do not parse as Rust. This is a defect of the
    /// generator.
    #[error("the generated {file} is not valid Rust")]
    NotRust {
        /// The name of the generated file the tokens were bound for.
        file: &'static str,
        /// The parser's error.
        #[source]
        source: syn::Error,
    },
    /// The reduced model could not be encoded to the blob, or the blob could
    /// not be read back.
    #[error("the reduced model does not survive a round trip")]
    Blob(#[source] DocumentError),
    /// The blob was read back and differs from the reduction it was written
    /// from, so a binary that loads it would not hold the document the bless
    /// step reduced.
    #[error("the reduced model is not the document's reduction after a round trip")]
    RoundTrip,
    /// No `rustfmt` executable was found on `PATH`.
    #[error("rustfmt is not on PATH, and a bless step formats every Rust file it writes")]
    RustfmtMissing,
    /// `rustfmt` was found and could not be started.
    #[error("cannot run rustfmt on {path}")]
    RustfmtSpawn {
        /// The generated file `rustfmt` was to format.
        path: PathBuf,
        /// The error the operating system returned.
        #[source]
        source: io::Error,
    },
    /// `rustfmt` ran and exited with a failure.
    #[error("rustfmt rejected the generated {path}")]
    RustfmtFailed {
        /// The generated file `rustfmt` was to format.
        path: PathBuf,
    },
}
