//! One operation plus values that satisfy it, and the request that falls out.
//!
//! [`Invocation::new`] is the only way to make one, and it validates: an
//! `Invocation` that exists names an operation, carries every required
//! parameter, carries nothing the operation does not declare, and holds a body
//! of the kind the operation asks for. Every value is held to the rules of its
//! schema and is kept as it was parsed, so `+5` is `5`. Three refusals are of
//! the request and not of the schema: a path value that is empty, `.` or `..`,
//! which a server reads as a step along the path and not as a name; a header
//! value that [`http::HeaderValue`] refuses; and a comma inside one item of a
//! header list, which the receiver splits in two.
//! [`Invocation::request`] is then a rendering, not a decision. It refuses a
//! base URL that holds userinfo or a query, because a request would send the one
//! and drop the other, and it hands on the `http` crate's refusal of anything
//! else it cannot build.
//!
//! This is the only request builder in the workspace. A CLI reaches it through
//! the `tree` module (feature `clap`); a generated Rust wrapper reaches it by building a
//! [`Values`] directly. Neither spells a path template twice.

use http::uri::Authority;
use http::{HeaderValue, Request, Uri, header};
use thiserror::Error;

use crate::model::{Body, Join, Location, Operation, Param, Shape, Unsupported};
use crate::multipart;
use crate::scalar::ScalarError;
use crate::values::{Payload, Values};

/// Values the document rejects for this operation.
///
/// Every variant names the operation by its `operationId` in `op`. The nine that
/// are about a parameter also carry it in `name`: the name the document gives
/// it, or, in [`ValueError::UnknownParam`], the name the caller gave, which the
/// document does not declare. The three about the body carry no `name`.
///
/// The three that refuse a path segment, a header value and a header list item
/// print no value of the caller's: a header value can be a credential. The
/// segment is one of three fixed words, so [`ValueError::PathSegment`] shows it.
/// The cause of [`ValueError::BadValue`] quotes the value.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueError {
    /// The operation declares the parameter required and no value came.
    #[error("{op}: `{name}` is required")]
    MissingParam {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The required parameter, as the document spells it on the wire.
        name: String,
    },
    /// A value arrived for a name the operation does not declare.
    #[error("{op}: there is no `{name}` parameter")]
    UnknownParam {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The name the caller gave, which the operation does not declare.
        name: String,
    },
    /// A value for a parameter no flag and no wrapper argument can carry.
    /// Dropping it silently would send a request the caller did not ask for.
    #[error("{op}: `{name}` is {why}, so there is nowhere in the request to put a value for it")]
    UnsupportedParam {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The parameter, as the document spells it on the wire.
        name: String,
        /// The reduction's reason this crate has no place for the parameter.
        why: Unsupported,
    },
    /// Several values for a parameter the document declares one value for. The
    /// list the caller meant is not a list the document describes.
    #[error("{op}: `{name}` takes one value, and was given {given}")]
    RepeatedParam {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The parameter, as the document spells it on the wire.
        name: String,
        /// How many values the caller gave for it.
        given: usize,
    },
    /// A value that the parameter's schema refuses. The cause says which rule.
    #[error("{op}: `{name}` was given a value the document refuses")]
    BadValue {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The parameter, as the document spells it on the wire.
        name: String,
        /// The rule of the parameter's schema that the value broke.
        #[source]
        source: ScalarError,
    },
    /// A path parameter whose value is empty, `.` or `..`. A server reads such
    /// a segment as a step along the path and not as a name, and a client may
    /// drop it before it sends, so the request would reach another resource than
    /// the one the value names. A list is judged as the segment it makes.
    #[error(
        "{op}: `{name}` is {}, which cannot be a path segment: a server reads it as a step along \
         the path and not as a name, so the request would reach another resource",
        shown(.value)
    )]
    PathSegment {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The path parameter, as the document spells it on the wire.
        name: String,
        /// The segment the value makes: empty, `.` or `..`.
        value: String,
    },
    /// A value for a header parameter that [`http::HeaderValue`] refuses: it
    /// holds a control character other than a tab. Left to the `http` crate the
    /// refusal comes late and names nothing, and a line break in a header is how
    /// a header is injected.
    #[error(
        "{op}: `{name}` was given a value that no header can carry: a header value holds no \
         control character but a tab"
    )]
    HeaderValue {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The header parameter, as the document spells it on the wire.
        name: String,
    },
    /// A comma inside one item of a header list. A header list is written with
    /// its items between commas and is not percent-encoded, so the receiver
    /// would read two items where the caller gave one.
    #[error(
        "{op}: `{name}` was given an item with a comma in it, and a header list is split at its \
         commas"
    )]
    HeaderItemComma {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The header parameter, as the document spells it on the wire.
        name: String,
    },
    /// A parameter given without one the document says has to come with it.
    #[error("{op}: `{name}` is given, and `{requires}` has to be given with it")]
    MissingCompanion {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// The parameter that was given, as the document spells it on the wire.
        name: String,
        /// The parameter that has to come with it and is absent, spelled the
        /// same way.
        requires: String,
    },
    /// The operation requires a request body and the caller brought none.
    #[error("{op}: a request body is required")]
    MissingBody {
        /// The `operationId` of the operation the values were checked against.
        op: String,
    },
    /// A body arrived for an operation the document gives none.
    #[error("{op}: takes no request body")]
    UnexpectedBody {
        /// The `operationId` of the operation the values were checked against.
        op: String,
    },
    /// A body of another kind than the operation's document declares.
    #[error("{op}: expects a {expected} body")]
    WrongBodyKind {
        /// The `operationId` of the operation the values were checked against.
        op: String,
        /// What the operation's body is: `JSON`, `multipart/form-data`, or the media
        /// type the document declares for an opaque body.
        expected: String,
    },
}

