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
    Components, OpenAPI, Parameter, ParameterSchemaOrContent, ReferenceOr, SchemaKind,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::names::{CommandName, renamed};
#[cfg(feature = "document")]
use crate::names::{IdError, Namespace, kebab};
use crate::scalar::Scalar;
#[cfg(feature = "document")]
use crate::schema::{RefError, is_json, is_multipart, resolve, resolve_schema, scalar_of};

/// The one extension this crate reads. HTTP cannot say "this GET writes", so
/// the document has to, and an Overlay is where an adopter says it.
#[cfg(feature = "document")]
const WRITES: &str = "x-cli-writes";

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

/// One operation: one subcommand, one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    id: String,
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

/// One path, query, or header parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    name: String,
    flag: String,
    location: Location,
    required: bool,
    scalar: Scalar,
    description: Option<String>,
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
    OperationId(#[from] IdError),
    #[error("two operations are both named `{0}` on the command line")]
    DuplicateCommand(CommandName),
    #[error(transparent)]
    Reference(#[from] RefError),
    #[error("{op}: parameter `{name}` is {reason}")]
    Parameter {
        op: String,
        name: String,
        reason: &'static str,
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
    /// Parse the vendor's document, apply the adopter's Overlay to it, and
    /// resolve the result into operations.
    ///
    /// *Requires the `document` feature.*
    ///
    /// Both arguments are file contents, YAML or JSON. Pass `""` as `overlay`
    /// to run a document that is already corrected.
    ///
    /// This is the expensive door, and the `document` feature is what opens it.
    /// A bless step calls it once and writes [`Document::to_blob`] beside the
    /// rest of what it generates; a shipped binary compiles without the feature
    /// and reaches the same reduction through [`Document::from_blob`].
    ///
    /// ```no_run
    /// use typed_openapi::{Document, Invocation, Values};
    ///
    /// let doc = Document::load(
    ///     include_str!("../../api-generated/spec/toy.overlaid.yaml"),
    ///     "",
    /// )?;
    /// let op = doc.get("getVoucher").expect("the document describes it");
    /// let request = Invocation::new(op, Values::new().param("id", 5))?.request(doc.base())?;
    /// assert_eq!(request.uri().path(), "/vouchers/5");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[cfg(feature = "document")]
    pub fn load(document: &str, overlay: &str) -> Result<Self, LoadError> {
        let doc = crate::overlay::apply(document, overlay)?;
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
        let components = doc.components.as_ref().unwrap_or(&empty);

        let mut ops: Vec<Operation> = Vec::new();
        for (path, item) in &doc.paths.paths {
            let item = item.as_item().ok_or_else(|| RefError {
                reference: format!("paths[{path}]"),
            })?;
            for (method, op) in item.iter() {
                let params = item.parameters.iter().chain(op.parameters.iter());
                ops.push(Operation::build(path, method, op, params, components)?);
            }
        }
        if let Some(duplicate) = first_duplicate(&ops) {
            return Err(LoadError::DuplicateCommand(duplicate));
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

    /// By subcommand name, as the user types it.
    #[must_use]
    pub fn by_command(&self, command: &str) -> Option<&Operation> {
        self.ops.iter().find(|op| op.command.as_str() == command)
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
        components: &Components,
    ) -> Result<Self, LoadError> {
        let id = op
            .operation_id
            .as_deref()
            .ok_or_else(|| LoadError::NoOperationId {
                method: method.to_owned(),
                path: path.to_owned(),
            })?;
        let command = CommandName::from_operation_id(id)?;
        let method = method_of(method);

        // One namespace per subcommand: the gate's own flags are claimed first.
        let mut flags =
            Namespace::with_reserved([COMMIT, JSON_BODY, RAW_BODY, FILE_PART, FIELD_PART]);
        let params = params
            .map(|p| Param::build(command.as_str(), p, components, &mut flags))
            .collect::<Result<Vec<_>, _>>()?;
        let body = Body::build(op, components, &mut flags)?;
        let effect = effect_of(&method, op);

        Ok(Self {
            id: id.to_owned(),
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

    /// The subcommand name, as the user types it.
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

/// Two operations under one subcommand name would silently shadow each other,
/// so the document is refused instead.
#[cfg(feature = "document")]
fn first_duplicate(ops: &[Operation]) -> Option<CommandName> {
    ops.iter().enumerate().find_map(|(index, op)| {
        ops.get(index + 1..)?
            .iter()
            .any(|later| later.command == op.command)
            .then(|| op.command.clone())
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
        let reject = |name: &str, reason: &'static str| LoadError::Parameter {
            op: op.to_owned(),
            name: name.to_owned(),
            reason,
        };
        let (location, data) = match param {
            Parameter::Path { parameter_data, .. } => (Location::Path, parameter_data),
            Parameter::Query { parameter_data, .. } => (Location::Query, parameter_data),
            Parameter::Header { parameter_data, .. } => (Location::Header, parameter_data),
            Parameter::Cookie { parameter_data, .. } => {
                return Err(reject(
                    &parameter_data.name,
                    "in: cookie, which this CLI does not send",
                ));
            }
        };
        let ParameterSchemaOrContent::Schema(schema) = &data.format else {
            return Err(reject(
                &data.name,
                "described by `content`, which this CLI does not encode",
            ));
        };
        let scalar = scalar_of(schema, components)?
            .ok_or_else(|| reject(&data.name, "not a scalar, so it cannot be one flag"))?;
        Ok(Self {
            flag: flags.claim(&kebab(&data.name), "param"),
            name: data.name.clone(),
            location,
            required: data.required,
            scalar,
            description: data.description.clone(),
        })
    }

    /// The wire name, as the document spells it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The flag name, without the leading `--`.
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
    pub fn location(&self) -> Location {
        self.location
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
