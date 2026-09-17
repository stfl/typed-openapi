//! Reading an OpenAPI document into the model it reduces to.
//!
//! *Requires the `document` feature.*
//!
//! [`Document::load`] takes the two files an adopter ships — the vendor's
//! OpenAPI document and their Overlay of corrections — and hands back a list
//! of [`Operation`]s with every `$ref` already resolved. Nothing from
//! `openapiv3` escapes: once `load` returns, the parsed document is dropped,
//! and what is left is the model its parent module holds, which every build
//! has.
//!
//! The reading is a module rather than a set of gated items among those types
//! because one `#[cfg]` on the module declaration is the whole statement: the
//! feature adds this file whole and removes it whole, which is the shape every
//! feature in this crate has. An attribute per item says the same thing many
//! times over, and leaves a reader checking that no two of them disagree.
//!
//! [`LoadError`] is here because it is a failure of the reading and not of the
//! model — so the feature that brings the one brings the other, and a binary
//! that starts from `Document::from_blob` does not have to know the list
//! exists.

use std::borrow::Cow;

use http::{Method, Uri};
use openapiv3::{
    Components, OpenAPI, Parameter, ParameterData, ParameterSchemaOrContent, PathStyle, QueryStyle,
    ReferenceOr, Schema, SchemaKind,
};
use thiserror::Error;

use super::{
    Body, COMMIT, Document, Effect, FIELD_PART, FILE_PART, Field, Gate, JSON_BODY,
    JSON_BODY_TEMPLATE, Join, Location, Operation, Param, RAW_BODY, Shape, Unsupported,
};
use crate::names::{Clash, CommandName, Grouping, NameError, Namespace, Protected, spelled};
use crate::scalar::Scalar;
use crate::schema::{
    RefError, description_of, format_of, is_json, is_media_type, is_multipart, resolve,
    resolve_schema, scalar_of, template,
};

/// The four extensions this crate reads, all of them an adopter's say over
/// something the document alone cannot settle. An Overlay is where they are
/// written.
///
/// HTTP cannot say "this GET writes", so the document has to.
const WRITES: &str = "x-cli-writes";
/// The command name to mount an operation under, where the path spells one
/// badly — or where two operations reduce to the same name.
const COMMAND: &str = "x-cli-command";
/// The group to mount an operation under, where the path's own segment is not
/// the resource the operation belongs to.
const GROUP: &str = "x-cli-group";
/// The hazards an operation stands behind by name, beside the write gate
/// itself. One list rather than one marker per name, so a gate an adoption
/// invents costs an Overlay line and not a release of this crate.
const GATES: &str = "x-cli-gates";
/// The other parameters a parameter needs beside it, by wire name. OpenAPI
/// cannot say that two parameters only mean something together, so the marker
/// sits on the parameter that needs the other.
const REQUIRES: &str = "x-cli-requires";

/// The five flags that carry a request's body, spent by every subcommand
/// before the document has a say.
///
/// A parameter or a body field that wants one of these moves aside for it, and
/// a gate that names one is refused. Nothing here is a person saying yes, which
/// is why these yield where a gate and the confirmation do not: a
/// `--body-json-body` is ugly and harmless, where a confirmation nobody typed
/// is neither.
///
/// Spending a word here changes the reduced model of any document that declares
/// a field spelled the same way, which is the point: the field moves aside at
/// bless time, where a reviewer sees it, rather than shadowing a flag the CLI
/// needs.
const TRANSPORT: [&str; 5] = [
    JSON_BODY,
    JSON_BODY_TEMPLATE,
    RAW_BODY,
    FILE_PART,
    FIELD_PART,
];

/// What a refusal calls the request body.
///
/// An operation's values are the parameters and the body, and the document names
/// every one of them but the body — so the body is named by the key it is
/// written under, which is the word an adopter goes looking for.
const BODY: &str = "requestBody";

