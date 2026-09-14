//! The document, reduced to the facts a CLI and a typed caller both need.
//!
//! `Document::load` takes the two files an adopter ships — the vendor's OpenAPI
//! document and their Overlay of corrections — and hands back a list of
//! [`Operation`]s with every `$ref` already resolved. Nothing from `openapiv3`
//! escapes this module: after `load` returns, the parsed document is dropped.
//!
//! Reducing a document is bless-time work, not startup work, and the `document`
//! feature is what says so in the manifest. A bless step enables it, calls
//! `Document::load` once and [`Document::to_blob`] on the result; the binary it
//! produces leaves it off, calls [`Document::from_blob`], and has no reader
//! compiled into it to read YAML with. The two are the same reduction by
//! construction — there is one `load` — and a test holds the shipped blob to
//! the shipped document to prove the pair was written by the same run.

use http::{Method, Uri};
#[cfg(feature = "document")]
use openapiv3::{
    Components, OpenAPI, Parameter, ParameterData, ParameterSchemaOrContent, PathStyle, QueryStyle,
    ReferenceOr, Schema, SchemaKind,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::names::{CommandName, renamed};
#[cfg(feature = "document")]
use crate::names::{Grouping, NameError, Namespace, kebab};
use crate::scalar::Scalar;
#[cfg(feature = "document")]
use crate::schema::{RefError, is_json, is_multipart, resolve, resolve_schema, scalar_of};

/// The three extensions this crate reads, all of them an adopter's say over
/// something the document alone cannot settle. An Overlay is where they are
/// written.
///
/// HTTP cannot say "this GET writes", so the document has to.
#[cfg(feature = "document")]
const WRITES: &str = "x-cli-writes";
/// The command name to mount an operation under, where the path spells one
/// badly — or where two operations reduce to the same name.
#[cfg(feature = "document")]
const COMMAND: &str = "x-cli-command";
/// The group to mount an operation under, where the path's own segment is not
/// the resource the operation belongs to.
#[cfg(feature = "document")]
const GROUP: &str = "x-cli-group";

/// Whole-body flag, for every operation that takes JSON.
pub const JSON_BODY: &str = "json-body";
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
}

/// Whether the CLI must hold this operation behind `--commit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    /// A safe method with no `x-cli-writes` marker: runs on sight.
    Read,
    /// A body-bearing or unsafe method, or a GET the document marks as writing.
    Write,
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
/// goes, what the flag accepts — hangs off [`Shape`], because a parameter this
/// CLI cannot spell has none of it.
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
/// Four shapes, one answer, because they cost a document the same thing: an
/// operation nobody can express sits beside a hundred that are expressible, and
/// refusing the document for it makes those hundred unreachable too. So an
/// unsupported parameter is carried rather than refused, and only a *required*
/// one — an operation that could never be invoked correctly — is named as a
/// `LoadError` while the document is reduced.
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
    JsonFields(Vec<Field>),
    /// JSON this CLI will not take apart — a nested object, an array, anything
    /// but an object of scalars. `--json-body` only, and no dead per-field
    /// flags beside it.
    JsonWhole { required: bool },
    /// `multipart/form-data`: assembled from `--file name=@path` and
    /// `--field name=value`. `names` is what the document declares, for the
    /// help line; the CLI accepts any part name, because a document that
    /// declares none (or misdeclares them) is common and refusing would help
    /// nobody.
    Multipart { names: Vec<String>, required: bool },
    /// A media type this CLI does not assemble. `--raw-body FILE` sends the
    /// bytes verbatim under this `Content-Type`.
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

