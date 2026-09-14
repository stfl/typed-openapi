//! One operation plus values that satisfy it, and the request that falls out.
//!
//! [`Invocation::new`] is the only way to make one, and it validates: an
//! `Invocation` that exists names an operation, carries every required
//! parameter, carries nothing the operation does not declare, and holds a body
//! of the kind the operation asks for. [`Invocation::request`] is then a
//! rendering, not a decision — it can only fail on a base URL that is not a URL.
//!
//! This is the only request builder in the workspace. A CLI reaches it through
//! the `tree` module (feature `clap`); a generated Rust wrapper reaches it by building a
//! [`Values`] directly. Neither spells a path template twice.

use http::{Request, Uri, header};
use thiserror::Error;

use crate::model::{Body, Effect, Location, Operation};
use crate::multipart;
use crate::scalar::ScalarError;
use crate::values::{Payload, Values};

/// Values the document rejects for this operation.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ValueError {
    #[error("{op}: `{name}` is required")]
    MissingParam { op: String, name: String },
    #[error("{op}: there is no `{name}` parameter")]
    UnknownParam { op: String, name: String },
    #[error("{op}: `{name}`: {source}")]
    BadValue {
        op: String,
        name: String,
        #[source]
        source: ScalarError,
    },
    #[error("{op}: a request body is required")]
    MissingBody { op: String },
    #[error("{op}: takes no request body")]
    UnexpectedBody { op: String },
    #[error("{op}: expects a {expected} body")]
    WrongBodyKind { op: String, expected: String },
}

/// An operation with values that satisfy it.
#[derive(Debug, Clone)]
pub struct Invocation<'a> {
    op: &'a Operation,
    values: Values,
}

impl<'a> Invocation<'a> {
    /// Check `values` against `op`.
    pub fn new(op: &'a Operation, values: Values) -> Result<Self, ValueError> {
        let name = || op.command().to_string();
        for (wire, raw) in values.params() {
            let param = op.param(wire).ok_or_else(|| ValueError::UnknownParam {
                op: name(),
                name: wire.clone(),
            })?;
            param
                .scalar()
                .parse(raw)
                .map_err(|source| ValueError::BadValue {
                    op: name(),
                    name: wire.clone(),
                    source,
                })?;
        }
        for param in op.params() {
            let given = values.params().iter().any(|(wire, _)| wire == param.name());
            if param.required() && !given {
                return Err(ValueError::MissingParam {
                    op: name(),
                    name: param.name().to_owned(),
                });
            }
        }
        check_body(op, values.payload())?;
        Ok(Self { op, values })
    }

    #[must_use]
    pub fn operation(&self) -> &'a Operation {
        self.op
    }

    /// `Write` means a CLI must see `--commit` before this is sent.
    #[must_use]
    pub fn effect(&self) -> Effect {
        self.op.effect()
    }

    /// The request this invocation stands for, against `base`.
    pub fn request(&self, base: &Uri) -> Result<Request<Vec<u8>>, http::Error> {
        let mut builder = Request::builder()
            .method(self.op.method().clone())
            .uri(self.url(base));
        for (name, value) in self.located(Location::Header) {
            builder = builder.header(name, value);
        }
        match self.body() {
            None => builder.body(Vec::new()),
            Some((content_type, bytes)) => builder
                .header(header::CONTENT_TYPE, content_type)
                .body(bytes),
        }
    }

    /// The `Content-Type` and the bytes, or `None` for a body-less request.
    fn body(&self) -> Option<(String, Vec<u8>)> {
        match self.values.payload()? {
            Payload::Json(value) => Some((
                "application/json".to_owned(),
                value.to_string().into_bytes(),
            )),
            Payload::Raw(bytes) => {
                // The media type is the document's, never the caller's.
                let media_type = match self.op.body() {
                    Body::Opaque { media_type, .. } => media_type.clone(),
                    Body::None
                    | Body::JsonFields(_)
                    | Body::JsonWhole { .. }
                    | Body::Multipart { .. } => "application/octet-stream".to_owned(),
                };
                Some((media_type, bytes.clone()))
            }
            Payload::Multipart(parts) => {
                let encoded = multipart::encode(parts);
                Some((encoded.content_type, encoded.bytes))
            }
        }
    }

    fn url(&self, base: &Uri) -> String {
        let mut url = String::new();
        if let Some(scheme) = base.scheme_str() {
            url.push_str(scheme);
            url.push_str("://");
        }
        if let Some(authority) = base.authority() {
            url.push_str(authority.as_str());
        }
        url.push_str(base.path().trim_end_matches('/'));

        let mut path = self.op.path().to_owned();
        for (name, value) in self.located(Location::Path) {
            path = path.replace(&format!("{{{name}}}"), &encode(value));
        }
        url.push_str(&path);

        let query: Vec<String> = self
            .located(Location::Query)
            .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
            .collect();
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        url
    }

    fn located(&self, location: Location) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .params()
            .iter()
            .filter_map(move |(name, value)| {
                let param = self.op.param(name)?;
                (param.location() == location).then_some((name.as_str(), value.as_str()))
            })
    }
}

