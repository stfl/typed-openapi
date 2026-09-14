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
pub enum Error {
    #[error("the document does not load: {0}")]
    Document(#[from] DocumentError),
    #[error(transparent)]
    Drift(#[from] DriftError),
    #[error(transparent)]
    Values(#[from] ValueError),
    #[error("cannot build the request: {0}")]
    Request(#[from] http::Error),
    #[error("cannot serialise the request body: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("transport: {0}")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{status}: {body}")]
    Status { status: StatusCode, body: String },
    #[error("the response is not the shape the document promises: {source}")]
    Decode {
        #[source]
        source: serde_json::Error,
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

/// A request body that does not deserialise into the type the document
/// describes for it.
///
/// This is what a `--json-body` file gets checked against. The source is
/// serde's own message, which names the field that is missing or ill-typed.
#[derive(Debug, Error)]
#[error("{op}: the request body does not fit the schema the document declares")]
pub struct BodyError {
    pub op: String,
    #[source]
    pub source: serde_json::Error,
}

/// Does `body` deserialise into `T`?
///
/// Generated code calls this with the type its own wrapper takes for the
/// operation, which is how a body assembled on a command line is held to the
/// same schema as a body passed from Rust — before a request is built.
pub fn fits<T: DeserializeOwned>(op: &str, body: &serde_json::Value) -> Result<(), BodyError> {
    serde_json::from_value::<T>(body.clone())
        .map(drop)
        .map_err(|source| BodyError {
            op: op.to_owned(),
            source,
        })
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