/// Reducing an OpenAPI document to [`Operation`]s failed.
///
/// *Requires the `document` feature.* Every variant names a way that reading a
/// document goes wrong, so the whole type is bless-time: a binary that starts
/// from [`Document::from_blob`] cannot produce one, does not compile one, and
/// does not have to know the list exists.
#[cfg(feature = "document")]
#[derive(Debug, Error)]
pub enum LoadError {
    #[error(transparent)]
    Overlay(#[from] crate::overlay::OverlayError),
    #[error("the overlaid document is not an OpenAPI 3 document: {0}")]
    Shape(#[source] serde_json::Error),
    #[error("no `servers` entry to send requests to")]
    NoServer,
    #[error("`servers[0].url` ({url}) is not a URL: {source}")]
    ServerUrl {
        url: String,
        #[source]
        source: http::uri::InvalidUri,
    },
    #[error("{method} {path} has no operationId")]
    NoOperationId { method: String, path: String },
    #[error(transparent)]
    Name(#[from] NameError),
    #[error("{op}: `{key}` is not a string")]
    Override { op: String, key: &'static str },
    #[error(
        "`{first}` and `{second}` are both `{group} {command}` on the command line; \
         give one of them an `x-cli-command`"
    )]
    DuplicateCommand {
        group: CommandName,
        command: CommandName,
        first: String,
        second: String,
    },
    #[error(transparent)]
    Reference(#[from] RefError),
    /// A parameter this CLI cannot supply that the document says a caller must.
    ///
    /// Every other unsupported parameter is carried as [`Shape::Unreachable`]
    /// and costs the document nothing. This one is an operation that could never
    /// be invoked correctly, so it is named here rather than mounted as a
    /// subcommand that is guaranteed to build a request the server refuses.
    #[error(
        "{op}: parameter `{name}` is {why}, and the document requires it; \
         correct the parameter in an Overlay, or drop its `required`"
    )]
    Parameter {
        op: String,
        name: String,
        why: Unsupported,
    },
    /// A rule the document states that cannot be run — a `pattern` no regex
    /// engine here reads. Refused while the document is reduced, because a rule
    /// that cannot run is one every value would otherwise pass.
    #[error("{op}: `{name}`: {source}")]
    Unrunnable {
        op: String,
        name: String,
        #[source]
        source: crate::scalar::ScalarError,
    },
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
    /// Parse the vendor's document, lay the adopter's Overlays over it in
    /// order, and resolve the result into operations.
    ///
    /// *Requires the `document` feature.*
    ///
    /// Every argument is a file's contents, YAML or JSON. `overlays` is a list
    /// because corrections come in layers — each one corrects the document the
    /// ones before it produced, so the order they are given in is the order
    /// they happen. An empty list runs a document that is already corrected.
    ///
    /// This is the expensive door, and the `document` feature is what opens it.
    /// A bless step calls it once and writes [`Document::to_blob`] beside the
    /// rest of what it generates; a shipped binary compiles without the feature
    /// and reaches the same reduction through [`Document::from_blob`].
    ///
    /// ```
    /// use typed_openapi::{Document, Invocation, Values};
    ///
    /// let doc = Document::load(
    ///     include_str!("../tests/fixtures/toy.yaml"),
    ///     &[
    ///         include_str!("../tests/fixtures/corrections.yaml"),
    ///         include_str!("../tests/fixtures/cli.yaml"),
    ///     ],
    /// )?;
    /// let op = doc.get("getVoucher").expect("the document describes it");
    /// let request = Invocation::new(op, Values::new().param("id", 5))?.request(doc.base())?;
    /// assert_eq!(request.uri().path(), "/vouchers/5");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[cfg(feature = "document")]
    pub fn load(document: &str, overlays: &[&str]) -> Result<Self, LoadError> {
        let mut doc = crate::overlay::parse(document)?;
        for overlay in overlays {
            doc = crate::overlay::apply(doc, overlay)?;
        }
        let doc: OpenAPI = serde_json::from_value(doc).map_err(LoadError::Shape)?;
        Self::from_openapi(&doc)
    }

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

    #[cfg(feature = "document")]
    fn from_openapi(doc: &OpenAPI) -> Result<Self, LoadError> {
        let server = doc.servers.first().ok_or(LoadError::NoServer)?;
        let base = Uri::try_from(&server.url).map_err(|source| LoadError::ServerUrl {
            url: server.url.clone(),
            source,
        })?;
        let empty = Components::default();
        // Which segment groups this document is a fact about all of its paths,
        // so it is read once and then applied one path at a time.
        let paths: Vec<&str> = doc.paths.paths.keys().map(String::as_str).collect();
        let whole = Reading {
            components: doc.components.as_ref().unwrap_or(&empty),
            grouping: Grouping::of(&paths),
        };

        let mut ops: Vec<Operation> = Vec::new();
        for (path, item) in &doc.paths.paths {
            let item = item.as_item().ok_or_else(|| RefError {
                reference: format!("paths[{path}]"),
            })?;
            for (method, op) in item.iter() {
                let params = item.parameters.iter().chain(op.parameters.iter());
                ops.push(Operation::build(path, method, op, params, whole)?);
            }
        }
        if let Some(collision) = first_collision(&ops) {
            return Err(collision);
        }
        Ok(Self { base, ops })
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
    #[cfg(feature = "document")]
    fn build<'d>(
        path: &str,
        method: &str,
        op: &openapiv3::Operation,
        params: impl Iterator<Item = &'d ReferenceOr<Parameter>>,
        whole: Reading<'_>,
    ) -> Result<Self, LoadError> {
        let id = op
            .operation_id
            .as_deref()
            .ok_or_else(|| LoadError::NoOperationId {
                method: method.to_owned(),
                path: path.to_owned(),
            })?;
        let method = method_of(method);
        let (group, command) = placement(op, id, path, &method, whole.grouping)?;

        // One namespace per subcommand: the gate's own flags are claimed first.
        let mut flags =
            Namespace::with_reserved([COMMIT, JSON_BODY, RAW_BODY, FILE_PART, FIELD_PART]);
        let params = params
            .map(|p| Param::build(id, p, whole.components, &mut flags))
            .collect::<Result<Vec<_>, _>>()?;
        let body = Body::build(id, op, whole.components, &mut flags)?;
        let effect = effect_of(&method, op);

        Ok(Self {
            id: id.to_owned(),
            group,
            command,
            method,
            path: path.to_owned(),
            summary: op.summary.clone(),
            description: op.description.clone(),
            params,
            body,
            effect,
        })
    }

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

    /// The parameter the document spells `name`, if there is one.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }
}

/// What the whole document supplies while one of its operations is read: the
/// schemas every `$ref` resolves against, and the rule that places operations
/// in the command tree. Both are facts about the document rather than about
/// the operation, so both are read once and handed down.
#[cfg(feature = "document")]
#[derive(Debug, Clone, Copy)]
struct Reading<'d> {
    components: &'d Components,
    grouping: Grouping,
}

