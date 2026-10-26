//! A typed client over a document, and the two steps every call is made of.
//!
//! [`Client`] holds one document and hands out [`Call`]s by `operationId`.
//! `Call` knows what its answer deserialises into, so sending is one expression
//! in each flavour and a caller never names a response type twice. A caller that
//! wants the gate a command line has plans the call with its [`Answers`] and
//! sends what it planned ([`Call::plan`], [`Planned`]).
//!
//! The four errors a call raises once it sends (`Status`, `Decode`, `Transport`
//! and `Encode`) name the operation. No message of theirs prints what the server
//! sent, and neither does the cause of a `Decode`: the body is a field of the
//! error, and [`Error::body`] reads it. The cause of a `Transport` is the
//! client's own error, which prints what that client prints. A `Request` is
//! raised before anything is sent and names no operation.
//!
//! Nothing here is specific to one API. A generated wrapper is the typed door
//! onto [`Client::call`]: it names the `operationId`, names the arguments under
//! the document's own names, and holds no path template, no query rule and no
//! encoder.

use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;

use http::{StatusCode, Uri};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::body_reader::{Read, Step, read};
use crate::model::{Document, DocumentError, DriftError, Operation};
use crate::plan::{Answers, Plan};
use crate::request::{Invocation, RequestError, ValueError, without_userinfo_and_query};
use crate::transport::{AsyncClient, HttpRequest, HttpResponse, SyncClient};
use crate::values::Values;

/// A response the document promises no body for.
///
/// It deserialises from anything, including an empty body, so an operation that
/// answers `201` with nothing is not a special case at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoContent;

impl<'de> serde::Deserialize<'de> for NoContent {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(de).map(|_| Self)
    }
}

