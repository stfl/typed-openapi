//! The document, reduced to the facts a CLI and a typed caller both need.
//!
//! What is here is the reduction itself: a list of [`Operation`]s, the values
//! each one carries, and the two names a user types to reach it. Every type on
//! this page is in every build, because the encoding a shipped binary reads,
//! after the stamp a blob opens with, is these types and nothing else — so a
//! caller who matches one writes the same match whatever the feature set.
//!
//! Where they come from is `reduce`, which the `document` feature brings and a
//! shipped binary leaves out. Reducing a document is bless-time work, not
//! startup work: a bless step enables the feature, calls `Document::load` once
//! and [`Document::to_blob`] on the result, and the binary it produces calls
//! [`Document::from_blob`] with no reader compiled into it to read YAML with.
//! The two are the same reduction by construction — there is one `load` — and a
//! test holds the shipped blob to the shipped document to prove the pair was
//! written by the same run.

mod blob;
#[cfg(feature = "document")]
mod reduce;

use std::collections::HashMap;
use std::fmt;

use http::{Method, Uri};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// The reading defines it and this module is where a caller goes looking, so
// `typed_openapi::LoadError` and `typed_openapi::model::LoadError` both name
// the one type wherever the reading itself is written down.
#[cfg(feature = "document")]
pub use self::reduce::{ConfirmationError, LoadError, Loading};
// The generator reads the parameters of an operation as the reduction did.
#[cfg(feature = "generate")]
pub(crate) use self::reduce::{declared, entry_of_flag};
use crate::names::{CommandName, NameError, renamed, spelled};
use crate::request::without_userinfo_and_query;
use crate::scalar::Scalar;

/// Whole-body flag, for every operation that takes JSON.
pub const JSON_BODY: &str = "json-body";
/// The shape of that body, printed instead of sent.
pub const JSON_BODY_TEMPLATE: &str = "json-body-template";
/// Whole-body flag, for a media type this CLI does not assemble.
pub const RAW_BODY: &str = "raw-body";
/// One file part of a multipart body.
pub const FILE_PART: &str = "file";
/// One text part of a multipart body.
pub const FIELD_PART: &str = "field";
/// The write gate: the word a CLI confirms with unless an adopter chooses
/// another one, which `Loading::commit` is how they do (the `document` feature).
pub const COMMIT: &str = "commit";
/// The argument id the confirmation is declared under, whatever word it wears.
///
/// clap wants an id unique within a subcommand, and the word itself is not safe
/// to use as one: an adopter who moves the confirmation to `--yes` frees
/// `commit` for a document property, and that property's flag would then be
/// declared under the same id as the confirmation. The colon keeps this
/// outside `[a-z0-9-]`, which is every spelling a document name can reduce to,
/// so nothing a vendor writes can collide with it.
///
/// A hand-written verb that offers its own confirmation declares it under this
/// id, so that `tree::answers` reads the same flag there as it does on
/// a generated subcommand.
pub const COMMIT_ID: &str = "cli:commit";

/// The six words that no flag of a subcommand may take: the five flags that
/// carry a request's body, which the tree declares itself, and `--help`, which
/// clap declares on every subcommand whether or not anyone asked.
///
/// A parameter or a body field that wants one moves aside for it, and a gate or
/// the confirmation that wants one is refused. Nothing here is a person saying
/// yes, which is why a parameter yields where a gate and the confirmation do
/// not: a `--body-json-body` is ugly and harmless, where a confirmation nobody
/// typed is neither. `--help` is the plainest case, because somebody types it to
/// read, so a word that answers it would be answered by a person who asked for
/// no more than a help page.
///
/// Spending a word here changes the reduced model of any document that declares
/// a field spelled the same way, which is the point: the field moves aside at
/// bless time, where a reviewer sees it, rather than shadowing a flag the CLI
/// needs. The six are written once: the load puts them in every `Namespace`,
/// and [`spends`] asks them of a decoded blob.
pub(crate) const SPENT: [&str; 6] = [
    JSON_BODY,
    JSON_BODY_TEMPLATE,
    RAW_BODY,
    FILE_PART,
    FIELD_PART,
    HELP,
];

/// The word clap spends twice: `--help` on every subcommand, and the `help`
/// subcommand that it adds to every command that has subcommands.
pub(crate) const HELP: &str = "help";

/// Whether a word is one that no flag of a subcommand may take, one of the
/// [`SPENT`] six.
///
/// None of them can move, so a gate or the confirmation is refused when it wants
/// one, and a parameter or a property moves aside. The reduction asks here for
/// the first, a decoded blob is refused for holding any of them, and both ask
/// here.
pub(crate) fn spends(word: &str) -> bool {
    SPENT.contains(&word)
}

/// Whether clap declares a subcommand of this name itself, on every command
/// that has subcommands.
///
/// Nothing can move it, so a group or a command that wears the name is two
/// subcommands with one spelling. The reduction refuses a document that places
/// an operation under one, and a decoded blob is refused for holding one, and
/// both ask here.
pub(crate) fn spends_command(name: &str) -> bool {
    name == HELP
}

/// The gate that stands on a read, if one does.
///
/// A read is sent on sight, so there is nothing for a gate to hold back. The
/// load refuses a document that names a gate on a read, and a decoded blob is
/// refused for holding one, and both ask here.
pub(crate) fn gate_on_read(effect: Effect, gates: &[Gate]) -> Option<&Gate> {
    match (effect, gates.first()) {
        (Effect::Read, gate) => gate,
        (Effect::Write, _) => None,
    }
}

/// The first operation that has a later twin under its `<group> <command>`, and
/// its first twin, in the order of the document.
///
/// The load refuses a document that has two, because one would shadow the other.
/// A decoded blob is refused for holding two, and both ask here. A shipped binary
/// asks at every start-up, and the number of operations is the document's, so one
/// pass answers.
///
/// The first operation with a twin is the first of its name, and its first twin
/// is the second of that name. So the pass remembers where each name first
/// stood, and the pair is the one whose first operation stood earliest among the
/// names that came up a second time.
pub(crate) fn first_collision(ops: &[Operation]) -> Option<(&Operation, &Operation)> {
    let mut first_of = HashMap::with_capacity(ops.len());
    let mut earliest: Option<(usize, usize)> = None;
    for (index, op) in ops.iter().enumerate() {
        let first = *first_of.entry((&op.group, &op.command)).or_insert(index);
        if first != index && earliest.is_none_or(|(held, _)| first < held) {
            earliest = Some((first, index));
        }
    }
    let (first, second) = earliest?;
    Some((ops.get(first)?, ops.get(second)?))
}

