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

use crate::model::{Body, Join, Location, Operation, Param, Shape, Unsupported};
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
    /// A value for a parameter no flag and no wrapper argument can carry.
    /// Dropping it silently would send a request the caller did not ask for.
    #[error("{op}: `{name}` is {why}, so there is nowhere in the request to put a value for it")]
    UnsupportedParam {
        op: String,
        name: String,
        why: Unsupported,
    },
    /// Several values for a parameter the document declares one value for. The
    /// list the caller meant is not a list the document describes.
    #[error("{op}: `{name}` takes one value, and was given {given}")]
    RepeatedParam {
        op: String,
        name: String,
        given: usize,
    },
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
        let name = || op.id().to_owned();
        for (wire, raw) in values.params() {
            let param = op.param(wire).ok_or_else(|| ValueError::UnknownParam {
                op: name(),
                name: wire.clone(),
            })?;
            match param.shape() {
                Shape::Flag { scalar, .. } => {
                    scalar.parse(raw).map_err(|source| ValueError::BadValue {
                        op: name(),
                        name: wire.clone(),
                        source,
                    })?;
                }
                Shape::Unreachable(why) => {
                    return Err(ValueError::UnsupportedParam {
                        op: name(),
                        name: wire.clone(),
                        why: why.clone(),
                    });
                }
            }
        }
        for param in op.params() {
            let given = values
                .params()
                .iter()
                .filter(|(wire, _)| wire == param.name())
                .count();
            if param.required() && given == 0 {
                return Err(ValueError::MissingParam {
                    op: name(),
                    name: param.name().to_owned(),
                });
            }
            if given > 1 && !param.shape().repeatable() {
                return Err(ValueError::RepeatedParam {
                    op: name(),
                    name: param.name().to_owned(),
                    given,
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

    /// The request this invocation stands for, against `base`.
    pub fn request(&self, base: &Uri) -> Result<Request<Vec<u8>>, http::Error> {
        let mut builder = Request::builder()
            .method(self.op.method().clone())
            .uri(self.url(base));
        for sent in self.placed(Location::Header) {
            // A header's style is `simple`, which comma-separates a list. The
            // values are not percent-encoded on the way in: a header is not a
            // URL, and nothing here is a delimiter in it but the comma.
            builder = builder.header(sent.name, sent.values.join(","));
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
        for sent in self.placed(Location::Path) {
            // A path parameter's style is `simple`, which comma-separates a list
            // however it explodes, so there is one rendering here and no branch.
            let placeholder = format!("{{{}}}", sent.name);
            path = path.replace(&placeholder, &commas(&sent.values));
        }
        url.push_str(&path);

        let query = self.query();
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        url
    }

    /// The query string's fields, in the order the caller first named each
    /// parameter.
    ///
    /// A repeated flag is one parameter holding several values, so the values
    /// are grouped before they are rendered: `?embed=a&embed=b` and `?embed=a,b`
    /// are one list spelled two ways, and which one it is, is the document's to
    /// say.
    fn query(&self) -> Vec<String> {
        let mut fields: Vec<String> = Vec::new();
        for sent in self.placed(Location::Query) {
            let name = encode(sent.name);
            match sent.join {
                Some(Join::Pairs) => fields.extend(
                    sent.values
                        .iter()
                        .map(|value| format!("{name}={}", encode(value))),
                ),
                Some(Join::Commas) | None => {
                    fields.push(format!("{name}={}", commas(&sent.values)));
                }
            }
        }
        fields
    }

    /// Every value the caller gave for a parameter that goes in `location`,
    /// grouped under the parameter it belongs to, with the join the document
    /// declared for it.
    ///
    /// A parameter this CLI cannot supply never reaches here — `Invocation::new`
    /// refuses a value for one — so grouping is over the parameters that have a
    /// place in the request and nothing else.
    fn placed(&self, location: Location) -> Vec<Sent<'_>> {
        let mut out: Vec<Sent<'_>> = Vec::new();
        for (name, value) in self.values.params() {
            let Some(Shape::Flag {
                location: at, join, ..
            }) = self.op.param(name).map(Param::shape)
            else {
                continue;
            };
            if *at != location {
                continue;
            }
            match out.iter_mut().find(|sent| sent.name == name.as_str()) {
                Some(sent) => sent.values.push(value),
                None => out.push(Sent {
                    name,
                    join: *join,
                    values: vec![value],
                }),
            }
        }
        out
    }
}

/// One parameter on its way into the request: the wire name, how the document
/// joins repeats of it, and the values in the order the caller gave them.
struct Sent<'v> {
    name: &'v str,
    join: Option<Join>,
    values: Vec<&'v str>,
}

/// Does the body the caller brought match the body the operation asks for?
fn check_body(op: &Operation, body: Option<&Payload>) -> Result<(), ValueError> {
    let name = || op.id().to_owned();
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
            | Body::JsonWhole {
                required: false, ..
            }
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
            Body::JsonWhole { required: true, .. }
            | Body::Multipart { required: true, .. }
            | Body::Opaque { required: true, .. },
            None,
        ) => Err(ValueError::MissingBody { op: name() }),
        (Body::JsonFields(_) | Body::JsonWhole { .. }, Some(_)) => wrong("JSON"),
        (Body::Multipart { .. }, Some(_)) => wrong("multipart/form-data"),
        (Body::Opaque { media_type, .. }, Some(_)) => wrong(media_type),
    }
}