/// Reducing an OpenAPI document to [`Operation`]s failed.
///
/// *Requires the `document` feature.* Every variant names a way that reading a
/// document goes wrong, so the whole type is bless-time: a binary that starts
/// from [`Document::from_blob`] cannot produce one, does not compile one, and
/// does not have to know the list exists.
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
    #[error("{op}: `{key}` is not a list of names")]
    GateList { op: String, key: &'static str },
    #[error("{op}: the gate `{gate}` is one of the flags every subcommand already spends")]
    ReservedGate { op: String, gate: Gate },
    /// A gate spelled like the confirmation. Both are words a person types to
    /// let something happen, and one flag cannot be two of them: answering the
    /// gate would answer the confirmation, which is the one thing a gate is
    /// there to make impossible.
    ///
    /// Either word may move, so the message names both doors.
    #[error(
        "{op}: the gate `{gate}` is the word this CLI confirms with; \
         rename the gate in `x-cli-gates`, or confirm with another word"
    )]
    GateIsTheConfirmation { op: String, gate: Gate },
    #[error("{op}: the gate `{gate}` is named twice")]
    DuplicateGate { op: String, gate: Gate },
    /// A document name that wanted the word a gate already spends. Refused
    /// rather than renamed: a flag carrying data must never be able to answer
    /// a gate, and a gate quietly moved aside is a hazard nobody typed.
    ///
    /// The gate's spelling belongs to the adopter's correction layer, so that
    /// is where the way out is — and the message names it, because the person
    /// reading this refusal is the person who wrote the layer.
    #[error(
        "{op}: the {what} `{name}` and the gate `{gate}` both want `--{gate}`; \
         rename the gate in `x-cli-gates`, to `gate-{gate}` or another word"
    )]
    GateTakenByName {
        op: String,
        what: &'static str,
        name: String,
        gate: String,
    },
    /// The same, for the confirmation. Its spelling is the adopter's own —
    /// chosen when the document is loaded — so the way out is to choose
    /// another one rather than to edit the document.
    #[error(
        "{op}: the {what} `{name}` and the confirmation both want `--{commit}`; \
         confirm with another word — `Loading::commit`, or `Settings::commit_word` \
         where a bless step generates"
    )]
    CommitTakenByName {
        op: String,
        what: &'static str,
        name: String,
        commit: String,
    },
    /// A read that names a gate is a document saying two things at once: a read
    /// is sent on sight, so there is nothing for the gate to hold back. Refused
    /// rather than mounted, because one of the two statements is a mistake and
    /// the document does not say which.
    #[error(
        "{op}: a read stands behind no gate, and this one names `{gate}`; \
         mark the operation `x-cli-writes: true` or drop the gate"
    )]
    GatedRead { op: String, gate: Gate },
    /// A `required` naming a key nothing describes, which is a mistake in the
    /// document — and the document reduced here is the vendor's with the
    /// adopter's Overlays applied, the one that has to be right. Refused rather
    /// than reduced, because what a generator makes of it is a required field
    /// of no stated type: the generated type then refuses the vendor's own
    /// responses, nothing downstream checks a value for the key, and the
    /// request goes out without the one its caller was told to send. The repair
    /// is a judgement about what the vendor meant, and a judgement belongs in
    /// an Overlay a reviewer can read.
    #[error(transparent)]
    Phantom(#[from] crate::required::PhantomKeys),
    /// A `$ref` met where there is no value yet to name it beside: one standing
    /// in for a whole path item, or for a whole parameter object.
    #[error(transparent)]
    Reference(#[from] RefError),
    /// The same failure met under one value of one operation, which is where
    /// almost all of them are met.
    ///
    /// Named rather than reported bare, because the invariant every other
    /// refusal here keeps is that a shape the reduction cannot read is refused
    /// *by name*: a document of a thousand operations is not searchable by the
    /// reference alone, and an adopter meeting one is being asked to go and
    /// correct it in an Overlay.
    #[error("{op}: `{name}`: {source}")]
    Unresolvable {
        op: String,
        name: String,
        #[source]
        source: RefError,
    },
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
    /// A `requestBody` whose `content` key names no media type — `form-data`
    /// where `multipart/form-data` was meant. Refused while the document is
    /// reduced, because the alternative is a request sent under a
    /// `Content-Type` no server can read.
    #[error(
        "{op}: `{media_type}` is not a media type; \
         an Overlay is where a document's content type is corrected"
    )]
    MediaType { op: String, media_type: String },
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
    /// An `x-cli-requires` that is present and is not a list of names. Refused
    /// rather than passed over, for the reason a gate list is: one name written
    /// where a list goes would otherwise be no requirement at all.
    #[error("{op}: `{REQUIRES}` on `{name}` is not a list of parameter names")]
    RequiresList { op: String, name: String },
    /// A parameter that requires a name the operation declares no parameter
    /// under. Every run giving it would be refused, and the spelling that was
    /// meant is the adopter's to correct.
    #[error("{op}: `{name}` requires `{requires}`, which is not a parameter of this operation")]
    RequiresUnknown {
        op: String,
        name: String,
        requires: String,
    },
    /// A parameter that requires one this crate cannot put in a request, so no
    /// request giving the first could ever be built.
    #[error("{op}: `{name}` requires `{requires}`, and `{requires}` is {why}")]
    RequiresUnreachable {
        op: String,
        name: String,
        requires: String,
        why: Unsupported,
    },
}