/// Every operation the document describes, in document order, plus the server
/// it describes them against.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(with = "uri_string")]
    base: Uri,
    commit: String,
    ops: Vec<Operation>,
}

/// The base is shown without its userinfo and its query. A load refuses a
/// server that holds either, but a blob is read with no load, and a `Debug` goes
/// to logs. Every field is bound by name, so a field added to `Document` stops
/// this from compiling until it is shown or left out on purpose.
impl fmt::Debug for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { base, commit, ops } = self;
        f.debug_struct("Document")
            .field(
                "base",
                &format_args!("{}", without_userinfo_and_query(base)),
            )
            .field("commit", commit)
            .field("ops", ops)
            .finish()
    }
}

/// One operation: one subcommand under one group, one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    id: String,
    group: CommandName,
    command: CommandName,
    commit: String,
    #[serde(with = "method_string")]
    method: Method,
    path: String,
    summary: Option<String>,
    description: Option<String>,
    params: Vec<Param>,
    body: Body,
    effect: Effect,
    gates: Vec<Gate>,
}

/// Whether the CLI must hold this operation behind `--commit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    /// A safe method with no `x-cli-writes` marker: runs on sight.
    Read,
    /// A body-bearing or unsafe method, or a GET the document marks as writing.
    Write,
}

/// One named hazard an operation stands behind, spelled as a long flag.
///
/// `--commit` asks one question — did you mean to write? — and some operations
/// are more than one question: an act that cannot be undone, and an act that
/// reaches a third party, each want their own word rather than a second meaning
/// for that one. A gate is answered *beside* the confirmation, never instead of
/// it, so what a gate adds is always another thing to say and never permission
/// to say less. What the word means is the adopter's business and no business
/// of this crate's, which only carries it.
///
/// A gate travels in the reduced model, so a name comes back off a blob as well
/// as out of a document. Both doors are the same door: `serde` reads it as a
/// `String` and runs it through the spelling rule a command name passes, so a
/// blob cannot smuggle in a flag a document could not have asked for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Gate(String);

impl Gate {
    /// `sendEmail` becomes `send-email`; anything that will not reduce to
    /// `[a-z0-9-]` is rejected rather than mangled, because what is being
    /// spelled is a flag a user has to type.
    pub fn new(origin: &'static str, raw: &str) -> Result<Self, NameError> {
        spelled(origin, raw).map(Self)
    }

    /// The flag name, without the leading `--`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Gate {
    type Error = NameError;

    fn try_from(raw: String) -> Result<Self, NameError> {
        Self::new("reduced model", &raw)
    }
}

impl From<Gate> for String {
    fn from(gate: Gate) -> Self {
        gate.0
    }
}

impl std::fmt::Display for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One place in an operation that carries a value of a kind the document names:
/// a parameter, or one property of a flat JSON body.
///
/// The two halves of an operation take different roads into the request — a
/// parameter is rendered into a path, a query string or a header, a field is a
/// property of the JSON body — so what an adopter does with one is not what
/// they do with the other. Saying which half it is costs the caller one match
/// and is the difference between a guard that reads the value and a guard that
/// hopes the names do not collide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Carrier<'op> {
    /// A path, query or header parameter.
    Param(&'op Param),
    /// One scalar property of a flat JSON body.
    Field(&'op Field),
}

impl<'op> Carrier<'op> {
    /// The wire name, as the document spells it: what a parameter is sent
    /// under, and the property a field is written to in the body.
    ///
    /// The name outlives the carrier, which is a value two words wide and not
    /// worth keeping — so `carrying(..).map(Carrier::name)` is a list of names
    /// rather than a borrow checker's argument.
    #[must_use]
    pub fn name(self) -> &'op str {
        match self {
            Self::Param(param) => param.name(),
            Self::Field(field) => field.name(),
        }
    }
}

/// Where the document puts a parameter: its `in`.
///
/// [`Location`] is where a flag's value goes in the request, and a cookie has no
/// flag, so the fourth place is only here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Place {
    /// `in: path`.
    Path,
    /// `in: query`.
    Query,
    /// `in: header`.
    Header,
    /// `in: cookie`.
    Cookie,
}

impl std::fmt::Display for Place {
    /// The word the document spells `in` with.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(match self {
            Self::Path => "path",
            Self::Query => "query",
            Self::Header => "header",
            Self::Cookie => "cookie",
        })
    }
}

/// Where a parameter goes in the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Location {
    /// A `{name}` placeholder in the path template.
    Path,
    /// A field of the query string.
    Query,
    /// A request header.
    Header,
}

/// One parameter the document declares.
///
/// Everything that only a parameter with a flag has — the flag, where its value
/// goes, what the flag accepts, what kind of value the document calls it —
/// hangs off [`Shape`], because a parameter this CLI cannot spell has none of
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    name: String,
    required: bool,
    shape: Shape,
    description: Option<String>,
    requires: Vec<String>,
}

/// What one parameter is worth on a command line.
///
/// Either the subcommand grows a flag and the request builder knows what to do
/// with its values, or it grows nothing at all. Everything a flag needs lives on
/// the variant that has one, so a parameter this CLI cannot supply cannot leave
/// a flag behind that the request builder would then ignore — which is the shape
/// [`Body`] already has, where one nested property sends a whole body through
/// `--json-body` rather than offering dead per-field flags beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// The flag this parameter grows, where its value goes, and what the flag
    /// accepts. `join` is `None` for a parameter that takes one value, and
    /// `Some` for a list — a flag that may be given again.
    Flag {
        /// The long flag as the user types it, without the leading `--`.
        flag: String,
        /// The part of the request the value is written into.
        location: Location,
        /// The rules the document states about one value, which the flag's
        /// parser enforces and its help line renders.
        scalar: Scalar,
        /// How the repeats of the flag reach the request, and `None` for a
        /// parameter that takes one value.
        join: Option<Join>,
        /// The `format` the document declares about this value, in its own
        /// spelling, and `None` where it declares none. It sits beside the
        /// rules rather than among them: nothing in this crate reads it, and
        /// [`Param::format`] is how an adopter does.
        format: Option<String>,
    },
    /// Nothing a flag carries. The subcommand names the parameter in its long
    /// help and grows nothing for it, and the request goes out without it.
    Unreachable(Unsupported),
}

