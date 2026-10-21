//! A typed client over a document, and the two steps every call is made of.
//!
//! [`Client`] holds one document and hands out [`Call`]s by `operationId`.
//! `Call` knows what its answer deserialises into, so sending is one expression
//! in each flavour and a caller never names a response type twice.
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

use crate::model::{Document, DocumentError, DriftError, Operation};
use crate::request::{Invocation, ValueError};
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
    /// The values satisfied the operation and the `http` crate refused the
    /// request they render to: a base URI, a header name or a header value it
    /// cannot carry.
    #[error("cannot build the request")]
    Request(#[from] http::Error),
    /// A typed argument that does not serialise to JSON, so there is no body to
    /// send. The cause is `serde_json`'s reading of the value.
    #[error("cannot serialise the request body")]
    Encode(#[source] serde_json::Error),
    /// The client refused the request or never got an answer, carrying the
    /// error the client itself returned.
    ///
    /// Boxed, so this type does not grow a parameter for the client. Whether
    /// the request left is a reading only an adapter can make, so it belongs
    /// above the seam, and a failure nobody has classified is one that may have
    /// arrived. The concrete error is here to be read: `downcast_ref` recovers
    /// `C::Error`, and [`Call::request`] hands back the bytes for a caller who
    /// would rather send them through the client's own `send` and box nothing.
    /// `docs/client.md` shows both.
    #[error("the transport failed")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// The server answered with a status that is not a success. Nothing is
    /// deserialised out of such a response, whatever its body holds.
    #[error("{status}: {body}")]
    Status {
        /// The status the server answered.
        status: StatusCode,
        /// The response body as text, with bytes that are not UTF-8 replaced.
        body: String,
    },
    /// A success response whose body is not the type the document promises for
    /// it.
    #[error("the response is not the shape the document promises")]
    Decode {
        /// `serde_json`'s reading of the body, with the line and column where
        /// it stopped.
        #[source]
        source: serde_json::Error,
        /// The response body as text, with bytes that are not UTF-8 replaced.
        raw: String,
    },
}

/// One document, one server, and a typed way to call anything in it.
#[derive(Debug)]
pub struct Client {
    document: Document,
    base: Uri,
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
    pub fn over(document: Document) -> Result<Self, Error> {
        let base = document.base().clone();
        Ok(Self { document, base })
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
/// None of them prints a value from the body: it can be a credential or a
/// client's data, and the refusal is printed where a log reads it.
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
    /// Only the first property, in the order of the body, is named.
    #[error(
        "{op}: the request body carries a property that the schema the document declares does \
         not take, at `{path}`"
    )]
    Undeclared {
        /// The `operationId` of the operation whose body was checked.
        op: String,
        /// Where in the body the property is: `address.extra`.
        path: String,
    },
    /// The body is not an object where the type takes one. Serde fills a struct
    /// from a list by place, so a list of the right length would otherwise fit.
    #[error(
        "{op}: the request body is {found}, and the schema the document declares for it takes \
         an object"
    )]
    NotAnObject {
        /// The `operationId` of the operation whose body was checked.
        op: String,
        /// What the body is, as the message words it after "is": `an array`,
        /// `a string`, `a number`, `a boolean` or `null`.
        found: &'static str,
    },
}