/// `--help`, which clap declares on every subcommand whether or not anyone
/// asked. Nothing here can move it, so it is a name the confirmation may not
/// take.
const HELP: &str = "help";

/// Why a word cannot be the confirmation.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ConfirmationError {
    /// Not a name a person could type as a flag.
    #[error(transparent)]
    Spelling(#[from] NameError),
    /// A name some other flag on the same subcommand already wears. The others
    /// cannot move — a body has to be reachable and `--help` is clap's — so
    /// the confirmation is what gives way.
    #[error(
        "`{word}` is a flag every subcommand already declares, \
         so it cannot also be the word this CLI confirms with"
    )]
    Taken {
        /// The word both of them wanted.
        word: String,
    },
}

/// What the CLI brings to a reduction, as against what the document brings.
///
/// A type rather than a bare argument, so that a caller reading
/// `Loading::new().commit("yes")?` at the call site can see which of the two
/// the word belongs to.
#[derive(Debug, Clone)]
pub struct Loading {
    commit: String,
}

impl Default for Loading {
    fn default() -> Self {
        Self::new()
    }
}

impl Loading {
    /// The defaults: `--commit` confirms a write.
    #[must_use]
    pub fn new() -> Self {
        Self {
            commit: COMMIT.to_owned(),
        }
    }