/// How the values of a list parameter reach the request.
///
/// Read off the `style` and `explode` the parameter declares, and obeyed by the
/// request builder and by the flag's help line alike — so what `--help` says a
/// repeated flag does is what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Join {
    /// One field per value: `?embed=a&embed=b`. A query parameter's
    /// `style: form` with `explode: true`, which is what OpenAPI defaults a
    /// query parameter to.
    Pairs,
    /// One field, values separated by commas: `?embed=a,b`, and `a,b` in a path
    /// segment or a header. A query parameter's `style: form` with
    /// `explode: false`, and `style: simple` everywhere else.
    Commas,
}

/// Why a parameter carries no flag.
///
/// Six shapes, one answer, because they cost a document the same thing: an
/// operation nobody can express sits beside a hundred that are expressible, and
/// refusing the document for it makes those hundred unreachable too. So an
/// unsupported parameter is carried rather than refused, and only a *required*
/// one — an operation that could never be invoked correctly — is named as a
/// `LoadError` while the document is reduced.
///
/// Four of the six are the document describing a value this CLI cannot put on
/// a flag. [`Unsupported::Unspellable`] and [`Unsupported::Shared`] are the other
/// half of the same question: the value is ordinary and it is the *name* that no
/// flag can carry, because the CLI cannot spell it or because another parameter
/// already has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unsupported {
    /// `in: cookie`. This CLI sends no cookies.
    Cookie,
    /// Described by `content` rather than `schema`: what the document asks for
    /// is a document in some media type, not a value.
    Encoded,
    /// A schema that is neither a value nor a list of values — an object, or an
    /// array of them.
    ///
    /// There is no spelling here to be exact about. OpenAPI states how a *flat*
    /// object serialises under `deepObject` and states nothing at all for a
    /// nested one; under the `form` a query parameter defaults to, an object's
    /// properties become top-level fields that collide with the operation's own
    /// parameters. A rendering this crate invented would produce a request that
    /// looks sent and is not read, which is worse than one that was never built.
    Structured,
    /// A `style` this crate does not serialise, named as the document spells
    /// it. Writing it out as some other style would put the value on the wire
    /// in a shape the server does not read.
    Style(String),
    /// A name with no spelling as a flag, so there is no flag to offer. It holds
    /// a character outside ASCII, which nothing here transliterates (`straße`),
    /// or it does not kebab-case into `[a-z0-9-]`: a parameter named `*` or `()`
    /// or `_` is the vendor's business and reaches a command line as nothing at
    /// all.
    ///
    /// A flag is the one name in this crate a *user types*, so it passes the
    /// rule [`crate::names`] states once and a command name and a gate are
    /// already held to. A parameter spelled outside it would otherwise get a
    /// flag called `""`, which clap renders as the end-of-options marker and
    /// which nothing on the help page distinguishes from one — a name the
    /// command line cannot spell, spelled anyway, and discoverable only by
    /// accident.
    Unspellable,
    /// A name that an earlier parameter of the operation already has, in the
    /// place this value names.
    ///
    /// OpenAPI tells two parameters apart by their name and their place, and
    /// [`Values`](crate::Values) tells them apart by their name alone, so a value
    /// for the name reaches the first parameter that has it and never the
    /// second. The one that comes later in [`Operation::params`] has no flag, so
    /// no flag is left that could never send.
    Shared(Place),
}

impl std::fmt::Display for Unsupported {
    /// The sentence a refusal and a subcommand's long help both use, so that
    /// what a user is told about a missing flag is what a bless step was told.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cookie => out.write_str("`in: cookie`, which this CLI does not send"),
            Self::Encoded => {
                out.write_str("described by `content`, which this CLI does not encode")
            }
            Self::Structured => out.write_str("neither a value nor a list of values"),
            Self::Style(style) => {
                write!(
                    out,
                    "declared with `style: {style}`, which this CLI does not serialise"
                )
            }
            Self::Unspellable => out.write_str(
                "named with a character outside ASCII, or in a way that does not kebab-case into \
                 [a-z0-9-]",
            ),
            Self::Shared(place) => write!(out, "sharing its name with the `{place}` parameter"),
        }
    }
}

impl Join {
    /// What a repeated flag does, for its help line. The same fact the request
    /// builder obeys, written once.
    #[must_use]
    pub fn note(self) -> &'static str {
        match self {
            Self::Pairs => "repeatable; each value is sent as its own field",
            Self::Commas => "repeatable; the values are sent comma-separated in one field",
        }
    }
}

impl Shape {
    /// Whether this parameter takes more than one value: a flag that may be
    /// given again, and a list on the wire.
    #[must_use]
    pub fn repeatable(&self) -> bool {
        matches!(self, Self::Flag { join: Some(_), .. })
    }
}

/// One scalar property of a flat JSON body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    name: String,
    flag: String,
    required: bool,
    scalar: Scalar,
    format: Option<String>,
    description: Option<String>,
}

/// What the operation wants in the request body — and therefore which flags the
/// subcommand grows. Each variant is exactly one flag set, so a body can never
/// offer a flag that the request builder then ignores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Body {
    /// The document asks for no body.
    None,
    /// A JSON object whose every property is a scalar: one flag per property,
    /// plus `--json-body` as a base document to merge them over.
    ///
    /// No template: the per-field flags already say what goes in the body, one
    /// per property, each with the rules its schema states. A second rendering
    /// of the same facts in a second notation would be the one place the two
    /// could come to disagree.
    JsonFields(Vec<Field>),
    /// JSON this CLI will not take apart — a nested object, an array, anything
    /// but an object of scalars. `--json-body` only, and no dead per-field
    /// flags beside it.
    ///
    /// This is the body with nothing on the command line saying what goes in
    /// it, so this is the body that carries a `template`: the JSON skeleton the
    /// same walk saw while it was deciding the body was not flat, rendered
    /// while the document was reduced. `None` where the document describes no
    /// shape to render, and then the subcommand grows no flag to ask for one.
    JsonWhole {
        /// Whether the document requires the body.
        required: bool,
        /// The JSON skeleton of the body, rendered while the document was
        /// reduced, and `None` where the document describes no shape to render.
        template: Option<String>,
    },
    /// `multipart/form-data`: assembled from `--file name=@path` and
    /// `--field name=value`. `names` is what the document declares, for the
    /// help line; the CLI accepts any part name, because a document that
    /// declares none (or misdeclares them) is common and refusing would help
    /// nobody.
    Multipart {
        /// The part names the document declares, in its order.
        names: Vec<String>,
        /// Whether the document requires the body.
        required: bool,
    },
    /// A media type this CLI does not assemble. `--raw-body FILE` sends the
    /// bytes verbatim under this `Content-Type` — and a media type is what it
    /// is, because a `content` key that is not one is refused while the
    /// document is reduced rather than carried here.
    Opaque {
        /// The `Content-Type` the bytes go out under, as the document spells
        /// the media type.
        media_type: String,
        /// Whether the document requires the body.
        required: bool,
    },
}