/// Why a base URL cannot carry a request.
///
/// A request is the base URL's scheme, host and path with the operation's path
/// and query after them. Anything else the URL holds is lost or leaked, so it is
/// refused, whether it comes from the document or from the caller. None of the
/// messages prints the URL, which is where a credential or a key would be.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum BaseUrlError {
    /// `user:password@` before the host. A client sends it as an `Authorization`
    /// header and a dry run would print it. A credential belongs in the client
    /// adapter (`docs/client.md`).
    #[error(
        "the base URL holds userinfo (`user:password@`), which a request would send as a \
         credential and a dry run would print"
    )]
    Userinfo,
    /// `?` and what follows it. The query of a request is the operation's own, so
    /// the base URL's would be dropped in silence.
    #[error(
        "the base URL holds a query (`?…`), which no request keeps: the query of a request is the \
         operation's own"
    )]
    Query,
    /// `#` and what follows it. A client does not send a fragment. An
    /// [`http::Uri`] drops one when it is parsed, so only the text of a server
    /// URL in a document holds one.
    #[error("the base URL holds a fragment (`#…`), which no client sends")]
    Fragment,
}

impl BaseUrlError {
    /// What a request cannot keep of `base`, userinfo first because it is the
    /// credential.
    fn of(base: &Uri) -> Result<(), Self> {
        if base
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'))
        {
            return Err(Self::Userinfo);
        }
        if base.query().is_some() {
            return Err(Self::Query);
        }
        Ok(())
    }

    /// What a request cannot keep of the server URL of a document, as the
    /// document spells it once its variables have their defaults.
    #[cfg(feature = "document")]
    pub(crate) fn of_server(text: &str, base: &Uri) -> Result<(), Self> {
        Self::of(base)?;
        if text.contains('#') {
            return Err(Self::Fragment);
        }
        Ok(())
    }
}

/// Why an invocation did not become a request.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RequestError {
    /// The base URL holds something a request cannot keep.
    #[error(transparent)]
    Base(#[from] BaseUrlError),
    /// The `http` crate refused the request: a base it cannot build a URI from, or
    /// a header name it cannot carry, which a document is refused for while it is
    /// loaded and a blob that was not written by a load can still hold.
    #[error("cannot build the request")]
    Http(#[from] http::Error),
}

/// An operation with values that satisfy it, in the text the request carries.
#[derive(Debug, Clone)]
pub struct Invocation<'a> {
    op: &'a Operation,
    /// The parameters under their wire names, each value as `Scalar::wire`
    /// wrote it, in the order the caller gave them.
    params: Vec<(String, String)>,
    body: Option<Payload>,
}

impl<'a> Invocation<'a> {
    /// Check `values` against `op`.
    pub fn new(op: &'a Operation, values: Values) -> Result<Self, ValueError> {
        let name = || op.id().to_owned();
        let params = rendered(op, &values)?;
        for param in op.params() {
            // A parameter with no flag is given nothing, even when another
            // parameter with its name is: the values under a name are the
            // first parameter's, and the loop above has already seen to them.
            let given = match param.shape() {
                Shape::Flag { .. } => values
                    .params()
                    .iter()
                    .filter(|(wire, _)| wire == param.name())
                    .count(),
                Shape::Unreachable(_) => 0,
            };
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
            if given > 0
                && let Some(requires) = param
                    .requires()
                    .iter()
                    .find(|requires| !values.params().iter().any(|(wire, _)| wire == *requires))
            {
                return Err(ValueError::MissingCompanion {
                    op: name(),
                    name: param.name().to_owned(),
                    requires: requires.clone(),
                });
            }
        }
        check_body(op, values.payload())?;
        let invocation = Self {
            op,
            params,
            body: values.into_body(),
        };
        invocation.check_segments()?;
        invocation.check_headers()?;
        Ok(invocation)
    }