/// Anything that stops a call short of a typed answer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The reduction a client is built from cannot be read. Generated code
    /// raises it when it reads the blob it ships.
    #[error("the document does not load")]
    Document(#[from] DocumentError),
    /// The generated inventory and the document it is paired with disagree.
    /// The message names the first row where they do.
    #[error(transparent)]
    Drift(#[from] DriftError),
    /// The values do not satisfy the operation. The message names the
    /// parameter, or says the body is the problem.
    #[error(transparent)]
    Values(#[from] ValueError),
    /// The values satisfied the operation and the request they render to could
    /// not be built: the base URL holds userinfo or a query, or the `http` crate
    /// refused what the document names, a URI or a header name. It is raised
    /// before anything is sent and names no operation.
    #[error(transparent)]
    Request(#[from] RequestError),
    /// A typed argument that does not serialise to JSON, so there is no body to
    /// send.
    #[error("{op}: cannot serialise the request body")]
    Encode {
        /// The `operationId` of the operation the argument is the body of.
        op: String,
        /// `serde_json`'s reading of the value.
        #[source]
        source: serde_json::Error,
    },
    /// The client refused the request or never got an answer, carrying the
    /// error the client itself returned.
    ///
    /// Boxed, so this type does not grow a parameter for the client. Whether
    /// the request left is a reading only an adapter can make, so it belongs
    /// above the seam, and a failure nobody has classified is one that may have
    /// arrived. The concrete error is here to be read: `downcast_ref` on the
    /// `source` recovers `C::Error`, and [`Call::request`] hands back the bytes
    /// for a caller who would rather send them through the client's own `send`
    /// and box nothing. `docs/client.md` shows both.
    #[error("{op}: the transport failed")]
    Transport {
        /// The `operationId` of the operation whose request was sent.
        op: String,
        /// The error the client returned.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The server answered with a status that is not a success. Nothing is
    /// deserialised out of such a response, whatever its body holds.
    ///
    /// The message names the operation and the status and nothing the server
    /// sent: the body is the server's, it can hold a credential or a client's
    /// data, and the message is printed where a log reads it. A caller that
    /// needs the body reads the field, or [`Error::body`].
    #[error("{op}: the server answered {status}")]
    Status {
        /// The `operationId` of the operation whose request was answered.
        op: String,
        /// The status the server answered.
        status: StatusCode,
        /// The response body as text, with bytes that are not UTF-8 replaced.
        body: String,
    },
    /// A success response whose body is not the type the document promises for
    /// it.
    ///
    /// The cause holds the kind of failure and where the read stopped, and no
    /// byte of the body: serde's own message quotes the value, the key or the
    /// variant it refused, and the chain of an error is printed where a log
    /// reads it. The text is in `raw`.
    #[error("{op}: the response is not the shape the document promises")]
    Decode {
        /// The `operationId` of the operation whose request was answered.
        op: String,
        /// What kind of failure it was, and the line and column where the read
        /// stopped.
        #[source]
        source: DecodeError,
        /// The response body as text, with bytes that are not UTF-8 replaced.
        raw: String,
    },
}

/// Where a response body stopped being the type the document promises, and
/// what kind of failure it was, and nothing the body said.
///
/// `serde_json` words its failure with the value, the key or the variant it
/// refused. This is what is left of that message once the bytes of the body are
/// taken out: the cause of an [`Error::Decode`], so a chain that a log prints
/// holds a fixed number of bytes whatever the server sent. The line and the
/// column are the reader's own, counted from 1; the text is [`Error::body`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("{class} at line {line} column {column}")]
#[non_exhaustive]
pub struct DecodeError {
    /// What kind of failure it was.
    pub class: DecodeClass,
    /// The line of the body where the read stopped.
    pub line: usize,
    /// The column of that line where the read stopped.
    pub column: usize,
}

impl DecodeError {
    /// What `serde_json` reports about its failure, without the message: it
    /// holds the bytes that the cause must not.
    pub(crate) fn of(error: &serde_json::Error) -> Self {
        use serde_json::error::Category;
        Self {
            class: match error.classify() {
                Category::Syntax => DecodeClass::Syntax,
                Category::Eof => DecodeClass::Eof,
                Category::Data => DecodeClass::Data,
                Category::Io => DecodeClass::Io,
            },
            line: error.line(),
            column: error.column(),
        }
    }
}

/// What kind of failure a [`DecodeError`] is, in the words that `serde_json`
/// classifies its own by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum DecodeClass {
    /// The body is not JSON.
    #[error("the body is not JSON")]
    Syntax,
    /// The body is JSON that ends before the value does.
    #[error("the body ends early")]
    Eof,
    /// The body is JSON and is not the type: a value of another type, a missing
    /// property, a property or a variant that the type does not take.
    #[error("the body does not fit the type")]
    Data,
    /// The reader failed. A body is a slice in memory, which cannot fail to be
    /// read; the variant is here because a reading of `serde_json`'s categories
    /// that left one out would have to panic.
    #[error("the body could not be read")]
    Io,
}

impl Error {
    /// What the server sent, for the two errors that hold a response: the body
    /// of a [`Error::Status`], and the text a [`Error::Decode`] could not read.
    /// Bytes that are not UTF-8 are replaced.
    ///
    /// It is a method and not part of any message or cause, because the body is
    /// the server's and the chain of an error is printed where a log reads it.
    #[must_use]
    pub fn body(&self) -> Option<&str> {
        match self {
            Self::Status { body, .. } => Some(body),
            Self::Decode { raw, .. } => Some(raw),
            Self::Document(_)
            | Self::Drift(_)
            | Self::Values(_)
            | Self::Request(_)
            | Self::Encode { .. }
            | Self::Transport { .. } => None,
        }
    }
}

/// One document, one server, and a typed way to call anything in it.
pub struct Client {
    document: Document,
    base: Uri,
}

/// The base is shown without its userinfo and its query: a caller names it, and
/// a `Debug` goes to logs.
impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("document", &self.document)
            .field(
                "base",
                &format_args!("{}", without_userinfo_and_query(&self.base)),
            )
            .finish()
    }
}

impl Client {
    /// The one constructor: a reduction that is already in hand.
    ///
    /// A shipped binary gets that reduction from `Document::from_blob`, which
    /// is all it can do — the reader is a cargo feature and the binary does not
    /// enable it. A bless step, which does, writes
    /// `Client::over(Document::load(document, overlay)?)` and handles the load
    /// failure under its own type, so no shape on this page depends on which
    /// build a reader is looking at.
    ///
    /// It cannot fail: the reduction is in hand, and the base is the one it
    /// carries.
    #[must_use]
    pub fn over(document: Document) -> Self {
        let base = document.base().clone();
        Self { document, base }
    }

    /// Point at a different server than the document's first one.
    #[must_use]
    pub fn with_base(mut self, base: Uri) -> Self {
        self.base = base;
        self
    }

    /// The server requests go to: the document's first one, unless
    /// [`Client::with_base`] named another.
    #[must_use]
    pub fn base(&self) -> &Uri {
        &self.base
    }

    /// The document itself, for a caller that wants the command tree or an
    /// operation no wrapper covers.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// An operation and the values for it.
    ///
    /// The operation is a reference into a document rather than a name to look
    /// up, so there is no "no such operation" to report here: the caller
    /// already holds one.
    pub fn call<'a, T>(&'a self, op: &'a Operation, values: Values) -> Result<Call<'a, T>, Error> {
        Ok(Call {
            invocation: Invocation::new(op, values)?,
            base: &self.base,
            response: PhantomData,
        })
    }
}

/// A request body that the type the document describes for it does not take.
///
/// This is what a `--json-body` file gets checked against. A refusal names the
/// operation and says which way the body is wrong, and the variants are apart
/// so that a caller can tell a wrong value from a property nobody declared.
/// `Undeclared` and `NotAnObject` print no value from the body: it can be a
/// credential or a client's data, and the refusal is printed where a log reads
/// it. The cause of a `Mismatch` is serde's own message, which can quote the
/// value it refused.
///
/// A path is in the `jq` spelling: `positions[1].flag`, or `.` for the body
/// itself. Serde's own message never says which element of which list held the
/// value, and serde says nothing about a property it skipped.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BodyError {
    /// The body does not deserialise into the type: a value of the wrong type,
    /// or a required property that is missing.
    #[error("{op}: the request body does not fit the schema the document declares at `{path}`")]
    Mismatch {
        /// The `operationId` of the operation whose body was checked.
        op: String,
        /// Where in the body the deserialiser stopped: `positions[1].flag`, or
        /// `.` for the body itself.
        path: String,
        /// `serde_json`'s message about the failure at `path`.
        #[source]
        source: serde_json::Error,
    },
    /// The body fits the type and carries a property that the type does not
    /// take. A schema that admits further properties (`additionalProperties`)
    /// takes them into a map, so a property of such a body is not this.
    ///
    /// Only one property is named: the first in the order of the body that the
    /// type does not write back, or, where there is none, the first that the
    /// type skipped while it read.
    #[error(
        "{op}: the request body carries a property that the schema the document declares does \
         not take, at `{path}`"
    )]
    Undeclared {
        /// The `operationId` of the operation whose body was checked.
        op: String,
        /// Where in the body the property is: `address.extra`. An externally
        /// tagged union puts the key of its member in the path
        /// (`place.address.extra`).
        path: String,
    },
    /// The body is a list or a scalar where the type reads an object, or holds a
    /// list in the place of one at any depth. Serde fills a struct from a list by
    /// place, so a list of the right length would otherwise fit. A type reads an
    /// object when it is a struct, a map or an adjacently tagged union. For an
    /// untagged, internally tagged or externally tagged union a scalar body that
    /// no member reads is a [`BodyError::Mismatch`], and a list body is this only
    /// when a member reads it by place. A scalar below the top is a `Mismatch`
    /// too, because serde cannot read an object from it.
    #[error(
        "{op}: the request body has {found} at `{path}`, where the schema the document declares \
         takes an object"
    )]
    NotAnObject {
        /// The `operationId` of the operation whose body was checked.
        op: String,
        /// Where in the body the value is: `address`, `positions[1]`, or `.` for
        /// the body itself.
        path: String,
        /// What the value is, as the message words it after "has": `an array`,
        /// `a string`, `a number`, `a boolean` or `null`.
        found: &'static str,
    },
    /// The body fits the type and the type cannot write itself back as JSON, so
    /// there is nothing to hold the body beside. A type that typify writes
    /// always can; a type an adopter owns (`Settings::replace`) may not.
    #[error("{op}: the type the document declares for the request body cannot be written as JSON")]
    Unwritable {
        /// The `operationId` of the operation whose body was checked.
        op: String,
        /// `serde_json`'s reading of the failure.
        #[source]
        source: serde_json::Error,
    },
}

