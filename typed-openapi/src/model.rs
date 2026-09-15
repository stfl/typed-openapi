//! The document, reduced to the facts a CLI and a typed caller both need.
//!
//! What is here is the reduction itself: a list of [`Operation`]s, the values
//! each one carries, and the two names a user types to reach it. Every type on
//! this page is in every build, because the blob a shipped binary reads is
//! these types and nothing else — so a caller who matches one exhaustively
//! writes the same match whatever the feature set.
//!
//! Where they come from is `reduce`, which the `document` feature brings and a
//! shipped binary leaves out. Reducing a document is bless-time work, not
//! startup work: a bless step enables the feature, calls `Document::load` once
//! and [`Document::to_blob`] on the result, and the binary it produces calls
//! [`Document::from_blob`] with no reader compiled into it to read YAML with.
//! The two are the same reduction by construction — there is one `load` — and a
//! test holds the shipped blob to the shipped document to prove the pair was
//! written by the same run.

#[cfg(feature = "document")]
mod reduce;

use http::{Method, Uri};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// The reading defines it and this module is where a caller goes looking, so
// `typed_openapi::LoadError` and `typed_openapi::model::LoadError` both name
// the one type wherever the reading itself is written down.
#[cfg(feature = "document")]
pub use self::reduce::LoadError;
use crate::names::{CommandName, NameError, renamed, spelled};
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
/// The write gate.
pub const COMMIT: &str = "commit";

/// Every operation the document describes, in document order, plus the server
/// it describes them against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(with = "uri_string")]
    base: Uri,
    ops: Vec<Operation>,
}

/// One operation: one subcommand under one group, one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    id: String,
    group: CommandName,
    command: CommandName,
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

/// Where a parameter goes in the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Location {
    Path,
    Query,
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
        flag: String,
        location: Location,
        scalar: Scalar,
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
/// Five shapes, one answer, because they cost a document the same thing: an
/// operation nobody can express sits beside a hundred that are expressible, and
/// refusing the document for it makes those hundred unreachable too. So an
/// unsupported parameter is carried rather than refused, and only a *required*
/// one — an operation that could never be invoked correctly — is named as a
/// `LoadError` while the document is reduced.
///
/// Four of the five are the document describing a value this CLI cannot put on
/// a flag. [`Unsupported::Unspellable`] is the other half of the same question:
/// the value is ordinary and it is the *name* that no flag can carry.
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
    /// A name with no kebab-case spelling, so there is no flag to offer. A
    /// parameter named `*` or `()` or `_` is the vendor's business and reaches
    /// a command line as nothing at all.
    ///
    /// A flag is the one name in this crate a *user types*, so it passes the
    /// rule [`crate::names`] states once and a command name and a gate are
    /// already held to. A parameter spelled outside it would otherwise get a
    /// flag called `""`, which clap renders as the end-of-options marker and
    /// which nothing on the help page distinguishes from one — a name the
    /// command line cannot spell, spelled anyway, and discoverable only by
    /// accident.
    Unspellable,
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
            Self::Unspellable => {
                out.write_str("named in a way that does not kebab-case into [a-z0-9-]")
            }
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
        required: bool,
        template: Option<String>,
    },
    /// `multipart/form-data`: assembled from `--file name=@path` and
    /// `--field name=value`. `names` is what the document declares, for the
    /// help line; the CLI accepts any part name, because a document that
    /// declares none (or misdeclares them) is common and refusing would help
    /// nobody.
    Multipart { names: Vec<String>, required: bool },
    /// A media type this CLI does not assemble. `--raw-body FILE` sends the
    /// bytes verbatim under this `Content-Type` — and a media type is what it
    /// is, because a `content` key that is not one is refused while the
    /// document is reduced rather than carried here.
    Opaque { media_type: String, required: bool },
}

/// The bytes a bless step wrote are not a reduction this crate can read.
///
/// This is the whole of what [`Document::from_blob`] and [`Document::to_blob`]
/// can say, and therefore the whole of what a shipped binary can fail with —
/// the one door it has onto a document is the blob. The type has the same
/// shape in every build: a caller who matches it exhaustively writes the same
/// match whether or not the `document` feature is on, and reading its rustdoc
/// under either feature set tells them the same thing. Reducing a document is
/// a different job with a different failure list, and `LoadError` — which the
/// `document` feature brings with the reader — is where that list lives.
#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("the reduced model the bless step writes is not valid: {0}")]
    Blob(#[from] postcard::Error),
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
    pub position: usize,
    pub expected: String,
    pub found: Option<String>,
}

impl Document {
    /// The same reduction, already done and written down.
    ///
    /// This is the call a shipped binary makes. `Document::load` is bless-time
    /// work — a YAML parse, an `openapiv3` deserialisation and a walk over every
    /// path item — and none of it tells a CLI anything that is not already in
    /// here. The bytes come from [`Document::to_blob`] in the same bless run
    /// that wrote the rest of the generated code.
    pub fn from_blob(blob: &[u8]) -> Result<Self, DocumentError> {
        Ok(postcard::from_bytes(blob)?)
    }

    /// This reduction, as the bytes a bless step commits.
    ///
    /// The encoding is not self-describing and carries no version tag: it is
    /// written and read by one build of one workspace, and an adopter who skips
    /// the bless step is caught by the pairing check in `Api::new` and by the
    /// test that reduces the committed document and compares it with this.
    pub fn to_blob(&self) -> Result<Vec<u8>, DocumentError> {
        Ok(postcard::to_allocvec(self)?)
    }

    /// The server the document names first. A caller may override it.
    #[must_use]
    pub fn base(&self) -> &Uri {
        &self.base
    }

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

    #[must_use]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// The path template, `{name}` placeholders intact.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    #[must_use]
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    #[must_use]
    pub fn body(&self) -> &Body {
        &self.body
    }

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

    /// The parameter the document spells `name`, if there is one.
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

    #[must_use]
    pub fn required(&self) -> bool {
        self.required
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

impl Field {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

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

    #[must_use]
    pub fn required(&self) -> bool {
        self.required
    }

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
