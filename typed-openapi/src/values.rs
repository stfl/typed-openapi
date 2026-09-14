//! The arguments for one operation, keyed on the names the document uses.
//!
//! This is the seam that lets one request builder serve two callers. A CLI
//! translates flags into a [`Values`]; a generated Rust wrapper builds one
//! directly from typed arguments. Neither knows about the other, and neither
//! re-spells a path template or a query string.

/// What goes in the request body. Which variant an operation wants is a
/// document fact, so the media type is not carried here — [`Body`] holds it.
///
/// [`Body`]: crate::Body
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// JSON, either assembled from per-field flags or taken whole from a file.
    Json(serde_json::Value),
    /// Bytes the CLI does not interpret, sent under the document's media type.
    Raw(Vec<u8>),
    /// `multipart/form-data`, assembled part by part.
    Multipart(Vec<Part>),
}

/// One part of a multipart body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    name: String,
    filename: Option<String>,
    bytes: Vec<u8>,
}

impl Part {
    /// A text part: `--field name=value`.
    #[must_use]
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            filename: None,
            bytes: value.into().into_bytes(),
        }
    }

    /// A file part: `--file name=@path`. `filename` is what the server sees.
    #[must_use]
    pub fn file(name: impl Into<String>, filename: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            filename: Some(filename.into()),
            bytes,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Parameters and a body for one operation, keyed on wire names.
///
/// Nothing here is checked against a document — that happens once, in
/// [`Invocation::new`], so there is exactly one place that decides what
/// satisfies an operation.
///
/// [`Invocation::new`]: crate::Invocation::new
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values {
    params: Vec<(String, String)>,
    body: Option<Payload>,
}

impl Values {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One parameter, under the name the document gives it.
    #[must_use]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "taking a reference would put `&` in front of every argument \
                  at every generated call site, for values that cost nothing to move"
    )]
    pub fn param(mut self, name: impl Into<String>, value: impl ToString) -> Self {
        self.params.push((name.into(), value.to_string()));
        self
    }

    /// The same, for an optional parameter: `None` adds nothing.
    #[must_use]
    pub fn maybe(self, name: impl Into<String>, value: Option<impl ToString>) -> Self {
        match value {
            Some(value) => self.param(name, value),
            None => self,
        }
    }

    #[must_use]
    pub fn json(mut self, value: serde_json::Value) -> Self {
        self.body = Some(Payload::Json(value));
        self
    }

    #[must_use]
    pub fn raw(mut self, bytes: Vec<u8>) -> Self {
        self.body = Some(Payload::Raw(bytes));
        self
    }

    #[must_use]
    pub fn multipart(mut self, parts: Vec<Part>) -> Self {
        self.body = Some(Payload::Multipart(parts));
        self
    }

    /// Attach `body` when there is one; leave the body alone when there is not.
    #[must_use]
    pub fn body(mut self, body: Option<Payload>) -> Self {
        if let Some(body) = body {
            self.body = Some(body);
        }
        self
    }

    #[must_use]
    pub fn params(&self) -> &[(String, String)] {
        &self.params
    }

    #[must_use]
    pub fn payload(&self) -> Option<&Payload> {
        self.body.as_ref()
    }
}