/// Does `body` deserialise into `T`, and does it carry nothing that `T` leaves
/// out?
///
/// Generated code calls this with the type its own wrapper takes for the
/// operation, which is how a body assembled on a command line is held to the
/// same schema as a body passed from Rust — before a request is built.
///
/// Three things refuse a body, each as its own [`BodyError`]. A value of the
/// wrong type, or a missing property, is a mismatch. A body that is not an
/// object where `T` takes one is refused for what it is. A property that `T`
/// does not take is refused by its path, because a server may ignore it
/// without a word and the operator would then believe it took effect. A type
/// that takes further properties into a map, as typify writes for
/// `additionalProperties`, takes them all.
///
/// The generated types do not refuse an unknown property themselves
/// (`deny_unknown_fields`): the same types decode a vendor's responses, and a
/// response may carry more than the document says. Only a request is held to
/// the rule, and this is where.
///
/// A property is seen when the type skips it while it reads the body. Serde
/// reads a union (`oneOf` or `anyOf`, which typify writes as an `untagged`
/// enum) and a `flatten`ed struct from a buffer, and nothing is skipped from a
/// buffer in a way the check sees. A property inside one of them passes.
pub fn fits<T: DeserializeOwned>(op: &str, body: &serde_json::Value) -> Result<(), BodyError> {
    if !body.is_object() && takes_an_object::<T>() {
        return Err(BodyError::NotAnObject {
            op: op.to_owned(),
            found: kind(body),
        });
    }
    let mut undeclared = None;
    let mut skipped = |path: serde_ignored::Path<'_>| {
        undeclared.get_or_insert_with(|| spelled(&path));
    };
    serde_path_to_error::deserialize::<_, T>(serde_ignored::Deserializer::new(body, &mut skipped))
        .map_err(|refused| BodyError::Mismatch {
            op: op.to_owned(),
            path: refused.path().to_string(),
            source: refused.into_inner(),
        })?;
    match undeclared {
        Some(path) => Err(BodyError::Undeclared {
            op: op.to_owned(),
            path,
        }),
        None => Ok(()),
    }
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

/// A path that `serde_ignored` found, in the `jq` spelling that
/// `serde_path_to_error` gives a path to a failure.
fn spelled(path: &serde_ignored::Path<'_>) -> String {
    let spelling = below(path);
    if spelling.is_empty() {
        ".".to_owned()
    } else {
        spelling
    }
}

/// The spelling of `path`, which is empty for the body itself. A wrapper that
/// serde adds around a value (`Some`, a newtype) is no step in the body.
fn below(path: &serde_ignored::Path<'_>) -> String {
    use serde_ignored::Path;
    match path {
        Path::Root => String::new(),
        Path::Seq { parent, index } => format!("{}[{index}]", below(parent)),
        Path::Map { parent, key } => match below(parent) {
            above if above.is_empty() => key.clone(),
            above => format!("{above}.{key}"),
        },
        Path::Some { parent }
        | Path::NewtypeStruct { parent }
        | Path::NewtypeVariant { parent } => below(parent),
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
/// A type that reads itself through `deserialize_any` (a union, a
/// [`serde_json::Value`]) asks for no particular shape, and neither does an
/// `Option`, which takes `null`. The answer is `false` for all of them.
fn takes_an_object<T: DeserializeOwned>() -> bool {
    let asked = Cell::new(false);
    drop(T::deserialize(Asked(&asked)));
    asked.get()
}

/// The deserialiser [`takes_an_object`] reads a type from.
struct Asked<'a>(&'a Cell<bool>);

impl<'de> serde::Deserializer<'de> for Asked<'_> {
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

    /// A newtype is the type inside it, which is how a map the document names
    /// reads itself.
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

impl<T: DeserializeOwned> Call<'_, T> {
    /// What the document says about the operation this call was built from:
    /// whether it writes, and which gates it stands behind.
    ///
    /// A Rust caller is trusted and is stopped by nothing here. A caller that
    /// wants the same gate a CLI has hands this to [`Plan::decide`], which is
    /// what the `finalize-voucher` verb in the example does.
    ///
    /// [`Plan::decide`]: crate::Plan::decide
    #[must_use]
    pub fn operation(&self) -> &Operation {
        self.invocation.operation()
    }

    /// The exact bytes that go on the wire. A CLI prints this for a dry run; a
    /// test asserts on it.
    pub fn request(&self) -> Result<HttpRequest, Error> {
        Ok(self.invocation.request(self.base)?)
    }

    /// Send it and deserialise the answer.
    pub fn send<C: SyncClient>(&self, client: &C) -> Result<T, Error> {
        parse(&client.send(self.request()?).map_err(transport)?)
    }

    /// The same, inside a runtime.
    pub async fn send_async<C: AsyncClient>(&self, client: &C) -> Result<T, Error> {
        parse(&client.send(self.request()?).await.map_err(transport)?)
    }
}

fn transport<E: std::error::Error + Send + Sync + 'static>(error: E) -> Error {
    Error::Transport(Box::new(error))
}