/// Does `body` deserialise into `T`, and does it carry nothing that `T` leaves
/// out?
///
/// Generated code calls this with the type its own wrapper takes for the
/// operation, which is how a body assembled on a command line is held to the
/// same schema as a body passed from Rust — before a request is built.
///
/// Each refusal is its own [`BodyError`], and they are asked in this order:
///
/// 1. A list or a scalar as the body itself, where `T` reads an object, is
///    [`BodyError::NotAnObject`] at `.`. `T` reads an object when it is a
///    struct, a map or an adjacently tagged union.
/// 2. A value of the wrong type, or a missing property, is
///    [`BodyError::Mismatch`]. That includes a scalar below the top where `T`
///    takes an object, which serde cannot read. It includes a scalar as the body
///    that no member of an untagged, internally tagged or externally tagged union
///    reads (such a union asks for no shape), and a list that no member reads.
/// 3. A `T` that cannot be written back is [`BodyError::Unwritable`].
/// 4. The body beside what `T` wrote back. The first place in the order of the
///    body that holds either a property that `T` did not write back
///    ([`BodyError::Undeclared`]) or a list that serde read by place
///    ([`BodyError::NotAnObject`]) is the one named. Serde fills a struct from a
///    list, so a list of the right length would otherwise fit, at any depth. A
///    server may ignore a property without a word and the operator would then
///    believe it took effect.
/// 5. Where the walk found nothing, the first property that `T` skipped while it
///    read is [`BodyError::Undeclared`]. See the last paragraph.
///
/// A type that takes further properties into a map, as typify writes for
/// `additionalProperties`, takes them all.
///
/// The generated types do not refuse an unknown property themselves
/// (`deny_unknown_fields`): the same types decode a vendor's responses, and a
/// response may carry more than the document says. Only a request is held to
/// the rule, and this is where.
///
/// The measure is the type. The body is read into `T`, `T` is written back, and
/// the body is walked beside what came back. That sees inside an untagged union,
/// an internally tagged union, an adjacently tagged union whose content comes
/// before its tag, and a `flatten`ed struct (which is also what typify writes for
/// an `anyOf` whose members it cannot judge exclusive). Serde reads each of them
/// from a buffer, and nothing is skipped from a buffer. So a body that
/// the typed call cannot send is refused as well, whatever the document says: an
/// untagged union keeps the one member that serde read, and a property of
/// another member is `Undeclared`. A type has to write the shape it reads — an
/// object as an object, a list as a list — which every type typify writes does.
///
/// One property passes the walk: a type leaves a `null`, an empty list and an
/// empty map out of what it writes, as typify's `skip_serializing_if` does for an
/// optional property. A property of that kind that nothing declares is seen only
/// where the type skips it while it reads, and the read notes the place with the
/// key of each externally tagged member on the way. Serde skips nothing from a
/// buffer, so the property passes in an object that the type reads from one: an
/// untagged union, an internally tagged union, a `flatten`ed struct and the
/// content of an adjacently tagged union that comes before its tag. When the
/// body itself is an untagged union, an internally tagged union or a
/// `flatten`ed struct, it passes at the top as well. Everywhere else it is
/// [`BodyError::Undeclared`]: in a struct, in an externally tagged union, in the
/// content of an adjacently tagged union whose tag comes first, and among the
/// own keys of an adjacently tagged union, which serde reads directly whichever
/// key comes first.
pub fn fits<T: DeserializeOwned + Serialize>(
    op: &str,
    body: &serde_json::Value,
) -> Result<(), BodyError> {
    if !body.is_object() && takes_an_object::<T>() {
        return Err(Refusal::NotAnObject {
            place: Vec::new(),
            found: kind(body),
        }
        .of(op));
    }
    let Read { value, skipped } = read::<T>(body).map_err(|refused| BodyError::Mismatch {
        op: op.to_owned(),
        path: refused.path().to_string(),
        source: refused.into_inner(),
    })?;
    let written = serde_json::to_value(&value).map_err(|source| BodyError::Unwritable {
        op: op.to_owned(),
        source,
    })?;
    let refusal = beside(body, &written, &mut Vec::new())
        .or_else(|| skipped.map(Refusal::Undeclared))
        .map(|refusal| refusal.of(op));
    refusal.map_or(Ok(()), Err)
}

/// What a body holds that its type does not take.
enum Refusal<'a> {
    /// A property: its place.
    Undeclared(Vec<Step<'a>>),
    /// A list or a scalar where the type writes an object.
    NotAnObject {
        place: Vec<Step<'a>>,
        found: &'static str,
    },
}

impl Refusal<'_> {
    fn of(self, op: &str) -> BodyError {
        let op = op.to_owned();
        match self {
            Self::Undeclared(place) => BodyError::Undeclared {
                op,
                path: spelled(&place),
            },
            Self::NotAnObject { place, found } => BodyError::NotAnObject {
                op,
                path: spelled(&place),
                found,
            },
        }
    }
}

/// A place in a body in the `jq` spelling, which is `.` for the body itself.
/// This is the one function that spells one.
fn spelled(place: &[Step<'_>]) -> String {
    if place.is_empty() {
        return ".".to_owned();
    }
    let parts: Vec<String> = place
        .iter()
        .enumerate()
        .map(|(position, step)| match step {
            Step::Key(key) if position == 0 => (*key).to_owned(),
            Step::Key(key) => format!(".{key}"),
            Step::Index(index) => format!("[{index}]"),
        })
        .collect();
    parts.concat()
}

/// The first place where `body` holds what `written`, the type's own writing of
/// the same body, does not. `at` is the place of `body` in the whole.
///
/// A body list beside a written object was read by place. A body key that the
/// written object lacks was not read, unless its value is one that the type
/// leaves out when it writes: typify drops a `null`, an empty list and an empty
/// map from an optional property.
fn beside<'a>(
    body: &'a serde_json::Value,
    written: &serde_json::Value,
    at: &mut Vec<Step<'a>>,
) -> Option<Refusal<'a>> {
    use serde_json::Value;
    match (body, written) {
        (Value::Object(body), Value::Object(written)) => body.iter().find_map(|(key, value)| {
            under(at, Step::Key(key), |at| match written.get(key) {
                Some(written) => beside(value, written, at),
                None if left_out_when_empty(value) => None,
                None => Some(Refusal::Undeclared(at.clone())),
            })
        }),
        (Value::Array(body), Value::Array(written)) => body
            .iter()
            .zip(written)
            .enumerate()
            .find_map(|(index, (value, written))| {
                under(at, Step::Index(index), |at| beside(value, written, at))
            }),
        (value, Value::Object(_)) => Some(Refusal::NotAnObject {
            place: at.clone(),
            found: kind(value),
        }),
        _ => None,
    }
}

