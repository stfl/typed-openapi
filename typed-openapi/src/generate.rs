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
//! # Using it
//!
//! ```no_run
//! # fn main() -> Result<(), typed_openapi::generate::GenerateError> {
//! typed_openapi::generate::Settings::new("spec/vendor.yaml")
//!     .overlay("spec/corrections.yaml")
//!     .overlay("spec/cli.yaml")
//!     .replace("money", "money::Money")
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

mod ops;
mod types;

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::DocumentError;
use crate::overlay::OverlayError;
use crate::{Document, LoadError, overlay};

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

/// What a bless step generates, and the two things only the adopter can say.
///
/// The vendor's document and the adopter's Overlays are the input and a
/// directory is the output; everything between them is derived. The two
/// settings are the two facts the documents do not carry: which Rust types the
/// adopter already owns for which vendor formats, and what command regenerates
/// the result.
#[derive(Debug, Clone)]
pub struct Settings {
    document: PathBuf,
    overlays: Vec<PathBuf>,
    replacements: Vec<(String, String)>,
    command: String,
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

    /// Write the four artefacts under `crate_dir`, and answer with their paths.
    ///
    /// The sink is a directory rather than four values the caller places,
    /// because the layout is not the caller's to choose: the generated crate
    /// embeds the corrected document and the reduced model by relative path,
    /// and the header of each Rust file states where the others are. One
    /// argument buys all four files in the arrangement they have to be in.
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

        let header = self.rust_header(&spec);
        let document = format!("{}{}", self.document_header(), corrected.yaml);

        // The types are emitted first because the wrappers name them, and
        // `names` is how they are named: `ops` looks a schema up in what
        // `types` wrote rather than deriving a spelling of its own.
        let (source, names) = types::emit(&corrected.api, &header, &self.replacements)?;

        write_bytes(&spec, document.as_bytes())?;
        write_rust(&types, &source)?;
        write_rust(
            &ops,
            &ops::emit(&corrected.api, &corrected.model, &header, &names)?,
        )?;
        write_model(&model, &corrected.model)?;

        Ok(vec![spec, types, ops, model])
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
        let model = Document::load(&yaml, &[]).map_err(GenerateError::Unusable)?;
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

/// One Overlay application, in the three views the four artefacts are emitted
/// from.
///
/// Producing all three from one application is the whole point: the YAML that
/// is committed, the reduction a binary reads and the object model the Rust is
/// emitted from are the same correction, so no two artefacts can describe a
/// different API.
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
    /// A wrapper would have to name a type the generated schemas do not
    /// define. Emitting it anyway is how an adopter ends up bisecting a
    /// generated file, so the schema is named here instead.
    #[error(
        "the document's schema `{schema}` has no generated type: a wrapper \
         would name `{rust}`, which the generated types do not define"
    )]
    NoType { schema: String, rust: String },
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