/// Does the body the caller brought match the body the operation asks for?
fn check_body(op: &Operation, body: Option<&Payload>) -> Result<(), ValueError> {
    let name = || op.command().to_string();
    let wrong = |expected: &str| {
        Err(ValueError::WrongBodyKind {
            op: name(),
            expected: expected.to_owned(),
        })
    };
    match (op.body(), body) {
        (
            Body::None
            | Body::JsonFields(_)
            | Body::JsonWhole { required: false }
            | Body::Multipart {
                required: false, ..
            }
            | Body::Opaque {
                required: false, ..
            },
            None,
        )
        | (Body::JsonFields(_) | Body::JsonWhole { .. }, Some(Payload::Json(_)))
        | (Body::Multipart { .. }, Some(Payload::Multipart(_)))
        | (Body::Opaque { .. }, Some(Payload::Raw(_))) => Ok(()),
        (Body::None, Some(_)) => Err(ValueError::UnexpectedBody { op: name() }),
        (
            Body::JsonWhole { required: true }
            | Body::Multipart { required: true, .. }
            | Body::Opaque { required: true, .. },
            None,
        ) => Err(ValueError::MissingBody { op: name() }),
        (Body::JsonFields(_) | Body::JsonWhole { .. }, Some(_)) => wrong("JSON"),
        (Body::Multipart { .. }, Some(_)) => wrong("multipart/form-data"),
        (Body::Opaque { media_type, .. }, Some(_)) => wrong(media_type),
    }
}

/// Percent-encode everything outside RFC 3986's unreserved set. Both path
/// segments and query values are safe under that rule; nothing this crate puts
/// in a URL is meant as a delimiter.
fn encode(raw: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The request as it goes on the wire, for a dry run.
///
/// A binary body is summarised rather than printed: an agent reading a dry run
/// needs the headers and the length, not the bytes of a PDF.
#[must_use]
pub fn render(request: &Request<Vec<u8>>) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {} HTTP/1.1",
        request.method(),
        request
            .uri()
            .path_and_query()
            .map_or("/", http::uri::PathAndQuery::as_str)
    );
    if let Some(authority) = request.uri().authority() {
        let _ = writeln!(out, "host: {authority}");
    }
    for (name, value) in request.headers() {
        let _ = writeln!(out, "{name}: {}", value.to_str().unwrap_or("<non-utf8>"));
    }
    if !request.body().is_empty() {
        out.push('\n');
        match std::str::from_utf8(request.body()) {
            Ok(text) => {
                out.push_str(text);
                if !text.ends_with('\n') {
                    out.push('\n');
                }
            }
            Err(_) => {
                let _ = writeln!(out, "<{} bytes>", request.body().len());
            }
        }
    }
    out
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    #[test]
    fn encoding_leaves_the_unreserved_set_alone_and_escapes_the_rest() {
        assert_eq!(encode("abc-123_x.y~z"), "abc-123_x.y~z");
        assert_eq!(encode("a b/c?d&e=f"), "a%20b%2Fc%3Fd%26e%3Df");
        assert_eq!(encode("Grüß"), "Gr%C3%BC%C3%9F");
    }

    #[test]
    fn a_binary_body_is_summarised_rather_than_printed() {
        let request = Request::builder()
            .uri("http://x/y")
            .body(vec![0xFF, 0xFE])
            .expect("a request with a two-byte body");
        assert!(
            render(&request).ends_with("<2 bytes>\n"),
            "{}",
            render(&request)
        );
    }
}