    /// Confirm writes with this word instead, spelled without the `--`.
    ///
    /// Held to the one spelling rule every flag is held to, and refused rather
    /// than mangled: a confirmation somebody cannot type is a confirmation
    /// nobody gives.
    ///
    /// Refused too where the word is one a subcommand already declares — a
    /// flag that carries the body, or the `--help` clap writes itself. Those
    /// cannot move, so a confirmation wearing one of their names is two flags
    /// with one spelling, which clap resolves at the user's startup and not at
    /// the adopter's bless. The same words are refused for a gate, in
    /// `gates_of`, and for the same reason.
    pub fn commit(mut self, word: &str) -> Result<Self, ConfirmationError> {
        let word = spelled("confirmation", word)?;
        if TRANSPORT.contains(&word.as_str()) || word == HELP {
            return Err(ConfirmationError::Taken { word });
        }
        self.commit = word;
        Ok(self)
    }
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
    ///     include_str!("../../tests/fixtures/toy.yaml"),
    ///     &[
    ///         include_str!("../../tests/fixtures/corrections.yaml"),
    ///         include_str!("../../tests/fixtures/cli.yaml"),
    ///     ],
    /// )?;
    /// let op = doc.get("getVoucher").expect("the document describes it");
    /// let request = Invocation::new(op, Values::new().param("id", 5))?.request(doc.base())?;
    /// assert_eq!(request.uri().path(), "/vouchers/5");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn load(document: &str, overlays: &[&str]) -> Result<Self, LoadError> {
        Self::load_with(document, overlays, &Loading::new())
    }

    /// The same, with the CLI's own words chosen rather than defaulted.
    ///
    /// *Requires the `document` feature.*
    ///
    /// The one word this door adds is the confirmation. It is settable because
    /// it is this crate's and not the vendor's: a document whose own schema
    /// declares a property called `commit` has every right to, and the way out
    /// is for the CLI to say yes in a different word rather than for the
    /// document's property to be renamed behind its author's back.
    ///
    /// ```
    /// use typed_openapi::{Document, Loading};
    ///
    /// let doc = Document::load_with(
    ///     include_str!("../../tests/fixtures/toy.yaml"),
    ///     &[
    ///         include_str!("../../tests/fixtures/corrections.yaml"),
    ///         include_str!("../../tests/fixtures/cli.yaml"),
    ///     ],
    ///     &Loading::new().commit("yes")?,
    /// )?;
    /// let op = doc
    ///     .get("enshrineVoucher")
    ///     .expect("the document describes it");
    /// assert_eq!(op.commit(), "yes");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn load_with(
        document: &str,
        overlays: &[&str],
        loading: &Loading,
    ) -> Result<Self, LoadError> {
        let mut doc = crate::overlay::parse(document)?;
        for overlay in overlays {
            doc = crate::overlay::apply(doc, overlay)?;
        }
        // Against the corrected document, so that an adopter's own Overlay is
        // what repairs a contradiction, and while it is still the text the
        // vendor wrote: the object model below normalises away shapes — an
        // inline body's schema among them — that this reading needs to see.
        crate::required::check(&doc)?;
        let doc: OpenAPI = serde_json::from_value(doc).map_err(LoadError::Shape)?;
        Self::from_openapi(&doc, &loading.commit)
    }

    fn from_openapi(doc: &OpenAPI, commit: &str) -> Result<Self, LoadError> {
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
            commit,
        };

        let mut ops: Vec<Operation> = Vec::new();
        for (path, item) in &doc.paths.paths {
            let item = item.as_item().ok_or_else(|| RefError::Missing {
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
        Ok(Self {
            base,
            commit: commit.to_owned(),
            ops,
        })
    }
}

impl Operation {
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
        let effect = effect_of(&method, op);
        // The gates are read before the namespace exists, because their flags
        // belong in it: a body field the vendor happens to spell `enshrine`
        // moves aside rather than shadowing the word that stands in front of
        // the hazard.
        let gates = gates_of(op, id, effect, whole.commit)?;

        // One namespace per subcommand. The words that carry consent — this
        // operation's gates and the confirmation — are guarded, so a document
        // name that wants one refuses the load instead of pushing it aside.
        // The flags that carry the body are merely spent, and move over.
        //
        // The confirmation is guarded on a write and nowhere else, because a
        // read declares no confirmation to clash with: a document is free to
        // call a query parameter `commit`, and an operation sent on sight has
        // no word standing in front of it. A gate is refused on a read
        // earlier still, and for the same reason.
        let confirmation = (effect == Effect::Write).then_some((whole.commit, Protected::Commit));
        let mut flags = Namespace::guarding(
            gates
                .iter()
                .map(|gate| (gate.as_str(), Protected::Gate))
                .chain(confirmation),
            TRANSPORT,
        );
        let params = params
            .map(|p| Param::build(id, p, whole.components, &mut flags))
            .collect::<Result<Vec<_>, _>>()?;
        companions(id, &params)?;
        let body = Body::build(id, op, whole.components, &mut flags)?;

        Ok(Self {
            id: id.to_owned(),
            group,
            command,
            commit: whole.commit.to_owned(),
            method,
            path: path.to_owned(),
            summary: op.summary.clone(),
            description: op.description.clone(),
            params,
            body,
            effect,
            gates,
        })
    }
}

