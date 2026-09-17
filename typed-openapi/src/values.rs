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
    content_type: Option<&'static str>,
    bytes: Vec<u8>,
}

impl Part {
    /// A text part: `--field name=value`.
    #[must_use]
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            filename: None,
            content_type: None,
            bytes: value.into().into_bytes(),
        }
    }

    /// A file part: `--file name=@path`. `filename` is what the server sees.
    ///
    /// The part is sent under the media type the filename's extension names —
    /// [`media_type_of`] — because a server that stores an upload keeps the
    /// type the part declared, and a PDF declared as `application/octet-stream`
    /// is stored as bytes rather than as a document.
    #[must_use]
    pub fn file(name: impl Into<String>, filename: impl Into<String>, bytes: Vec<u8>) -> Self {
        let filename = filename.into();
        Self {
            name: name.into(),
            content_type: Some(media_type_of(&filename)),
            filename: Some(filename),
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

    /// The media type a file part is sent under; a text part declares none.
    #[must_use]
    pub const fn content_type(&self) -> Option<&'static str> {
        self.content_type
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The media type a file called `filename` is sent under.
///
/// Read off the extension alone, whatever its case, and only for the document
/// types an upload is for: `pdf`, `png`, `jpg` and `jpeg`. Anything else,
/// and a name with no extension, is `application/octet-stream` — the type that
/// claims nothing about the bytes. The bytes themselves are not sniffed: the
/// name is what the caller chose to send, and a type guessed from content would
/// be a second opinion the caller cannot see on the command line.
#[must_use]
pub fn media_type_of(filename: &str) -> &'static str {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some(_) | None => "application/octet-stream",
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

    /// Every value of a list parameter, under the name the document gives it.
    ///
    /// A list reaches the request builder as the name repeated, which is exactly
    /// what a repeated flag reaches it with — so the two consumers hand over the
    /// same thing, and how the repeats are laid out in the request is the
    /// document's to say rather than either caller's. An empty list adds
    /// nothing, and is the same as not naming the parameter at all.
    #[must_use]
    pub fn each(
        mut self,
        name: impl Into<String>,
        values: impl IntoIterator<Item = impl ToString>,
    ) -> Self {
        let name = name.into();
        for value in values {
            self.params.push((name.clone(), value.to_string()));
        }
        self
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
