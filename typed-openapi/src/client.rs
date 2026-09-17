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
/// This is what a `--json-body` file gets checked against. `path` is where in
/// the body the deserialiser stopped, written the way `jq` writes one —
/// `positions[1].flag`, and `.` for the body itself — and the source is
/// serde's own message about the value it found there. The two are apart
/// because serde's message names a value and the type it wanted, and never
/// which element of which list held it.
#[derive(Debug, Error)]
#[error("{op}: the request body does not fit the schema the document declares at `{path}`")]
pub struct BodyError {
    pub op: String,
    pub path: String,
    #[source]
    pub source: serde_json::Error,
}

/// Does `body` deserialise into `T`?
///
/// Generated code calls this with the type its own wrapper takes for the
/// operation, which is how a body assembled on a command line is held to the
/// same schema as a body passed from Rust — before a request is built.
pub fn fits<T: DeserializeOwned>(op: &str, body: &serde_json::Value) -> Result<(), BodyError> {
    serde_path_to_error::deserialize::<_, T>(body)
        .map(drop)
        .map_err(|refused| BodyError {
            op: op.to_owned(),
            path: refused.path().to_string(),
            source: refused.into_inner(),
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

#[cfg(test)]
mod tests {
    #![expect(clippy::unwrap_used, reason = "a failed unwrap is a failing test")]

    use serde::Deserialize;
    use serde_json::json;

    use super::fits;

    /// A body with a list of objects in it, which is the shape whose refusal
    /// is hardest to place by eye: serde's own message names the value and the
    /// type it wanted, and not which element of which list held it.
    #[derive(Debug, Deserialize)]
    struct Save {
        #[expect(dead_code, reason = "read only by the deserialiser under test")]
        positions: Vec<Position>,
    }

    #[derive(Debug, Deserialize)]
    struct Position {
        #[expect(dead_code, reason = "read only by the deserialiser under test")]
        flag: bool,
    }

    #[test]
    fn a_body_that_does_not_fit_names_the_place_it_stopped_at() {
        let body = json!({"positions": [{"flag": true}, {"flag": "1"}]});

        let refused = fits::<Save>("save", &body).unwrap_err();

        assert_eq!(refused.path, "positions[1].flag");
        assert!(
            refused.to_string().contains("`positions[1].flag`"),
            "the refusal does not say where the body went wrong: {refused}"
        );
        assert_eq!(
            refused.source.to_string(),
            "invalid type: string \"1\", expected a boolean",
            "serde's own reading of the value was lost"
        );
    }

    #[test]
    fn a_field_missing_from_the_top_of_the_body_is_placed_at_the_body_itself() {
        let refused = fits::<Save>("save", &json!({})).unwrap_err();

        assert_eq!(refused.path, ".");
        assert_eq!(refused.source.to_string(), "missing field `positions`");
    }

    #[test]
    fn a_body_that_fits_is_not_refused() {
        assert!(fits::<Save>("save", &json!({"positions": [{"flag": false}]})).is_ok());
    }
}