/// The bytes a bless step wrote are not a reduction this build can read.
///
/// This is the whole of what [`Document::from_blob`] and [`Document::to_blob`]
/// can say, and therefore the whole of what a shipped binary can fail with —
/// the one door it has onto a document is the blob. The type has the same
/// shape in every build: a caller's match on it is the same, `_` arm included,
/// whether or not the `document` feature is on, and reading its rustdoc under
/// either feature set tells them the same thing. Reducing a document is
/// a different job with a different failure list, and `LoadError` — which the
/// `document` feature brings with the reader — is where that list lives.
///
/// Every variant that refuses a blob says to run the bless step again, because
/// the repair is the same for all of them: a blob is written by a bless step,
/// and a blob that cannot be read is written again. [`Encode`] is the one
/// variant that is not about a blob that was read.
///
/// [`from_blob`] asks the first four in this order. It reads the stamp the blob
/// opens with before it decodes a byte, so [`Unstamped`] and [`Stale`] never
/// depend on what follows the stamp. [`Blob`] and [`Trailing`] are a blob that
/// does not decode as the model and nothing else. The rest are a blob that
/// decodes and holds something a load would not have written, and a rule that
/// compares two flags or two operations needs the whole model, so they are
/// asked after the decode. They are asked operation by operation in the order
/// of the document, and the names two operations share after every operation,
/// not in the order of this page. Each of them names the operation that holds
/// it, [`DuplicateCommand`] names both of two, and [`Confirmation`] names the
/// word the document confirms with, which belongs to no operation.
///
/// [`from_blob`]: Document::from_blob
/// [`Unstamped`]: DocumentError::Unstamped
/// [`Stale`]: DocumentError::Stale
/// [`Blob`]: DocumentError::Blob
/// [`Trailing`]: DocumentError::Trailing
/// [`Confirmation`]: DocumentError::Confirmation
/// [`DuplicateCommand`]: DocumentError::DuplicateCommand
/// [`Encode`]: DocumentError::Encode
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DocumentError {
    /// The bytes do not open with a layout stamp: an empty file, a blob from a
    /// typed-openapi older than 0.1.0, or a file that is no model blob at all.
    #[error(
        "the model blob carries no layout stamp, so a typed-openapi older than 0.1.0 wrote it, \
         or it is not a model blob; this build reads layout {reads}; run the bless step again"
    )]
    Unstamped {
        /// The layout this build reads, [`Document::MODEL_LAYOUT`].
        reads: u32,
    },
    /// The stamp names another layout than this build reads. Nothing after the
    /// stamp was decoded, because the same bytes would mean something else.
    #[error(
        "the model blob has layout {layout}, written by typed-openapi {version}; \
         this build reads layout {reads}; run the bless step again"
    )]
    Stale {
        /// The layout the blob was written in.
        layout: u32,
        /// The version of typed-openapi that wrote it, as the stamp spells it.
        version: String,
        /// The layout this build reads, [`Document::MODEL_LAYOUT`].
        reads: u32,
    },
    /// The stamp is this build's, and `postcard` could not read the bytes after
    /// it as a reduction.
    #[error("the model blob is damaged; run the bless step again")]
    Blob(#[source] postcard::Error),
    /// The stamp is this build's, the bytes after it decode as a reduction, and
    /// more bytes follow the reduction. A blob is the stamp and one encoding and
    /// nothing else, so bytes that were added or left over from a longer file
    /// mean the blob is not what a bless step wrote.
    #[error(
        "the model blob is damaged: bytes left after the model: {bytes}; run the bless step again"
    )]
    Trailing {
        /// How many bytes follow the encoding of the reduction.
        bytes: usize,
    },
    /// The document's confirmation word is one a load refuses: a word nobody
    /// could type as a flag, or one that every subcommand spends already.
    #[error(
        "the confirmation word `{word}` of the model blob is not one a load accepts; \
         run the bless step again"
    )]
    Confirmation {
        /// The word as the blob spells it, without the `--`.
        word: String,
    },
    /// The confirmation word an operation declares is one a load refuses. It is
    /// the flag clap sees: where it is `help`, a flag that the subcommand
    /// declares already, or a word with a leading `-`, a debug build of the
    /// binary panics when it builds the command tree, and a release build lets
    /// one declaration take the place of the other.
    #[error(
        "{op}: the confirmation word `{word}` is not one a load accepts; \
         run the bless step again"
    )]
    OperationConfirmation {
        /// The `operationId` of the operation that declares the word.
        op: String,
        /// The word as the blob spells it, without the `--`.
        word: String,
    },
    /// A flag of an operation, on a parameter or on a body property, is not
    /// spelled as a load spells one: not kebab-case, not `[a-z0-9-]`, or empty.
    /// A flag with a leading `-` makes a debug build of the binary panic when it
    /// builds the command tree.
    #[error("{op}: the flag `{flag}` is not one a load spells; run the bless step again")]
    Flag {
        /// The `operationId` of the operation that declares the flag.
        op: String,
        /// The flag as the blob spells it, without the `--`.
        flag: String,
    },
    /// A flag of an operation, on a parameter, a body property or a gate, is one
    /// that every subcommand spends already: `--help`, or one of the five flags
    /// that carry a body. Where the subcommand declares the word too, which is
    /// always so for `help`, a debug build of the binary panics when it builds
    /// the command tree, and a release build lets one declaration take the place
    /// of the other.
    #[error(
        "{op}: the flag `{flag}` is one every subcommand already spends; \
         run the bless step again"
    )]
    FlagSpent {
        /// The `operationId` of the operation that declares the flag.
        op: String,
        /// The flag as the blob spells it, without the `--`.
        flag: String,
    },
    /// Two declarations of one operation take one flag. A declaration is a
    /// parameter, a body property, a gate or, on a write, the confirmation word,
    /// and any two of them clash, as does one gate named twice. A load gives
    /// every declaration a flag of its own. A debug build of the binary panics
    /// when it builds the command tree and meets a flag twice, and a release
    /// build lets one declaration take the place of the other.
    #[error("{op}: the flag `{flag}` is declared twice; run the bless step again")]
    FlagTwice {
        /// The `operationId` of the operation that declares the flag.
        op: String,
        /// The flag as the blob spells it, without the `--`.
        flag: String,
    },
    /// A read names a gate. A read is sent on sight, so there is nothing for a
    /// gate to hold back, and a load refuses the document that says both. A
    /// `GET` that the document makes a write and gates is held as a write, so a
    /// blob that holds it as a read is one that no load wrote, and the gate it
    /// still names is what gives it away.
    #[error(
        "{op}: a read stands behind no gate, and this one names `{gate}`; \
         run the bless step again"
    )]
    GatedRead {
        /// The `operationId` of the read.
        op: String,
        /// The gate as the blob spells it, without the `--`.
        gate: String,
    },
    /// An operation is marked a read and its method writes. A load makes every
    /// method but `GET`, `HEAD`, `OPTIONS` and `TRACE` a write, so a blob that
    /// says otherwise is one that no load wrote, and a read is sent on sight:
    /// the subcommand declares no confirmation word, and nothing holds the
    /// request back.
    ///
    /// The blob does not hold the document's `x-cli-writes`, so the check finds
    /// a method that writes held as a read and cannot find a `GET` that the
    /// document marks as a write and the blob holds as a read. Where such a
    /// `GET` names a gate, [`DocumentError::GatedRead`] finds it.
    #[error(
        "{op}: `{method}` writes, and the model blob holds the operation as a read, \
         which is sent on sight; run the bless step again"
    )]
    WriteAsRead {
        /// The `operationId` of the operation held as a read.
        op: String,
        /// The method of the operation, as the blob spells it.
        method: String,
    },
    /// A group or a command is named `help`. A load refuses a document that
    /// places an operation under one. clap declares a `help` subcommand on every
    /// command that has subcommands, so a debug build of the binary panics when
    /// it builds the command tree, and a release build lets one take the place
    /// of the other.
    #[error(
        "{op}: the {what} `{name}` is a subcommand that clap declares itself; \
         run the bless step again"
    )]
    NameSpent {
        /// The `operationId` of the operation under the name.
        op: String,
        /// Which of the two names it is: `group` or `command`.
        what: &'static str,
        /// The name as the blob spells it.
        name: String,
    },
    /// Two operations sit under one `<group> <command>`. A load refuses a
    /// document that has two. A debug build of the binary panics when it builds
    /// the command tree, and a release build lets one shadow the other.
    #[error(
        "`{first}` and `{second}` are both `{group} {command}` on the command line; \
         run the bless step again"
    )]
    DuplicateCommand {
        /// The group both operations are mounted under.
        group: CommandName,
        /// The command both operations are mounted under.
        command: CommandName,
        /// The `operationId` of the operation that comes first in the blob.
        first: String,
        /// The `operationId` of the later one.
        second: String,
    },
    /// A parameter requires its own name. A load refuses a document that says
    /// so, and a debug build of the binary panics when it builds the command
    /// tree, because clap does not let an argument require itself.
    #[error("{op}: `{name}` requires itself; run the bless step again")]
    RequiresItself {
        /// The `operationId` of the operation that declares the parameter.
        op: String,
        /// The parameter that requires itself, as the document spells it.
        name: String,
    },
    /// A parameter requires a name the operation declares no parameter under.
    #[error(
        "{op}: `{name}` requires `{requires}`, which is not a parameter of this operation; \
         run the bless step again"
    )]
    RequiresUnknown {
        /// The `operationId` of the operation that declares the parameter.
        op: String,
        /// The parameter that requires another, as the document spells it.
        name: String,
        /// The name that no parameter of the operation has.
        requires: String,
    },
    /// A parameter requires one that has no flag, so no request giving the
    /// first could ever be built.
    #[error(
        "{op}: `{name}` requires `{requires}`, and `{requires}` is {why}; \
         run the bless step again"
    )]
    RequiresUnreachable {
        /// The `operationId` of the operation that declares the parameter.
        op: String,
        /// The parameter that requires another, as the document spells it.
        name: String,
        /// The listed parameter that has no flag.
        requires: String,
        /// Why this crate has no flag for it.
        why: Unsupported,
    },
    /// `postcard` could not write the reduction as bytes. No reduction a load
    /// writes fails this way, so it is a defect of the encoder or of a type of
    /// the model, and the bless step that meets it has no blob to write again.
    #[error("the model could not be written as a blob")]
    Encode(#[source] postcard::Error),
}