    /// A path value that is empty, `.` or `..` is no name for a resource.
    fn check_segments(&self) -> Result<(), ValueError> {
        for sent in self.placed(Location::Path) {
            let segment = sent.values.join(",");
            if matches!(segment.as_str(), "" | "." | "..") {
                return Err(ValueError::PathSegment {
                    op: self.op.id().to_owned(),
                    name: sent.name.to_owned(),
                    value: segment,
                });
            }
        }
        Ok(())
    }

    /// A header value is what `HeaderValue` takes, and an item of a header list
    /// has no comma in it, because the list is written with commas between its
    /// items and a header is not percent-encoded.
    fn check_headers(&self) -> Result<(), ValueError> {
        let op = || self.op.id().to_owned();
        for sent in self.placed(Location::Header) {
            if sent
                .values
                .iter()
                .any(|item| HeaderValue::from_str(item).is_err())
            {
                return Err(ValueError::HeaderValue {
                    op: op(),
                    name: sent.name.to_owned(),
                });
            }
            if sent.join.is_some() && sent.values.iter().any(|item| item.contains(',')) {
                return Err(ValueError::HeaderItemComma {
                    op: op(),
                    name: sent.name.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// The operation the values were checked against.
    #[must_use]
    pub fn operation(&self) -> &'a Operation {
        self.op
    }

    /// The request this invocation stands for, against `base`.
    ///
    /// A `base` with userinfo or a query is refused, and so is every road to a
    /// request that passes through here: the gate, the typed call and the
    /// command line.
    pub fn request(&self, base: &Uri) -> Result<Request<Vec<u8>>, RequestError> {
        BaseUrlError::of(base)?;
        let mut builder = Request::builder()
            .method(self.op.method().clone())
            .uri(self.url(base));
        for sent in self.placed(Location::Header) {
            // A header's style is `simple`, which comma-separates a list. The
            // values are not percent-encoded on the way in: a header is not a
            // URL, and nothing here is a delimiter in it but the comma.
            builder = builder.header(sent.name, sent.values.join(","));
        }
        let request = match self.body() {
            None => builder.body(Vec::new()),
            Some((content_type, bytes)) => builder
                .header(header::CONTENT_TYPE, content_type)
                .body(bytes),
        };
        Ok(request?)
    }

    /// The `Content-Type` and the bytes, or `None` for a body-less request.
    fn body(&self) -> Option<(String, Vec<u8>)> {
        match self.body.as_ref()? {
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
        for (name, value) in &self.params {
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

/// Every value the caller gave, under its wire name and as the request carries
/// it, or the first one the operation refuses: a name it does not declare, a
/// parameter it cannot supply, or a value the document's rules refuse.
fn rendered(op: &Operation, values: &Values) -> Result<Vec<(String, String)>, ValueError> {
    let name = || op.id().to_owned();
    let mut params = Vec::with_capacity(values.params().len());
    for (wire, raw) in values.params() {
        let param = op.param(wire).ok_or_else(|| ValueError::UnknownParam {
            op: name(),
            name: wire.clone(),
        })?;
        match param.shape() {
            Shape::Flag { scalar, .. } => {
                let text = scalar.wire(raw).map_err(|source| ValueError::BadValue {
                    op: name(),
                    name: wire.clone(),
                    source,
                })?;
                params.push((wire.clone(), text));
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
    Ok(params)
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
        let _ = writeln!(out, "host: {}", host_of(authority));
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

/// The authority a request is sent to, which is the host and the port. Userinfo
/// is not part of it: a client sends it as a header, and a dry run that printed
/// it would put a credential in a log.
fn host_of(authority: &Authority) -> &str {
    authority
        .as_str()
        .rsplit_once('@')
        .map_or_else(|| authority.as_str(), |(_, host)| host)
}

/// A path segment that is no name, as a message shows it.
fn shown(segment: &str) -> String {
    if segment.is_empty() {
        "empty".to_owned()
    } else {
        format!("`{segment}`")
    }
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