/// Run `walk` one step deeper than `at`, and leave `at` as it was.
fn under<'a, R>(
    at: &mut Vec<Step<'a>>,
    step: Step<'a>,
    walk: impl FnOnce(&mut Vec<Step<'a>>) -> R,
) -> R {
    at.push(step);
    let found = walk(at);
    at.pop();
    found
}

/// Is `value` one that a type leaves out of what it writes, as a property it
/// holds and did not need?
fn left_out_when_empty(value: &serde_json::Value) -> bool {
    value.is_null()
        || value.as_array().is_some_and(Vec::is_empty)
        || value.as_object().is_some_and(serde_json::Map::is_empty)
}

/// What a JSON value is, worded to follow "is".
pub(crate) fn kind(value: &serde_json::Value) -> &'static str {
    use serde_json::Value;
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Does `T` read an object?
///
/// Serde derives `visit_seq` beside `visit_map` for a struct, so asking the
/// body is no help: a list that happens to hold five values reads into a struct
/// of five fields. What tells an object from a list is what the type asks of
/// the deserialiser. `T` is read from a value that holds nothing and refuses
/// whatever the type asks for, and only the way it asks is kept: a struct, or a
/// map, is an object.
///
/// A type that reads itself through `deserialize_any` (an untagged union, an
/// internally tagged union, a [`serde_json::Value`]) or `deserialize_enum` (an
/// externally tagged union) asks for no particular shape, and neither does an
/// `Option`, which takes `null`. The answer is `false` for all of them. An
/// adjacently tagged union asks for a struct, and the answer is `true` for it.
/// The question is put to the body and to nothing nested in it: below the top,
/// [`fits`] finds a list read by place from what the type writes back, and a
/// list of the wrong length is a mismatch there.
fn takes_an_object<T: DeserializeOwned>() -> bool {
    let asked = Cell::new(false);
    drop(T::deserialize(Probe(&asked)));
    asked.get()
}

/// The deserialiser [`takes_an_object`] reads a type from.
struct Probe<'a>(&'a Cell<bool>);

impl<'de> serde::Deserializer<'de> for Probe<'_> {
    type Error = serde::de::value::Error;

    fn deserialize_any<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
        Err(serde::de::Error::custom("the body is read from nothing"))
    }

    fn deserialize_struct<V: serde::de::Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_map<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
        self.0.set(true);
        Err(serde::de::Error::custom("the body is read from nothing"))
    }

    /// A newtype struct is the type inside it. The derived `Deserialize` of one
    /// that is not `#[serde(transparent)]` asks for a newtype struct, and this
    /// method hands its visitor the probe again, so that the question reaches the
    /// inner type; without it the question would end at the wrapper.
    fn deserialize_newtype_struct<V: serde::de::Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct seq tuple tuple_struct enum
        identifier ignored_any
    }
}

/// One request, built and waiting, that knows what its answer deserialises into.
pub struct Call<'a, T> {
    invocation: Invocation<'a>,
    base: &'a Uri,
    response: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for Call<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Call")
            .field("operation", &self.invocation.operation().id())
            .field("response", &std::any::type_name::<T>())
            .finish()
    }
}

impl<'a, T: DeserializeOwned> Call<'a, T> {
    /// What the document says about the operation this call was built from:
    /// whether it writes, and which gates it stands behind.
    ///
    /// A Rust caller is trusted and is stopped by nothing here. A caller that
    /// wants the same gate a CLI has plans the call with [`Call::plan`], which
    /// is what the `finalize-voucher` verb in the example does.
    #[must_use]
    pub fn operation(&self) -> &Operation {
        self.invocation.operation()
    }

    /// The exact bytes that go on the wire. A CLI prints this for a dry run; a
    /// test asserts on it.
    pub fn request(&self) -> Result<HttpRequest, Error> {
        Ok(self.invocation.request(self.base)?)
    }

    /// Put the call to the gate: a read is sent on sight, and a write is sent
    /// once `answers` confirm it and answer every gate it names.
    ///
    /// The request is built here, once, and the [`Planned`] that comes back
    /// holds it. What a caller prints as a dry run and what [`Planned::send`]
    /// sends are that one value, so the two cannot drift. Nothing is sent and no
    /// client is asked for.
    pub fn plan(&self, answers: &Answers) -> Result<Planned<'a, T>, Error> {
        Ok(Planned {
            operation: self.invocation.operation(),
            plan: Plan::decide(&self.invocation, self.base, answers)?,
            response: PhantomData,
        })
    }

    /// Send it and deserialise the answer.
    pub fn send<C: SyncClient>(&self, client: &C) -> Result<T, Error> {
        deliver(self.operation().id(), client, self.request()?)
    }

    /// The same, inside a runtime.
    pub async fn send_async<C: AsyncClient>(&self, client: &C) -> Result<T, Error> {
        deliver_async(self.operation().id(), client, self.request()?).await
    }
}

/// A call that the gate has decided about, holding the request it decided on.
///
/// [`Call::plan`] is the only way to one. The verdict, the request it is about
/// and the type of the answer are one value, so a request cannot be sent through
/// a call that did not plan it, and a dry run cannot be sent at all: [`send`]
/// answers `None` for it and asks the client for nothing.
///
/// A plan that nothing sends sends nothing, so dropping one unused is a warning:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use typed_openapi::client::Error;
/// use typed_openapi::{Answers, Call, NoContent};
///
/// fn forget(call: &Call<'_, NoContent>) -> Result<(), Error> {
///     call.plan(&Answers::new())?;
///     Ok(())
/// }
/// ```
///
/// The same function compiles where the plan is read:
///
/// ```
/// #![deny(unused_must_use)]
/// use typed_openapi::client::Error;
/// use typed_openapi::{Answers, Call, NoContent};
///
/// fn look(call: &Call<'_, NoContent>) -> Result<String, Error> {
///     let planned = call.plan(&Answers::new())?;
///     Ok(planned.request().uri().to_string())
/// }
/// ```
///
/// [`send`]: Planned::send
#[must_use = "a planned call sends nothing until `send` or `send_async` is called"]
pub struct Planned<'a, T> {
    operation: &'a Operation,
    plan: Plan,
    response: PhantomData<fn() -> T>,
}