/// A generated inventory and the document it was generated from disagree.
///
/// Generated code names operations by position in the inventory it was emitted
/// from. [`Document::matches`] is what makes that positional promise true for a
/// document read at run time, and this is what it says when it is not.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error(
    "the document and the generated inventory disagree at operation {position}: \
     the inventory says `{expected}` and the document says {}",
    found.as_deref().map_or_else(|| "there is no such operation".to_owned(), |id| format!("`{id}`"))
)]
pub struct DriftError {
    /// The index of the first row where the inventory and the document differ,
    /// counted from zero.
    pub position: usize,
    /// The `operationId` the inventory names at `position`, or `nothing after
    /// it` where the document has an operation past the inventory's last row.
    pub expected: String,
    /// The `operationId` the document has at `position`, or `None` where the
    /// document ends before the inventory does.
    pub found: Option<String>,
}

impl Effect {
    /// What the method alone makes of an operation: every method but `GET`,
    /// `HEAD`, `OPTIONS` and `TRACE` writes.
    ///
    /// The document's `x-cli-writes` can add a write to a safe method and never
    /// removes one, so this is the least an operation is. The load starts from
    /// it, and a decoded blob that holds a read where this says write is
    /// refused.
    pub(crate) fn of_method(method: &Method) -> Self {
        match *method {
            Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE => Self::Read,
            _ => Self::Write,
        }
    }
}