/// Where an operation sits in the command tree: the grouping rule, with the
/// document's own overrides over it.
///
/// `x-cli-group` and `x-cli-command` are the adopter's say over a name a path
/// spells badly, and the only way out of a collision — so they are read here,
/// where the name is decided, and nowhere else.
#[cfg(feature = "document")]
fn placement(
    op: &openapiv3::Operation,
    id: &str,
    path: &str,
    method: &Method,
    grouping: Grouping,
) -> Result<(CommandName, CommandName), LoadError> {
    let group = match named(op, id, GROUP)? {
        Some(raw) => CommandName::new(GROUP, raw)?,
        None => grouping.group(path)?,
    };
    let command = match named(op, id, COMMAND)? {
        Some(raw) => CommandName::new(COMMAND, raw)?,
        None => grouping.leaf(path, method)?,
    };
    Ok((group, command))
}

/// One `x-cli-` name the document offers, if it offers one.
///
/// A marker that is present and is not a string is the document saying
/// something this crate has no reading for, and is refused rather than passed
/// over — an adopter who writes a list where a name goes would otherwise get
/// the name they were overriding.
#[cfg(feature = "document")]
fn named<'o>(
    op: &'o openapiv3::Operation,
    id: &str,
    key: &'static str,
) -> Result<Option<&'o str>, LoadError> {
    match op.extensions.get(key) {
        None => Ok(None),
        Some(serde_json::Value::String(raw)) => Ok(Some(raw)),
        Some(_) => Err(LoadError::Override {
            op: id.to_owned(),
            key,
        }),
    }
}

/// Two operations under one `<group> <command>` would silently shadow each
/// other, so the document is refused instead — never resolved by renaming one
/// of them, which would move a name nobody asked to move.
#[cfg(feature = "document")]
fn first_collision(ops: &[Operation]) -> Option<LoadError> {
    ops.iter().enumerate().find_map(|(index, op)| {
        let later = ops
            .get(index + 1..)?
            .iter()
            .find(|later| later.group == op.group && later.command == op.command)?;
        Some(LoadError::DuplicateCommand {
            group: op.group.clone(),
            command: op.command.clone(),
            first: op.id.clone(),
            second: later.id.clone(),
        })
    })
}

/// `PathItem::iter` yields only the eight methods OpenAPI names, lowercase.
#[cfg(feature = "document")]
fn method_of(name: &str) -> Method {
    match name {
        "put" => Method::PUT,
        "post" => Method::POST,
        "delete" => Method::DELETE,
        "options" => Method::OPTIONS,
        "head" => Method::HEAD,
        "patch" => Method::PATCH,
        "trace" => Method::TRACE,
        _ => Method::GET,
    }
}