/// The verdict and the operation, and not the request: a `Debug` goes to logs,
/// and the request holds the header values and the body the caller gave.
impl<T> fmt::Debug for Planned<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Planned")
            .field("operation", &self.operation.id())
            .field(
                "verdict",
                &match self.plan {
                    Plan::Send(_) => "Send",
                    Plan::DryRun(_) => "DryRun",
                },
            )
            .field("response", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T> Planned<'_, T> {
    /// What the gate decided, with the request: [`Plan::Send`] for a read and
    /// for a write that was confirmed, [`Plan::DryRun`] for a write that was
    /// not.
    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// The request, whichever way the gate went. A dry run prints exactly the
    /// bytes that [`Planned::send`] sends.
    #[must_use]
    pub fn request(&self) -> &HttpRequest {
        self.plan.request()
    }
}

impl<T: DeserializeOwned> Planned<'_, T> {
    /// Carry out what the gate decided: send the planned request and
    /// deserialise the answer, or, for a dry run, send nothing and answer
    /// `None`.
    ///
    /// It consumes the plan, so the request is built once and sent at most once.
    pub fn send<C: SyncClient>(self, client: &C) -> Result<Option<T>, Error> {
        match self.plan {
            Plan::Send(request) => deliver(self.operation.id(), client, request).map(Some),
            Plan::DryRun(_) => Ok(None),
        }
    }

    /// The same, inside a runtime.
    pub async fn send_async<C: AsyncClient>(self, client: &C) -> Result<Option<T>, Error> {
        match self.plan {
            Plan::Send(request) => deliver_async(self.operation.id(), client, request)
                .await
                .map(Some),
            Plan::DryRun(_) => Ok(None),
        }
    }
}

/// Send `request` for the operation `op`, and read the answer.
fn deliver<T: DeserializeOwned, C: SyncClient>(
    op: &str,
    client: &C,
    request: HttpRequest,
) -> Result<T, Error> {
    parse(
        op,
        &client.send(request).map_err(|error| transport(op, error))?,
    )
}

/// The same, inside a runtime.
async fn deliver_async<T: DeserializeOwned, C: AsyncClient>(
    op: &str,
    client: &C,
    request: HttpRequest,
) -> Result<T, Error> {
    parse(
        op,
        &client
            .send(request)
            .await
            .map_err(|error| transport(op, error))?,
    )
}

fn transport<E: std::error::Error + Send + Sync + 'static>(op: &str, error: E) -> Error {
    Error::Transport {
        op: op.to_owned(),
        source: Box::new(error),
    }
}

/// Status first, then the body — never deserialise the success type out of an
/// error response.
fn parse<T: DeserializeOwned>(op: &str, response: &HttpResponse) -> Result<T, Error> {
    let body = response.body();
    if !response.status().is_success() {
        return Err(Error::Status {
            op: op.to_owned(),
            status: response.status(),
            body: String::from_utf8_lossy(body).into_owned(),
        });
    }
    // An empty body is how "no content" arrives; `NoContent` accepts `null`.
    let bytes = if body.is_empty() { b"null" } else { &body[..] };
    serde_json::from_slice(bytes).map_err(|error| Error::Decode {
        op: op.to_owned(),
        source: DecodeError::of(&error),
        raw: String::from_utf8_lossy(body).into_owned(),
    })
}