impl Document {
    /// The layout of the model this build reads and writes.
    ///
    /// Every blob opens with the number of the layout it was written in, and
    /// [`Document::from_blob`] refuses another number before it decodes
    /// anything. The number moves with the layout: a change to a type of the
    /// model that `postcard` writes differently, such as a field added,
    /// removed, reordered or retyped, or a variant added, removed or moved,
    /// needs a new number. It moves too when a load writes something different
    /// for the same document and no type changed. The crate's own tests trace
    /// the layout and fail on a change to a type that left the number where it
    /// was.
    pub const MODEL_LAYOUT: u32 = 5;

    /// The same reduction, already done and written down.
    ///
    /// This is the call a shipped binary makes. `Document::load` is bless-time
    /// work — a YAML parse, an `openapiv3` deserialisation and a walk over every
    /// path item — and none of it tells a CLI anything that is not already in
    /// here. The bytes come from [`Document::to_blob`] in the same bless run
    /// that wrote the rest of the generated code.
    ///
    /// The stamp is read first. A blob that does not open with one is
    /// [`DocumentError::Unstamped`], and one stamped with another layout is
    /// [`DocumentError::Stale`]; neither reaches the decoder, so a blob that
    /// another build wrote never decodes into a wrong model. A blob of this
    /// layout that does not decode is [`DocumentError::Blob`], and bytes after
    /// the encoding are [`DocumentError::Trailing`].
    ///
    /// What decodes is then checked again, because a shipped binary starts from
    /// the blob and nothing has read the document. The check repeats these rules
    /// of a load: the confirmation word of the document, and for each operation
    /// its group and command, which are not called `help`; the effect its method
    /// allows; its confirmation word; its gates, which stand on a write only; its
    /// flags, each spelled as a load spells it, none of them `help` or one of the
    /// five that carry a body, and each declared once; and each `requires`. It
    /// also repeats that no two operations share a `<group> <command>`. It leaves
    /// out the rules whose input the blob does not hold, such as a `$ref`, and
    /// three that a blob can break: a required parameter that has no flag, a body
    /// whose `Content-Type` is no media type, and a `pattern` that no engine here
    /// reads.
    ///
    /// Each operation is asked in the order of the document, and the names two
    /// operations share after every operation. The error names the operation, or
    /// both of two operations that share a name, and for the document's own
    /// confirmation word it names the word. The two command names and the
    /// spelling of each gate are checked as they decode.
    ///
    /// Only `from_blob` applies these rules. `postcard` drops the message of an
    /// error a `Deserialize` raises, so a check inside one could not name the
    /// operation, and a rule that compares two flags or two operations cannot sit
    /// in a single field. A `Document` read through `Deserialize` in another
    /// format is not held to them.
    pub fn from_blob(blob: &[u8]) -> Result<Self, DocumentError> {
        let unstamped = || DocumentError::Unstamped {
            reads: Self::MODEL_LAYOUT,
        };
        let (stamp, encoding) = blob::read(blob).ok_or_else(unstamped)?;
        if stamp.layout != Self::MODEL_LAYOUT {
            return Err(DocumentError::Stale {
                layout: stamp.layout,
                version: stamp.version,
                reads: Self::MODEL_LAYOUT,
            });
        }
        let (document, rest) =
            postcard::take_from_bytes::<Self>(encoding).map_err(DocumentError::Blob)?;
        if !rest.is_empty() {
            return Err(DocumentError::Trailing { bytes: rest.len() });
        }
        document.check()?;
        Ok(document)
    }

    /// This reduction, as the bytes a bless step commits.
    ///
    /// The bytes open with a stamp: a magic of eight bytes, the number of the
    /// layout the model is written in ([`Document::MODEL_LAYOUT`], four bytes,
    /// little-endian), and the version of typed-openapi that wrote it (its
    /// length in four bytes, little-endian, then its UTF-8). The `postcard`
    /// encoding follows. The format of the stamp itself never changes, because
    /// it is what a later build reads of a blob it cannot decode.
    ///
    /// The encoding is not self-describing, and the stamp is what tells a build
    /// that a blob was written in another layout. A blob also carries the
    /// version, so a bless step run again after the crate moves writes other
    /// bytes even when the layout stayed. An adopter who skips the bless step
    /// after a change to a document is caught by the pairing check in `Api::new`
    /// and by the test that reduces the committed document and compares it with
    /// this.
    pub fn to_blob(&self) -> Result<Vec<u8>, DocumentError> {
        let stamp = blob::stamp(Self::MODEL_LAYOUT, env!("CARGO_PKG_VERSION"));
        postcard::to_extend(self, stamp).map_err(DocumentError::Encode)
    }

    /// The server the document names first, with each variable in its URL
    /// replaced by the `default` the document gives it. A caller may override it.
    #[must_use]
    pub fn base(&self) -> &Uri {
        &self.base
    }

    /// The word this CLI spends on confirming a write, without the `--`.
    ///
    /// `commit` unless the adopter chose another one while the document was
    /// loaded. Anything that describes the confirmation in prose — a help
    /// page, the summary, a generated doc comment — reads it from here rather
    /// than spelling `--commit`, so that what a page says and what a
    /// subcommand accepts are one string.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// Every operation, in document order.
    pub fn iter(&self) -> std::slice::Iter<'_, Operation> {
        self.ops.iter()
    }

    /// By `operationId`, as the document spells it. This is the lookup a typed
    /// Rust caller uses.
    #[must_use]
    pub fn get(&self, operation_id: &str) -> Option<&Operation> {
        self.ops.iter().find(|op| op.id == operation_id)
    }

    /// By the two names the user types, `<group> <command>`.
    #[must_use]
    pub fn by_command(&self, group: &str, command: &str) -> Option<&Operation> {
        self.ops
            .iter()
            .find(|op| op.group.as_str() == group && op.command.as_str() == command)
    }

    /// Every operation, in document order. The order is the one a generated
    /// inventory is emitted in, which is what [`Document::matches`] checks.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.ops
    }

    /// Every gate any operation in this document names, once each and in
    /// document order.
    ///
    /// This is the list a `--help` page, a release note or a test suite reads
    /// instead of keeping one by hand: a gate an Overlay adds appears here the
    /// moment the document is reduced, and nothing has to be told twice.
    #[must_use]
    pub fn gates(&self) -> Vec<&Gate> {
        let mut named: Vec<&Gate> = Vec::new();
        for gate in self.ops.iter().flat_map(Operation::gates) {
            if !named.contains(&gate) {
                named.push(gate);
            }
        }
        named
    }

    /// Every operation standing behind one gate, in document order.
    ///
    /// A suite that has something to say about everything irreversible asks
    /// the document which operations those are, rather than carrying a list
    /// that an Overlay can silently grow past.
    pub fn gated_by(&self, gate: &str) -> impl Iterator<Item = &Operation> {
        self.ops
            .iter()
            .filter(move |op| op.gates().iter().any(|named| named.as_str() == gate))
    }

    /// Check this document against a generated `(operationId, method, path)`
    /// inventory, row by row and in order.
    ///
    /// A caller that has run this may index [`Document::operations`] by the
    /// inventory's own positions: every row named an operation, and every
    /// operation was named by a row.
    pub fn matches(&self, inventory: &[(&str, &str, &str)]) -> Result<(), DriftError> {
        let drift = |position: usize, expected: &str| DriftError {
            position,
            expected: expected.to_owned(),
            found: self.ops.get(position).map(|op| op.id.clone()),
        };
        for (position, (id, method, path)) in inventory.iter().enumerate() {
            match self.ops.get(position) {
                Some(op) if op.id == *id && op.method == *method && op.path == *path => {}
                _ => return Err(drift(position, id)),
            }
        }
        match self.ops.get(inventory.len()) {
            None => Ok(()),
            Some(extra) => Err(DriftError {
                position: inventory.len(),
                expected: "nothing after it".to_owned(),
                found: Some(extra.id.clone()),
            }),
        }
    }
}