/// One parameter's values as a single field: each percent-encoded, joined by
/// commas.
///
/// This is what RFC 6570's `simple` gives a list, and what OpenAPI's `form` with
/// `explode: false` gives one. Encoding runs first, so a comma *inside* a value
/// is `%2C` and the comma *between* two values is the delimiter the document
/// asked for — whoever reads the request can tell them apart.
fn commas(values: &[&str]) -> String {
    values
        .iter()
        .copied()
        .map(encode)
        .collect::<Vec<_>>()
        .join(",")
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
/// needs the headers and the length, not the bytes of a PDF. A multipart body
/// keeps every part's headers, so the file an upload sends is named with its
/// type — see `body_text`.
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
        out.push_str(&body_text(request));
    }
    out
}

/// The body of a dry run, ending in a newline.
///
/// Text as it is. A multipart body that is not all text has its parts laid out
/// with only their non-text content summarised, because the part headers are
/// what says which file goes and as what. Any other body that is not text is
/// its length.
fn body_text(request: &Request<Vec<u8>>) -> String {
    let body = request.body();
    if let Ok(text) = std::str::from_utf8(body) {
        let mut rendered = text.to_owned();
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        return rendered;
    }
    request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|declared| declared.to_str().ok())
        .and_then(|declared| multipart::summarised(body, declared))
        .unwrap_or_else(|| format!("<{} bytes>\n", body.len()))
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

    /// A multipart body with a document in it is not text, and summarising the
    /// whole of it would hide the one thing a dry run of an upload has to show:
    /// which file goes, under which name and which type.
    #[test]
    fn a_multipart_body_renders_every_part_header_and_summarises_only_what_is_not_text() {
        let encoded = crate::multipart::encode(&[
            crate::values::Part::text("kind", "invoice"),
            crate::values::Part::file("file", "receipt.pdf", vec![0x25, 0x50, 0xFF, 0xFE]),
        ]);
        let request = Request::builder()
            .method("POST")
            .uri("http://x/upload")
            .header(http::header::CONTENT_TYPE, &encoded.content_type)
            .body(encoded.bytes)
            .expect("a multipart request");

        let rendered = render(&request);
        let boundary = encoded
            .content_type
            .rsplit_once("boundary=")
            .expect("the encoder names its boundary")
            .1;

        assert!(
            rendered.ends_with(&format!(
                "\n--{boundary}\r\n\
                 Content-Disposition: form-data; name=\"kind\"\r\n\
                 \r\n\
                 invoice\r\n\
                 --{boundary}\r\n\
                 Content-Disposition: form-data; name=\"file\"; filename=\"receipt.pdf\"\r\n\
                 Content-Type: application/pdf\r\n\
                 \r\n\
                 <4 bytes>\r\n\
                 --{boundary}--\r\n"
            )),
            "{rendered}"
        );
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