/// A GET the document marks as writing is a write; so is anything but a safe
/// method. Default-closed: the marker can only add writes, never remove them.
#[cfg(feature = "document")]
fn effect_of(method: &Method, op: &openapiv3::Operation) -> Effect {
    if op.extensions.get(WRITES) == Some(&serde_json::Value::Bool(true)) {
        return Effect::Write;
    }
    match *method {
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE => Effect::Read,
        _ => Effect::Write,
    }
}

impl Param {
    #[cfg(feature = "document")]
    fn build(
        op: &str,
        param: &ReferenceOr<Parameter>,
        components: &Components,
        flags: &mut Namespace,
    ) -> Result<Self, LoadError> {
        let param = resolve(param, |key| components.parameters.get(key), "parameters")?;
        let data = param.parameter_data_ref();
        let shape = shape_of(op, param, data, components, flags)?;
        if let Shape::Unreachable(why) = &shape
            && data.required
        {
            return Err(LoadError::Parameter {
                op: op.to_owned(),
                name: data.name.clone(),
                why: why.clone(),
            });
        }
        Ok(Self {
            name: data.name.clone(),
            required: data.required,
            shape,
            description: data.description.clone(),
        })
    }

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

    #[must_use]
    pub fn required(&self) -> bool {
        self.required
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// What one parameter is worth on a command line, and the flag it claims when it
/// is worth one.
///
/// Every way of not being worth a flag lands in the same place. The blast radius
/// of a shape this CLI cannot spell is the operation that declares it, never the
/// document that holds it.
#[cfg(feature = "document")]
fn shape_of(
    op: &str,
    param: &Parameter,
    data: &ParameterData,
    components: &Components,
    flags: &mut Namespace,
) -> Result<Shape, LoadError> {
    let Some((location, style)) = sent_in(param) else {
        return Ok(Shape::Unreachable(Unsupported::Cookie));
    };
    let ParameterSchemaOrContent::Schema(schema) = &data.format else {
        return Ok(Shape::Unreachable(Unsupported::Encoded));
    };
    // One value, or a list of them: the schema is asked first, and an array is
    // asked again about its items.
    let (scalar, repeats) = if let Some(scalar) = scalar_of(schema, components)? {
        (scalar, false)
    } else if let Some(scalar) = items_of(schema, components)? {
        (scalar, true)
    } else {
        return Ok(Shape::Unreachable(Unsupported::Structured));
    };
    runnable(&scalar, op, &data.name)?;
    // The style is read whatever the schema is, because it is not only about
    // delimiters: `matrix` puts a `;name=` in front of one path value as surely
    // as in front of a list. A style written out as `form` instead would be a
    // request in a shape the server does not read.
    let join = match style {
        Ok(join) => join,
        Err(style) => return Ok(Shape::Unreachable(Unsupported::Style(style.to_owned()))),
    };
    Ok(Shape::Flag {
        flag: flags.claim(&kebab(&data.name), "param"),
        location,
        scalar,
        join: repeats.then_some(join),
    })
}

/// Where a parameter goes, and how a list of its values would reach it there.
///
/// One answer, because the second is read off the first: `style` means different
/// things in a query and in a path, and `in: cookie` has no answer at all. The
/// `Err` carries the document's own spelling of a style this crate does not
/// write, so that whatever refuses it can name it — `form` in a query and
/// `simple` everywhere else are the two it writes, and they are also the two
/// OpenAPI defaults, so a document that says nothing lands on them.
#[cfg(feature = "document")]
fn sent_in(param: &Parameter) -> Option<(Location, Result<Join, &'static str>)> {
    Some(match param {
        Parameter::Query {
            parameter_data,
            style,
            ..
        } => (Location::Query, query_join(style, parameter_data.explode)),
        Parameter::Path { style, .. } => (
            Location::Path,
            match style {
                PathStyle::Simple => Ok(Join::Commas),
                PathStyle::Matrix => Err("matrix"),
                PathStyle::Label => Err("label"),
            },
        ),
        // `simple` is the only style a header has, and a list under it is
        // comma-separated whether or not it explodes.
        Parameter::Header { .. } => (Location::Header, Ok(Join::Commas)),
        Parameter::Cookie { .. } => return None,
    })
}

/// `form` is what a query parameter defaults to and `explode: true` is what
/// `form` defaults to, which is one field per value. The three other styles name
/// themselves rather than being written as `form`.
#[cfg(feature = "document")]
fn query_join(style: &QueryStyle, explode: Option<bool>) -> Result<Join, &'static str> {
    match style {
        QueryStyle::Form => Ok(match explode {
            Some(false) => Join::Commas,
            None | Some(true) => Join::Pairs,
        }),
        QueryStyle::SpaceDelimited => Err("spaceDelimited"),
        QueryStyle::PipeDelimited => Err("pipeDelimited"),
        QueryStyle::DeepObject => Err("deepObject"),
    }
}

/// The scalar a `type: array` parameter's items are, when its items are one.
#[cfg(feature = "document")]
fn items_of(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<Option<Scalar>, RefError> {
    let schema = resolve_schema(schema, components)?;
    let SchemaKind::Type(openapiv3::Type::Array(array)) = &schema.schema_kind else {
        return Ok(None);
    };
    let Some(items) = &array.items else {
        return Ok(None);
    };
    scalar_of(&items.clone().unbox(), components)
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

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

impl Body {
    #[cfg(feature = "document")]
    fn build(
        id: &str,
        op: &openapiv3::Operation,
        components: &Components,
        flags: &mut Namespace,
    ) -> Result<Self, LoadError> {
        let Some(body) = &op.request_body else {
            return Ok(Self::None);
        };
        let body = resolve(
            body,
            |key| components.request_bodies.get(key),
            "requestBodies",
        )?;
        let required = body.required;
        // The JSON entry if the document offers one, else whatever it offers
        // first: a vendor who misspells `multipart/form-data` lands here.
        let entry = body
            .content
            .iter()
            .find(|(name, _)| is_json(name))
            .or_else(|| body.content.iter().next());
        let Some((media_type, media)) = entry else {
            return Ok(Self::None);
        };
        if is_multipart(media_type) {
            return Ok(Self::Multipart {
                names: part_names(media, components),
                required,
            });
        }
        if !is_json(media_type) {
            return Ok(Self::Opaque {
                media_type: media_type.clone(),
                required,
            });
        }
        let Some(schema) = &media.schema else {
            return Ok(Self::JsonWhole { required });
        };
        let schema = resolve_schema(schema, components)?;
        let SchemaKind::Type(openapiv3::Type::Object(object)) = &schema.schema_kind else {
            return Ok(Self::JsonWhole { required });
        };

        let mut fields = Vec::with_capacity(object.properties.len());
        for (name, property) in &object.properties {
            let property = property.clone().unbox();
            let Some(scalar) = scalar_of(&property, components)? else {
                // One nested property is enough: the whole body goes through
                // `--json-body`, and no sibling gets a flag the request builder
                // would then throw away.
                return Ok(Self::JsonWhole { required });
            };
            runnable(&scalar, id, name)?;
            let described = resolve_schema(&property, components)?;
            fields.push(Field {
                flag: flags.claim(&kebab(name), "body"),
                name: name.clone(),
                required: required && object.required.iter().any(|r| r == name),
                scalar,
                description: described.schema_data.description.clone(),
            });
        }
        Ok(Self::JsonFields(fields))
    }
}

/// Refuse a rule that cannot be run, naming the operation and the value it was
/// stated about.
///
/// A `pattern` the engine cannot read refuses every value, so a document that
/// states one describes a flag nothing can satisfy. Saying so while the
/// document is reduced is what keeps that a bless-time failure rather than a
/// user's.
#[cfg(feature = "document")]
fn runnable(scalar: &Scalar, op: &str, name: &str) -> Result<(), LoadError> {
    scalar.runnable().map_err(|source| LoadError::Unrunnable {
        op: op.to_owned(),
        name: name.to_owned(),
        source,
    })
}

/// The part names a multipart schema declares, in document order.
#[cfg(feature = "document")]
fn part_names(media: &openapiv3::MediaType, components: &Components) -> Vec<String> {
    let Some(schema) = &media.schema else {
        return Vec::new();
    };
    let Ok(schema) = resolve_schema(schema, components) else {
        return Vec::new();
    };
    let SchemaKind::Type(openapiv3::Type::Object(object)) = &schema.schema_kind else {
        return Vec::new();
    };
    object.properties.keys().cloned().collect()
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