impl<'a> IntoIterator for &'a Document {
    type Item = &'a Operation;
    type IntoIter = std::slice::Iter<'a, Operation>;

    fn into_iter(self) -> Self::IntoIter {
        self.ops.iter()
    }
}

impl Operation {
    /// The `operationId`, as the document spells it.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The group this operation is mounted under, as the user types it.
    #[must_use]
    pub fn group(&self) -> &CommandName {
        &self.group
    }

    /// The subcommand name under that group, as the user types it.
    #[must_use]
    pub fn command(&self) -> &CommandName {
        &self.command
    }

    /// The HTTP method, which with the `x-cli-writes` marker decides the
    /// operation's [`Effect`].
    #[must_use]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// The path template, `{name}` placeholders intact.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The document's one-line `summary` of the operation, if it has one.
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    /// The document's longer `description` of the operation, if it has one.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Every parameter the document declares for the operation, the ones with
    /// no flag included: the path item's in the order it lists them, then the
    /// operation's own. A parameter of the operation takes the place of the
    /// path item's with the same name and place, as OpenAPI says. The three
    /// headers it says to ignore (`Accept`, `Content-Type` and `Authorization`)
    /// are not listed.
    #[must_use]
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// The request body the document asks for, and the flags it grows.
    #[must_use]
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Whether the operation runs on sight or waits for the confirmation. It
    /// comes from the method and the `x-cli-writes` marker, never from a name.
    #[must_use]
    pub fn effect(&self) -> Effect {
        self.effect
    }

    /// The named hazards this operation stands behind, in the order the
    /// document names them. Every one of them is answered beside the write
    /// confirmation, and a read has none.
    #[must_use]
    pub fn gates(&self) -> &[Gate] {
        &self.gates
    }

    /// The word this CLI spends on confirming a write, without the `--`.
    ///
    /// Decided while the document was reduced and copied onto every operation,
    /// because a subcommand is built from one operation alone: `tree` (the
    /// `clap` feature) is handed an `Operation` and no document, and the flag it declares has
    /// to be the word the adopter chose. [`Document::commit`] is the same
    /// string, written from the same place, for a caller that has the document
    /// and wants it once.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// The first parameter the document spells `name`, if there is one: the one
    /// that a value given under that name reaches. Another parameter with the
    /// name has no flag ([`Unsupported::Shared`]).
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Every value of this operation the document declares `format` about, in
    /// document order: the parameters first, then the fields of a flat body.
    ///
    /// This is the question a guard asks — which of the values I am about to
    /// send are of a kind I have something to say about — and it is asked of
    /// the reduced model, so a shipped binary answers it with no document, no
    /// reader and no second pass over anything. The document names the kind
    /// and this crate carries the name; what the name *means* is the adopter's,
    /// and a rule this crate could run would have been a `pattern`.
    ///
    /// Two shapes are silent here, both because they carry no value for a kind
    /// to be about. A parameter this CLI cannot spell is a
    /// [`Shape::Unreachable`]: it has no scalar, no flag and no place in the
    /// request. A [`Body::JsonWhole`] has no fields at all — a body with one
    /// nested property goes through `--json-body` whole — so the kinds its
    /// properties declare are not reachable from here, and a guard over such a
    /// body is a guard over JSON the adopter reads themselves.
    ///
    /// The halves are reachable on their own: [`Operation::params`] with
    /// [`Param::format`], and [`Body::fields`] with [`Field::format`]. Take
    /// them where one half is what the caller means. What this adds over
    /// chaining them is the *other* half — a guard written over the parameters
    /// and not the body passes every body field it was meant to cover, and an
    /// omission announces itself the way no wrong answer does, which is not at
    /// all. [`Carrier`] is then what makes one list out of two: both halves
    /// name themselves with a `&str`, and the two go to a request by different
    /// roads, so the name alone does not say which road.
    pub fn carrying(&self, format: &str) -> impl Iterator<Item = Carrier<'_>> {
        let params = self
            .params
            .iter()
            .filter(move |param| param.format() == Some(format))
            .map(Carrier::Param);
        let fields = self
            .body
            .fields()
            .iter()
            .filter(move |field| field.format() == Some(format))
            .map(Carrier::Field);
        params.chain(fields)
    }
}

impl Param {
    /// The wire name, as the document spells it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What this parameter is worth on a command line: a flag and everything
    /// that goes with one, or nothing.
    #[must_use]
    pub fn shape(&self) -> &Shape {
        &self.shape
    }

    /// The kind of value this parameter carries, as the document's `format`
    /// names it, and `None` where the document names none.
    ///
    /// A parameter this CLI cannot spell answers `None` whatever the document
    /// says: a [`Shape::Unreachable`] gets no flag, no argument and no place in
    /// the request, so there is no value here for a kind to be about.
    #[must_use]
    pub fn format(&self) -> Option<&str> {
        match &self.shape {
            Shape::Flag { format, .. } => format.as_deref(),
            Shape::Unreachable(_) => None,
        }
    }