/// What the whole document supplies while one of its operations is read: the
/// schemas every `$ref` resolves against, and the rule that places operations
/// in the command tree. Both are facts about the document rather than about
/// the operation, so both are read once and handed down.
#[derive(Debug, Clone, Copy)]
struct Reading<'d> {
    components: &'d Components,
    grouping: Grouping,
    /// The word this CLI spends on confirming a write. Read once here and then
    /// written onto the document and onto every operation: a subcommand is
    /// built from one operation with no document in reach, so the copy is what
    /// lets `tree` declare the flag the adopter chose.
    commit: &'d str,
}

/// Where an operation sits in the command tree: the grouping rule, with the
/// document's own overrides over it.
///
/// `x-cli-group` and `x-cli-command` are the adopter's say over a name a path
/// spells badly, and the only way out of a collision — so they are read here,
/// where the name is decided, and nowhere else.
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

/// The gates one operation stands behind, in the order the document names them.
///
/// Every rule about a gate is run here, while the document is reduced: a name
/// that is not a flag, a name the command line has already spent, a name given
/// twice, and a gate on an operation that is sent on sight. All four are an
/// adopter's mistake, and all four are refused at their expense rather than at
/// a user's — a gate that reaches a shipped binary is a gate somebody is about
/// to type.
fn gates_of(
    op: &openapiv3::Operation,
    id: &str,
    effect: Effect,
    commit: &str,
) -> Result<Vec<Gate>, LoadError> {
    let mut gates: Vec<Gate> = Vec::new();
    for raw in listed(op, id, GATES)? {
        let gate = Gate::new(GATES, raw)?;
        if gate.as_str() == commit {
            return Err(LoadError::GateIsTheConfirmation {
                op: id.to_owned(),
                gate,
            });
        }
        if TRANSPORT.contains(&gate.as_str()) {
            return Err(LoadError::ReservedGate {
                op: id.to_owned(),
                gate,
            });
        }
        if gates.contains(&gate) {
            return Err(LoadError::DuplicateGate {
                op: id.to_owned(),
                gate,
            });
        }
        gates.push(gate);
    }
    match (effect, gates.first()) {
        (Effect::Read, Some(gate)) => Err(LoadError::GatedRead {
            op: id.to_owned(),
            gate: gate.clone(),
        }),
        _ => Ok(gates),
    }
}

/// The names one list-valued `x-cli-` marker offers, if it offers any.
///
/// A marker that is present and is not a list of names is the document saying
/// something this crate has no reading for, and is refused rather than passed
/// over — the same reading [`named`] gives a marker that should have been one
/// name, for the same reason: an adopter who writes one word where a list goes
/// would otherwise get no gate at all.
fn listed<'o>(
    op: &'o openapiv3::Operation,
    id: &str,
    key: &'static str,
) -> Result<Vec<&'o str>, LoadError> {
    let reject = || LoadError::GateList {
        op: id.to_owned(),
        key,
    };
    match op.extensions.get(key) {
        None => Ok(Vec::new()),
        Some(serde_json::Value::Array(names)) => names
            .iter()
            .map(|name| name.as_str().ok_or_else(reject))
            .collect(),
        Some(_) => Err(reject()),
    }
}

/// Two operations under one `<group> <command>` would silently shadow each
/// other, so the document is refused instead — never resolved by renaming one
/// of them, which would move a name nobody asked to move.
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
            requires: requires_of(op, data)?,
        })
    }
}

/// The names one parameter's `x-cli-requires` lists, if it lists any.
fn requires_of(op: &str, data: &ParameterData) -> Result<Vec<String>, LoadError> {
    let reject = || LoadError::RequiresList {
        op: op.to_owned(),
        name: data.name.clone(),
    };
    match data.extensions.get(REQUIRES) {
        None => Ok(Vec::new()),
        Some(serde_json::Value::Array(names)) => names
            .iter()
            .map(|name| name.as_str().map(str::to_owned).ok_or_else(reject))
            .collect(),
        Some(_) => Err(reject()),
    }
}