/// The request body of a typed wrapper, as JSON. `op` is the `operationId` of
/// the operation the value is the body of, and the error that a value which does
/// not serialise raises names it.
pub fn to_json<T: Serialize>(op: &str, value: &T) -> Result<serde_json::Value, Error> {
    serde_json::to_value(value).map_err(|source| Error::Encode {
        op: op.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    #![expect(clippy::unwrap_used, reason = "a failed unwrap is a failing test")]

    use std::collections::HashMap;

    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};

    use super::{BodyError, DecodeClass, DecodeError, fits};

    /// A body with a list of objects in it, which is the shape whose refusal
    /// is hardest to place by eye: serde's own message does not say which
    /// element of which list held the value.
    #[derive(Deserialize, Serialize)]
    struct Save {
        positions: Vec<Position>,
    }

    #[derive(Deserialize, Serialize)]
    struct Position {
        flag: bool,
    }

    /// The body of the toy's `createContact`: a nested object, one level down.
    #[derive(Deserialize, Serialize)]
    struct Contact {
        name: String,
        address: Address,
    }

    #[derive(Deserialize, Serialize)]
    struct Address {
        street: String,
        city: String,
    }

    /// What typify writes for `type: object, additionalProperties: {type:
    /// string}`: a transparent newtype over a map.
    #[derive(Deserialize, Serialize)]
    #[serde(transparent)]
    struct Labels(HashMap<String, String>);

    /// A struct that takes its undeclared properties into a map of its own.
    #[derive(Deserialize, Serialize)]
    struct Open {
        name: String,
        #[serde(flatten)]
        extra: HashMap<String, Value>,
    }

    /// The probe's own body: five values, in the order the struct declares its
    /// fields, which serde's derived `Deserialize` reads from a list by place.
    #[derive(Deserialize, Serialize)]
    struct Voucher {
        currency: String,
        id: Option<i64>,
        internal_ref: Option<String>,
        status: String,
        total: String,
    }

    /// Objects at every depth that serde can read from a list: in a list, in a
    /// map and in a property.
    #[derive(Deserialize, Serialize)]
    struct Pack {
        addresses: Vec<Address>,
        by_name: HashMap<String, Address>,
        main: Address,
    }

    #[test]
    fn a_body_that_does_not_fit_names_the_place_it_stopped_at() {
        let body = json!({"positions": [{"flag": true}, {"flag": "1"}]});

        let refused = fits::<Save>("save", &body).unwrap_err();

        let BodyError::Mismatch { op, path, source } = &refused else {
            panic!("a wrong type is a mismatch: {refused:?}");
        };
        assert_eq!(op, "save");
        assert_eq!(path, "positions[1].flag");
        assert!(
            refused.to_string().contains("`positions[1].flag`"),
            "the refusal does not say where the body went wrong: {refused}"
        );
        assert_eq!(
            source.to_string(),
            "invalid type: string \"1\", expected a boolean",
            "serde's own reading of the value was lost"
        );
    }

    #[test]
    fn a_field_missing_from_the_top_of_the_body_is_placed_at_the_body_itself() {
        let refused = fits::<Save>("save", &json!({})).unwrap_err();

        let BodyError::Mismatch { path, source, .. } = &refused else {
            panic!("a missing field is a mismatch: {refused:?}");
        };
        assert_eq!(path, ".");
        assert_eq!(source.to_string(), "missing field `positions`");
    }

    #[test]
    fn a_body_that_fits_is_not_refused() {
        assert!(fits::<Save>("save", &json!({"positions": [{"flag": false}]})).is_ok());
    }

    #[test]
    fn a_property_no_schema_declares_is_refused_and_named() {
        let body = json!({
            "name": "Acme Ltd",
            "address": {"street": "1 Example Way", "city": "Vienna"},
            "undeclaredField": "this property is in no schema",
        });

        let refused = fits::<Contact>("createContact", &body).unwrap_err();

        let BodyError::Undeclared { op, path } = &refused else {
            panic!("a property nothing declares is its own refusal: {refused:?}");
        };
        assert_eq!(op, "createContact");
        assert_eq!(path, "undeclaredField");
        let shown = refused.to_string();
        assert!(shown.contains("createContact"), "{shown}");
        assert!(shown.contains("`undeclaredField`"), "{shown}");
    }

    /// The value of a refused property can be a credential or a client's data,
    /// and the refusal is printed where an agent and a log both read it.
    #[test]
    fn the_refusal_names_the_property_and_never_its_value() {
        let body = json!({
            "name": "Acme Ltd",
            "address": {"street": "1 Example Way", "city": "Vienna"},
            "undeclaredField": "this property is in no schema",
        });

        let refused = fits::<Contact>("createContact", &body).unwrap_err();

        assert!(
            !refused
                .to_string()
                .contains("this property is in no schema"),
            "{refused}"
        );
    }

    #[test]
    fn a_property_deep_in_the_body_is_named_by_its_path() {
        let nested = json!({
            "name": "Acme Ltd",
            "address": {"street": "1 Example Way", "city": "Vienna", "extra": "x"},
        });
        let listed = json!({"positions": [{"flag": true}, {"flag": false, "extra": 1}]});

        let nested = fits::<Contact>("createContact", &nested).unwrap_err();
        let listed = fits::<Save>("save", &listed).unwrap_err();

        assert!(
            matches!(&nested, BodyError::Undeclared { path, .. } if path == "address.extra"),
            "{nested:?}"
        );
        assert!(
            matches!(&listed, BodyError::Undeclared { path, .. } if path == "positions[1].extra"),
            "{listed:?}"
        );
    }

    #[test]
    fn the_first_property_in_the_bodys_own_order_is_the_one_named() {
        let body = json!({
            "name": "Acme Ltd",
            "zeta": 1,
            "address": {"street": "1 Example Way", "city": "Vienna", "alpha": 2},
        });

        let refused = fits::<Contact>("createContact", &body).unwrap_err();

        assert!(
            matches!(&refused, BodyError::Undeclared { path, .. } if path == "zeta"),
            "{refused:?}"
        );
    }

    #[test]
    fn a_property_an_open_schema_admits_is_not_refused() {
        let labels = json!({"vip": "yes", "region": "east"});
        let open = json!({"name": "Acme Ltd", "vip": true, "region": {"code": "east"}});

        assert!(fits::<Labels>("setContactLabels", &labels).is_ok());
        assert!(fits::<Open>("openContact", &open).is_ok());
    }

    /// serde reads a struct from a list by position, and nothing about an
    /// unknown property sees that.
    #[test]
    fn a_list_where_the_type_takes_an_object_is_refused() {
        let body = json!(["EUR", null, null, "draft", "1.00"]);

        let refused = fits::<Voucher>("createVoucher", &body).unwrap_err();

        let BodyError::NotAnObject { op, path, found } = &refused else {
            panic!("the list is refused for what it is, not for its fields: {refused:?}");
        };
        assert_eq!(op, "createVoucher");
        assert_eq!(path, ".");
        assert_eq!(*found, "an array");
        assert_eq!(
            refused.to_string(),
            "createVoucher: the request body has an array at `.`, where the schema the document \
             declares takes an object"
        );
    }

    /// The body is an object whatever the length of the list that stands for it,
    /// so a list that no struct could read is refused for what it is as well.
    #[test]
    fn a_list_of_any_length_is_refused_for_what_it_is() {
        let refused = fits::<Voucher>("createVoucher", &json!(["EUR"])).unwrap_err();

        assert!(
            matches!(&refused, BodyError::NotAnObject { path, .. } if path == "."),
            "{refused:?}"
        );
    }

    #[test]
    fn a_scalar_where_the_type_takes_an_object_is_refused_for_what_it_is() {
        for (body, found) in [
            (json!("EUR"), "a string"),
            (json!(5), "a number"),
            (json!(true), "a boolean"),
            (Value::Null, "null"),
        ] {
            let refused = fits::<Voucher>("createVoucher", &body).unwrap_err();
            assert!(
                matches!(&refused, BodyError::NotAnObject { found: seen, .. } if *seen == found),
                "{body}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_map_is_an_object_too() {
        let refused = fits::<Labels>("setContactLabels", &json!(["vip"])).unwrap_err();

        assert!(
            matches!(refused, BodyError::NotAnObject { .. }),
            "{refused:?}"
        );
    }

    /// A newtype that is not `#[serde(transparent)]` asks the deserialiser for a
    /// newtype struct, and the object is in what it wraps.
    #[test]
    fn a_newtype_around_an_object_takes_an_object() {
        #[derive(Deserialize, Serialize)]
        struct Named(Labels);

        let refused = fits::<Named>("setContactLabels", &json!(["vip"])).unwrap_err();

        assert!(
            matches!(&refused, BodyError::NotAnObject { path, .. } if path == "."),
            "{refused:?}"
        );
    }

    #[test]
    fn a_list_where_the_type_takes_a_list_is_not_refused() {
        assert!(fits::<Vec<Position>>("saveAll", &json!([{"flag": true}])).is_ok());
    }

    #[test]
    fn a_wrong_type_is_reported_before_a_property_nothing_declares() {
        let body = json!({"positions": [{"flag": "1", "extra": 1}]});

        let refused = fits::<Save>("save", &body).unwrap_err();

        assert!(matches!(refused, BodyError::Mismatch { .. }), "{refused:?}");
    }

    #[derive(Deserialize, Serialize)]
    struct Customer {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        memo: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        address: Option<Address>,
        #[serde(skip_serializing_if = "Option::is_none")]
        labels: Option<Labels>,
    }

    #[test]
    fn a_wrapper_serde_adds_around_a_value_is_no_step_in_the_path() {
        let body = json!({
            "name": "Acme Ltd",
            "address": {"street": "1 Example Way", "city": "Vienna", "extra": "x"},
        });

        let refused = fits::<Customer>("createCustomer", &body).unwrap_err();

        assert!(
            matches!(&refused, BodyError::Undeclared { path, .. } if path == "address.extra"),
            "{refused:?}"
        );
    }

    #[test]
    fn a_property_of_an_open_schema_nested_in_a_body_is_not_refused() {
        let body = json!({"name": "Acme Ltd", "labels": {"vip": "yes", "region": "east"}});

        assert!(fits::<Customer>("createCustomer", &body).is_ok());
    }

    /// A type leaves a property out of what it writes when the property holds
    /// nothing: typify's `skip_serializing_if`. That is no property of the body
    /// that the type failed to read.
    #[test]
    fn a_declared_property_that_holds_nothing_is_not_refused() {
        let body = json!({"name": "Acme Ltd", "memo": null, "tags": [], "labels": {}});

        assert!(fits::<Customer>("createCustomer", &body).is_ok());
    }

    /// The type skips the property while it reads, and the read notes the place
    /// even though the value is one the walk of the body lets pass.
    #[test]
    fn an_undeclared_property_that_holds_nothing_is_refused_where_the_type_skips_it() {
        for empty in [Value::Null, json!([]), json!({})] {
            let top = json!({"name": "Acme Ltd", "undeclaredField": empty});
            let nested = json!({
                "name": "Acme Ltd",
                "address": {"street": "1 Example Way", "city": "Vienna", "extra": empty},
            });

            let top = fits::<Customer>("createCustomer", &top).unwrap_err();
            let nested = fits::<Customer>("createCustomer", &nested).unwrap_err();

            assert!(
                matches!(&top, BodyError::Undeclared { path, .. } if path == "undeclaredField"),
                "{top:?}"
            );
            assert!(
                matches!(&nested, BodyError::Undeclared { path, .. } if path == "address.extra"),
                "{nested:?}"
            );
        }
    }

    /// A list where the type reads an object, at each depth serde can read it
    /// from a list.
    #[test]
    fn a_list_in_the_place_of_a_nested_object_is_refused_at_its_path() {
        let address = json!({"street": "1 Example Way", "city": "Vienna"});
        let listed = json!(["2 Example Way", "Graz"]);
        let property = json!({"name": "Acme Ltd", "address": listed});
        let element = json!({"addresses": [address, listed], "by_name": {}, "main": address});
        let value = json!({"addresses": [], "by_name": {"k": listed}, "main": address});

        let property = fits::<Customer>("createCustomer", &property).unwrap_err();
        let element = fits::<Pack>("pack", &element).unwrap_err();
        let value = fits::<Pack>("pack", &value).unwrap_err();

        for (refused, at) in [
            (property, "address"),
            (element, "addresses[1]"),
            (value, "by_name.k"),
        ] {
            assert!(
                matches!(&refused, BodyError::NotAnObject { path, found, .. }
                    if path == at && *found == "an array"),
                "{at}: {refused:?}"
            );
        }
    }

    /// Serde cannot read a list of the wrong length into a struct, so that is a
    /// value of the wrong type like any other.
    #[test]
    fn a_list_that_the_type_cannot_read_in_the_place_of_a_nested_object_is_a_mismatch() {
        let body = json!({"name": "Acme Ltd", "address": ["1 Example Way"]});

        let refused = fits::<Contact>("createContact", &body).unwrap_err();

        assert!(
            matches!(&refused, BodyError::Mismatch { path, .. } if path == "address"),
            "{refused:?}"
        );
    }

    /// Serde reads a struct from a scalar never, so a scalar in the place of a
    /// nested object is a value of the wrong type like any other.
    #[test]
    fn a_scalar_in_the_place_of_a_nested_object_is_a_mismatch() {
        for scalar in [json!("1 Example Way"), json!(5), json!(true)] {
            let body = json!({"name": "Acme Ltd", "address": scalar});

            let refused = fits::<Contact>("createContact", &body).unwrap_err();

            assert!(
                matches!(&refused, BodyError::Mismatch { path, .. } if path == "address"),
                "{scalar}: {refused:?}"
            );
        }
    }

    /// What typify writes for an externally tagged `oneOf`: a newtype variant
    /// around a named schema, and a struct variant for an inline one.
    #[derive(Deserialize, Serialize)]
    enum Placed {
        #[serde(rename = "address")]
        Address(Address),
        #[serde(rename = "point")]
        Point { x: i64 },
    }

    #[derive(Deserialize, Serialize)]
    struct Parcel {
        place: Placed,
    }

    /// The key of a variant is a step of the body. The read takes it from the
    /// enum the type asks for, so a property that holds nothing, which only the
    /// read sees, has it in its path.
    #[test]
    fn the_key_of_an_externally_tagged_variant_is_in_the_path_of_an_empty_property() {
        for empty in [Value::Null, json!([]), json!({})] {
            let newtype =
                json!({"place": {"address": {"street": "s", "city": "c", "extra": empty}}});
            let record = json!({"place": {"point": {"x": 1, "extra": empty}}});

            let newtype = fits::<Parcel>("send", &newtype).unwrap_err();
            let record = fits::<Parcel>("send", &record).unwrap_err();

            assert!(
                matches!(&newtype, BodyError::Undeclared { path, .. } if path == "place.address.extra"),
                "{newtype:?}"
            );
            assert!(
                matches!(&record, BodyError::Undeclared { path, .. } if path == "place.point.extra"),
                "{record:?}"
            );
        }
    }

    /// The type tells a variant from a property, and the body can spell both
    /// alike: the property that was skipped is spelled like the variant.
    #[test]
    fn a_variant_key_spelled_like_the_property_that_was_skipped_is_told_apart() {
        let body = json!({"place": {"point": {"x": 1, "point": null}}});

        let refused = fits::<Parcel>("send", &body).unwrap_err();

        assert!(
            matches!(&refused, BodyError::Undeclared { path, .. } if path == "place.point.point"),
            "{refused:?}"
        );
    }

    /// What typify writes for a `oneOf` of one-property objects whose property
    /// holds an object that has a property of the same name: a struct variant
    /// whose first field is spelled like its key.
    #[derive(Deserialize, Serialize)]
    enum Repeated {
        #[serde(rename = "address")]
        Address {
            #[serde(skip_serializing_if = "Option::is_none")]
            address: Option<Address>,
            #[serde(skip_serializing_if = "Option::is_none")]
            note: Option<String>,
        },
    }

    /// The key of the variant and the name of its first field are the same word,
    /// and the body holds it twice. The path has both whatever the skipped
    /// property holds.
    #[test]
    fn a_member_key_that_its_content_repeats_is_in_the_path_whatever_the_value() {
        for property in [Value::Null, json!([]), json!({}), json!("x")] {
            let address = json!({"street": "s", "city": "c", "extra": property});
            let body = json!({"address": {"address": address, "note": "n"}});

            let refused = fits::<Repeated>("send", &body).unwrap_err();

            assert!(
                matches!(&refused, BodyError::Undeclared { path, .. } if path == "address.address.extra"),
                "{property}: {refused:?}"
            );
        }
    }

    /// What typify writes for a `oneOf` whose member holds a property spelled like
    /// a property of the object inside it: a struct variant with a field `a` and a
    /// field `b`, and the object of `a` has no `b`.
    #[derive(Deserialize, Serialize)]
    enum Member {
        #[serde(rename = "a")]
        A {
            #[serde(skip_serializing_if = "Option::is_none")]
            a: Option<Counted>,
            #[serde(skip_serializing_if = "Option::is_none")]
            b: Option<i64>,
        },
    }

    #[derive(Deserialize, Serialize)]
    struct Counted {
        #[serde(skip_serializing_if = "Option::is_none")]
        x: Option<i64>,
    }

    /// The member declares `b` and holds nothing in it, and the object inside the
    /// member holds an undeclared `b` that holds nothing too. Both are spelled the
    /// same and a body can be read either way, so only the type says which one it
    /// skipped.
    #[test]
    fn a_declared_property_spelled_like_the_skipped_one_does_not_take_its_place() {
        for property in [Value::Null, json!([]), json!({})] {
            let body = json!({"a": {"a": {"x": 1, "b": property}, "b": null}});

            let refused = fits::<Member>("send", &body).unwrap_err();

            assert!(
                matches!(&refused, BodyError::Undeclared { path, .. } if path == "a.a.b"),
                "{property}: {refused:?}"
            );
        }
    }

    /// An object of one key is a property of a struct and not a variant, because
    /// the type reads it so.
    #[test]
    fn a_one_key_object_is_a_property_when_the_type_reads_it_as_one() {
        #[derive(Deserialize, Serialize)]
        struct Lone {
            #[serde(skip_serializing_if = "Option::is_none")]
            address: Option<Address>,
        }

        let body = json!({"address": {"street": "s", "city": "c", "extra": null}});

        let refused = fits::<Lone>("lone", &body).unwrap_err();

        assert!(
            matches!(&refused, BodyError::Undeclared { path, .. } if path == "address.extra"),
            "{refused:?}"
        );
    }

    /// What typify writes for a `oneOf`, or an `anyOf` it judges exclusive, whose
    /// members are objects that it cannot tell apart by a tag.
    #[derive(Deserialize, Serialize)]
    #[serde(untagged)]
    enum Either {
        Address(Address),
        Count { count: i64 },
    }

    /// A type that reads one struct through another with `#[serde(flatten)]`,
    /// which serde reads from a buffer. typify writes that attribute for the map
    /// of `additionalProperties` beside properties.
    #[derive(Deserialize, Serialize)]
    struct Composed {
        name: String,
        #[serde(flatten)]
        address: Address,
    }

    /// Serde reads an untagged union and a `flatten`ed struct from a buffer, and
    /// nothing is skipped from a buffer, so a property inside one is found by
    /// what the type writes back.
    #[test]
    fn a_property_inside_an_untagged_union_or_a_flattened_struct_is_refused() {
        let address = json!({"street": "1 Example Way", "city": "Vienna", "extra": "x"});
        let composed = json!({"name": "Acme Ltd", "street": "1", "city": "Vienna", "extra": "x"});

        let union = fits::<Either>("pick", &address).unwrap_err();
        let flattened = fits::<Composed>("compose", &composed).unwrap_err();

        assert!(
            matches!(&union, BodyError::Undeclared { path, .. } if path == "extra"),
            "{union:?}"
        );
        assert!(
            matches!(&flattened, BodyError::Undeclared { path, .. } if path == "extra"),
            "{flattened:?}"
        );
    }

    /// The one property the walk of the body lets pass and nothing else sees:
    /// the type does not write it back, and does not skip it while it reads.
    /// When this fails, the limit is gone and the documentation says so.
    #[test]
    fn an_undeclared_property_that_holds_nothing_inside_an_untagged_union_is_not_seen() {
        for empty in [Value::Null, json!([]), json!({})] {
            let address = json!({"street": "1 Example Way", "city": "Vienna", "extra": empty});

            assert!(fits::<Either>("pick", &address).is_ok(), "{address}");
        }
    }

    /// A schema that states `additionalProperties: false` is closed by typify
    /// itself, which refuses at the first unknown key, so serde's refusal gets
    /// there before the check does.
    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Closed {
        name: String,
    }

    #[test]
    fn a_type_that_denies_unknown_fields_refuses_them_as_a_mismatch() {
        let refused =
            fits::<Closed>("close", &json!({"name": "Acme Ltd", "extra": 1})).unwrap_err();

        let BodyError::Mismatch { source, .. } = &refused else {
            panic!("serde refused it first: {refused:?}");
        };
        assert!(
            source.to_string().starts_with("unknown field `extra`"),
            "{source}"
        );
    }

    /// A type that reads a body and cannot write it back leaves nothing to hold
    /// the body beside, and the gate does not let that pass.
    #[derive(Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Unwritten {
        name: String,
    }

    impl Serialize for Unwritten {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("this type is not written"))
        }
    }

    #[test]
    fn a_type_that_cannot_be_written_back_is_refused_and_not_let_through() {
        let refused = fits::<Unwritten>("write", &json!({"name": "Acme Ltd"})).unwrap_err();

        let BodyError::Unwritable { op, source } = &refused else {
            panic!("there is nothing to hold the body beside: {refused:?}");
        };
        assert_eq!(op, "write");
        assert_eq!(source.to_string(), "this type is not written");
    }

    /// A reader that fails: the one way `serde_json` raises its `Io` category,
    /// which a slice never does.
    struct Failing;

    impl std::io::Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("secret-token"))
        }
    }

    #[test]
    fn a_read_that_fails_is_its_own_class_and_prints_nothing_of_the_failure() {
        let failed = serde_json::from_reader::<_, u8>(Failing).unwrap_err();

        let cause = DecodeError::of(&failed);

        assert_eq!(cause.class, DecodeClass::Io);
        assert!(
            cause.to_string().starts_with("the body could not be read"),
            "{cause}"
        );
        assert!(!cause.to_string().contains("secret-token"), "{cause}");
    }
}