/// Status first, then the body — never deserialise the success type out of an
/// error response.
fn parse<T: DeserializeOwned>(response: &HttpResponse) -> Result<T, Error> {
    let body = response.body();
    if !response.status().is_success() {
        return Err(Error::Status {
            status: response.status(),
            body: String::from_utf8_lossy(body).into_owned(),
        });
    }
    // An empty body is how "no content" arrives; `NoContent` accepts `null`.
    let bytes = if body.is_empty() { b"null" } else { &body[..] };
    serde_json::from_slice(bytes).map_err(|source| Error::Decode {
        source,
        raw: String::from_utf8_lossy(body).into_owned(),
    })
}

/// The request body of a typed wrapper, as JSON.
pub fn to_json<T: Serialize>(value: &T) -> Result<serde_json::Value, Error> {
    serde_json::to_value(value).map_err(Error::Encode)
}

#[cfg(test)]
mod tests {
    #![expect(clippy::unwrap_used, reason = "a failed unwrap is a failing test")]

    use std::collections::HashMap;

    use serde::Deserialize;
    use serde_json::{Value, json};

    use super::{BodyError, fits};

    /// A body with a list of objects in it, which is the shape whose refusal
    /// is hardest to place by eye: serde's own message does not say which
    /// element of which list held the value.
    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Save {
        positions: Vec<Position>,
    }

    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Position {
        flag: bool,
    }

    /// The body of the toy's `createContact`: a nested object, one level down.
    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Contact {
        name: String,
        address: Address,
    }

    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Address {
        street: String,
        city: String,
    }

    /// What typify writes for `type: object, additionalProperties: {type:
    /// string}`: a transparent newtype over a map.
    #[derive(Debug, Deserialize)]
    #[serde(transparent)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Labels(HashMap<String, String>);

    /// A struct that takes its undeclared properties into a map of its own.
    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Open {
        name: String,
        #[serde(flatten)]
        extra: HashMap<String, Value>,
    }

    /// The probe's own body: five values, in the order the struct declares its
    /// fields, which serde's derived `Deserialize` reads from a list by place.
    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Voucher {
        currency: String,
        id: Option<i64>,
        internal_ref: Option<String>,
        status: String,
        total: String,
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

        let shown = format!("{refused} / {refused:?}");
        assert!(!shown.contains("this property is in no schema"), "{shown}");
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

        let BodyError::NotAnObject { op, found } = &refused else {
            panic!("the list is refused for what it is, not for its fields: {refused:?}");
        };
        assert_eq!(op, "createVoucher");
        assert_eq!(*found, "an array");
        assert_eq!(
            refused.to_string(),
            "createVoucher: the request body is an array, and the schema the document \
             declares for it takes an object"
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

    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Customer {
        name: String,
        address: Option<Address>,
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

    /// What typify writes for a `oneOf` or an `anyOf` with an object in it.
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    enum Either {
        Address(Address),
        Count { count: i64 },
    }

    /// A type that reads one struct through another, which a hand-written type
    /// may do and typify does not write for a schema.
    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Composed {
        name: String,
        #[serde(flatten)]
        address: Address,
    }

    /// What typify writes for `additionalProperties: false`.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Closed {
        name: String,
    }

    /// The limit `fits` states in its documentation: serde reads a union and a
    /// `flatten`ed field from a buffer, and nothing is skipped from a buffer.
    /// When this fails, the limit is gone and the documentation says so.
    #[test]
    fn a_property_inside_a_union_or_a_flattened_struct_is_not_seen() {
        let address = json!({"street": "1 Example Way", "city": "Vienna", "extra": "x"});
        let composed = json!({"name": "Acme Ltd", "street": "1", "city": "Vienna", "extra": "x"});

        assert!(fits::<Either>("pick", &address).is_ok());
        assert!(fits::<Composed>("compose", &composed).is_ok());
    }

    /// A schema that states `additionalProperties: false` is closed by typify
    /// itself, which refuses at the first unknown key, so serde's refusal gets
    /// there before the check does.
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

    /// The other limit `fits` states: the question "does the type take an
    /// object" is asked of the body, and a type nested in it is read by serde.
    #[test]
    fn a_list_in_the_place_of_a_nested_object_is_read_by_place() {
        let body = json!({"name": "Acme Ltd", "address": ["1 Example Way", "Vienna"]});

        assert!(fits::<Contact>("createContact", &body).is_ok());
    }
}