/// Every requirement one operation's parameters state names a parameter of the
/// same operation that a request can carry.
///
/// Checked once every parameter is read, because a requirement may name one the
/// document lists after it. A name that is not there, or one this crate cannot
/// send, is a requirement no run could meet, so it is refused here rather than
/// mounted as a flag that refuses everyone who gives it.
fn companions(op: &str, params: &[Param]) -> Result<(), LoadError> {
    for param in params {
        for requires in param.requires() {
            let Some(companion) = params.iter().find(|other| other.name() == requires) else {
                return Err(LoadError::RequiresUnknown {
                    op: op.to_owned(),
                    name: param.name().to_owned(),
                    requires: requires.clone(),
                });
            };
            if let Shape::Unreachable(why) = companion.shape() {
                return Err(LoadError::RequiresUnreachable {
                    op: op.to_owned(),
                    name: param.name().to_owned(),
                    requires: requires.clone(),
                    why: why.clone(),
                });
            }
        }
    }
    Ok(())
}

/// What one parameter is worth on a command line, and the flag it claims when it
/// is worth one.
///
/// Every way of not being worth a flag lands in the same place. The blast radius
/// of a shape this CLI cannot spell is the operation that declares it, never the
/// document that holds it.
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
    // One value, or a list of them: an array is asked about its items, and
    // whichever schema describes the value is the one everything about that
    // value is read off — so a list's rules and its declared kind are its
    // items', which is where a list of days says that a day is what it holds.
    let (value, repeats) = match under(items_of(schema, components), op, &data.name)? {
        Some(items) => (Cow::Owned(items), true),
        None => (Cow::Borrowed(schema), false),
    };
    let Some(scalar) = under(scalar_of(&value, components), op, &data.name)? else {
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
    // The name is asked last, so that a parameter this CLI could not have
    // supplied anyway is reported for what the document said about its value
    // rather than for how the vendor spelled it.
    let Ok(spelling) = spelled("parameter", &data.name) else {
        return Ok(Shape::Unreachable(Unsupported::Unspellable));
    };
    Ok(Shape::Flag {
        flag: flags
            .claim(&spelling, "param")
            .map_err(|clash| clashed(&clash, op, "parameter", &data.name))?,
        location,
        scalar,
        join: repeats.then_some(join),
        format: under(format_of(&value, components), op, &data.name)?,
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

/// The schema a `type: array` parameter's values are described by, when the
/// document says what its items are.
///
/// Everything a list parameter's values are — the rules they are held to and
/// the kind the document calls them — is stated here rather than on the array,
/// which only says how many of them there are.
fn items_of(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<Option<ReferenceOr<Schema>>, RefError> {
    let schema = resolve_schema(schema, components)?;
    let SchemaKind::Type(openapiv3::Type::Array(array)) = &schema.schema_kind else {
        return Ok(None);
    };
    Ok(array.items.clone().map(ReferenceOr::unbox))
}

impl Body {
    fn build(
        id: &str,
        op: &openapiv3::Operation,
        components: &Components,
        flags: &mut Namespace,
    ) -> Result<Self, LoadError> {
        let Some(body) = &op.request_body else {
            return Ok(Self::None);
        };
        let body = under(
            resolve(
                body,
                |key| components.request_bodies.get(key),
                "requestBodies",
            ),
            id,
            BODY,
        )?;
        let required = body.required;
        // The JSON entry if the document offers one, else whatever it offers
        // first.
        let entry = body
            .content
            .iter()
            .find(|(name, _)| is_json(name))
            .or_else(|| body.content.iter().next());
        let Some((media_type, media)) = entry else {
            return Ok(Self::None);
        };
        // The key this operation would be sent under, and only that one: a key
        // beside it that nothing here ever reads is the vendor's business, and
        // refusing over it would refuse documents this crate serves correctly.
        if !is_media_type(media_type) {
            return Err(LoadError::MediaType {
                op: id.to_owned(),
                media_type: media_type.clone(),
            });
        }
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
        json_body(id, media.schema.as_ref(), required, components, flags)
    }
}

/// What a JSON body is worth on a command line: one flag per property, or one
/// flag for the whole thing.
///
/// The same question [`shape_of`] asks of a parameter, and answered the same
/// way — everything a flag set needs lives on the variant that has it, so a
/// body cannot leave behind a flag the request builder would ignore.
///
/// A body that goes whole carries its skeleton, because going whole is exactly
/// what leaves the command line with nothing saying what the body wants. The
/// walk that decided it has already seen the shape.
fn json_body(
    id: &str,
    schema: Option<&ReferenceOr<Schema>>,
    required: bool,
    components: &Components,
    flags: &mut Namespace,
) -> Result<Body, LoadError> {
    let Some(schema) = schema else {
        return Ok(Body::JsonWhole {
            required,
            template: None,
        });
    };
    let whole = || {
        Ok(Body::JsonWhole {
            required,
            template: under(template(schema, components), id, BODY)?,
        })
    };
    let SchemaKind::Type(openapiv3::Type::Object(object)) =
        &under(resolve_schema(schema, components), id, BODY)?.schema_kind
    else {
        return whole();
    };

    let mut fields = Vec::with_capacity(object.properties.len());
    for (name, property) in &object.properties {
        let property = property.clone().unbox();
        let Some(scalar) = under(scalar_of(&property, components), id, name)? else {
            // One nested property is enough: the whole body goes through
            // `--json-body`, and no sibling gets a flag the request builder
            // would then throw away.
            return whole();
        };
        runnable(&scalar, id, name)?;
        // A property whose name has no kebab-case spelling has no flag either,
        // and the body's rule is about whether a property has one rather than
        // about why it has none — so it goes the way a nested property goes.
        // What that buys is the disclosure: the template `--json-body-template`
        // prints carries the key, where a flag called `""` names it nowhere.
        let Ok(spelling) = spelled("property", name) else {
            return whole();
        };
        fields.push(Field {
            flag: flags
                .claim(&spelling, "body")
                .map_err(|clash| clashed(&clash, id, "property", name))?,
            name: name.clone(),
            required: required && object.required.iter().any(|r| r == name),
            scalar,
            format: under(format_of(&property, components), id, name)?,
            description: under(description_of(&property, components), id, name)?,
        });
    }
    Ok(Body::JsonFields(fields))
}

/// A clash, said in the vocabulary of whoever can resolve it.
///
/// The two words are guarded for one reason and moved for two different ones,
/// so the refusal is two sentences rather than one with a branch in it: the
/// gate's spelling is in the document's correction layer, and the
/// confirmation's is in the call that loads or generates from it.
fn clashed(clash: &Clash, op: &str, what: &'static str, name: &str) -> LoadError {
    match clash.kind {
        Protected::Gate => LoadError::GateTakenByName {
            op: op.to_owned(),
            what,
            name: name.to_owned(),
            gate: clash.word.clone(),
        },
        Protected::Commit => LoadError::CommitTakenByName {
            op: op.to_owned(),
            what,
            name: name.to_owned(),
            commit: clash.word.clone(),
        },
    }
}

/// Read a schema on one value's behalf, so that a `$ref` under it is refused by
/// name.
///
/// Every `$ref` this reduction follows is reached through a parameter or a body
/// property, and the reference on its own is not something an adopter can find:
/// one `#/components/schemas/…` stands under a dozen operations, and only one of
/// them is the one to correct. So the reads under a value go through
/// here, which is why [`runnable`] stands beside it — both turn a failure about
/// a schema into a refusal saying which value stated it.
fn under<T>(read: Result<T, RefError>, op: &str, name: &str) -> Result<T, LoadError> {
    read.map_err(|source| LoadError::Unresolvable {
        op: op.to_owned(),
        name: name.to_owned(),
        source,
    })
}

/// Refuse a rule that cannot be run, naming the operation and the value it was
/// stated about.
///
/// A `pattern` the engine cannot read refuses every value, so a document that
/// states one describes a flag nothing can satisfy. Saying so while the
/// document is reduced is what keeps that a bless-time failure rather than a
/// user's.
fn runnable(scalar: &Scalar, op: &str, name: &str) -> Result<(), LoadError> {
    scalar.runnable().map_err(|source| LoadError::Unrunnable {
        op: op.to_owned(),
        name: name.to_owned(),
        source,
    })
}

/// The part names a multipart schema declares, in document order.
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