    /// Whether the document requires the parameter.
    #[must_use]
    pub fn required(&self) -> bool {
        self.required
    }

    /// The other parameters of this operation that have to be given whenever
    /// this one is, by wire name, in the order the document's
    /// `x-cli-requires` lists them.
    ///
    /// OpenAPI has no way to say that two parameters only mean something
    /// together — an id and the kind of object it names, spelled across a
    /// query string — so the document says it with that marker. The reduction
    /// has already checked that every name is a parameter a request can carry;
    /// [`Invocation::new`](crate::Invocation::new) and the command line both
    /// refuse this one without them.
    #[must_use]
    pub fn requires(&self) -> &[String] {
        &self.requires
    }

    /// The document's `description` of the parameter, if it has one.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// A requirement among the parameters of one operation that cannot be mounted:
/// it names the parameter itself, a parameter the operation does not declare, or
/// one that has no flag.
///
/// The load and the decode of a blob both refuse such a requirement, each in
/// its own error, so the question is asked here once and each caller words the
/// answer.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Unmet<'p> {
    /// The parameter lists its own name. It asks for nothing, and a debug build
    /// of clap refuses an argument that requires itself.
    Itself { name: &'p str },
    /// The parameter that `requires` lists is not one the operation declares.
    Unknown { name: &'p str, requires: &'p str },
    /// The parameter that `requires` lists has no flag.
    Unreachable {
        name: &'p str,
        requires: &'p str,
        why: &'p Unsupported,
    },
}

/// The first requirement among `params` that cannot be mounted, in the order the
/// parameters are listed.
pub(crate) fn unmet(params: &[Param]) -> Option<Unmet<'_>> {
    params.iter().find_map(|param| {
        param.requires().iter().find_map(|requires| {
            let name = param.name();
            if requires == name {
                return Some(Unmet::Itself { name });
            }
            match params.iter().find(|other| other.name() == requires) {
                None => Some(Unmet::Unknown { name, requires }),
                Some(other) => match other.shape() {
                    Shape::Unreachable(why) => Some(Unmet::Unreachable {
                        name,
                        requires,
                        why,
                    }),
                    Shape::Flag { .. } => None,
                },
            }
        })
    })
}

impl Field {
    /// The property's name in the body, as the document spells it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The long flag as the user types it, without the leading `--`.
    #[must_use]
    pub fn flag(&self) -> &str {
        &self.flag
    }

    /// The flag is not the plain kebab-case of the wire name, because that name
    /// was already taken in this subcommand.
    #[must_use]
    pub fn renamed(&self) -> bool {
        renamed(&self.flag, &self.name)
    }

    /// Whether the body's schema requires the property.
    #[must_use]
    pub fn required(&self) -> bool {
        self.required
    }

    /// The rules the document states about one value of the property, which
    /// the flag's parser enforces.
    #[must_use]
    pub fn scalar(&self) -> &Scalar {
        &self.scalar
    }

    /// The kind of value this field carries, as the document's `format` names
    /// it, and `None` where the document names none.
    ///
    /// Only a flat body has fields to ask. One nested property sends the whole
    /// body through `--json-body`, and then there is no [`Field`] anywhere to
    /// carry what its properties declare.
    #[must_use]
    pub fn format(&self) -> Option<&str> {
        self.format.as_deref()
    }

    /// What the property says about itself, or what the schema it points at
    /// says, if either does.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

impl Body {
    /// Whether the document asks for a request body at all.
    ///
    /// The question a count of body-less operations asks, answered here rather
    /// than by a caller matching on one variant: a body kind added later is a
    /// body, and this says so without anyone revisiting the caller.
    #[must_use]
    pub fn present(&self) -> bool {
        match self {
            Self::None => false,
            Self::JsonFields(_)
            | Self::JsonWhole { .. }
            | Self::Multipart { .. }
            | Self::Opaque { .. } => true,
        }
    }

    /// Whether this body goes out whole: one flag for the lot, and no flag per
    /// property.
    ///
    /// A document that asks for no body answers `false` — there is nothing here
    /// to go out whole or in pieces — so an operation with no per-field flags is
    /// either this or body-less, and the two are worth counting apart: one has
    /// nothing to fill in, the other is a body an adopter has to hand over
    /// themselves. A flat body of no properties is one of these too, because
    /// what it offers a command line is `--json-body` and nothing else.
    #[must_use]
    pub fn whole(&self) -> bool {
        match self {
            Self::None => false,
            Self::JsonFields(fields) => fields.is_empty(),
            Self::JsonWhole { .. } | Self::Multipart { .. } | Self::Opaque { .. } => true,
        }
    }

    /// The properties this body offers a flag each, in document order.
    ///
    /// A flat JSON body — an object whose every property is a scalar — is the
    /// one shape a command line takes apart, so it is the one shape with
    /// fields. Every other body answers with nothing, and each of them is empty
    /// for a reason a caller can act on: [`Body::None`] because the document
    /// asks for no body, [`Body::JsonWhole`] because a single nested property
    /// sends the whole body through `--json-body` and its properties are never
    /// reduced to fields, [`Body::Multipart`] because its parts are files and
    /// text rather than properties, and [`Body::Opaque`] because the bytes go
    /// out as they arrived.
    ///
    /// That is what makes this the door for anything walking a body a value at
    /// a time — a guard, a renderer, a page of documentation. An empty answer
    /// is the true one: what is not here is not reachable a property at a time
    /// by anything, so a walk needs no match over the variants to be complete,
    /// and a body kind added later is covered without revisiting the caller.
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        match self {
            Self::JsonFields(fields) => fields,
            Self::None | Self::JsonWhole { .. } | Self::Multipart { .. } | Self::Opaque { .. } => {
                &[]
            }
        }
    }
}

/// `http::Uri` is not a `serde` type; the blob carries the string it prints as.
mod uri_string {
    use http::Uri;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(uri: &Uri, out: S) -> Result<S::Ok, S::Error> {
        out.collect_str(uri)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Uri, D::Error> {
        let raw = String::deserialize(input)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// `http::Method` is not a `serde` type either, and the eight OpenAPI methods
/// are exactly the ones it spells as constants.
mod method_string {
    use http::Method;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(method: &Method, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(method.as_str())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Method, D::Error> {
        let raw = String::deserialize(input)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}
