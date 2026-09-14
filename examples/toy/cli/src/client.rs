//! The HTTP client adapters.
//!
//! Both are the whole adapter. The seam is `http::Request<Vec<u8>>` in,
//! `http::Response<Vec<u8>>` out, so there is nothing else to write.

use typed_openapi::transport::{HttpRequest, HttpResponse};

/// ureq 3, the client this binary ships with.
///
/// `http_status_as_error(false)` is load-bearing: ureq otherwise turns a 4xx
/// into an error and throws the body away, and the body is what an agent needs.
/// The status belongs to `api`, which maps it into [`api::Error::Status`].
#[derive(Debug)]
pub struct Ureq(ureq::Agent);

impl Default for Ureq {
    fn default() -> Self {
        Self::new()
    }
}

impl Ureq {
    #[must_use]
    pub fn new() -> Self {
        Self(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
        )
    }
}

impl typed_openapi::SyncClient for Ureq {
    type Error = ureq::Error;

    fn send(&self, request: HttpRequest) -> Result<HttpResponse, ureq::Error> {
        let (parts, mut body) = self.0.run(request)?.into_parts();
        Ok(HttpResponse::from_parts(parts, body.read_to_vec()?))
    }
}

/// The same seam, for a caller inside a tokio runtime. Built by
/// `cargo check -p cli --features reqwest-client`; nothing else in this binary
/// changes, which is the point of the trait.
///
/// The newtype is the orphan rule, not a design choice: `AsyncClient` and
/// `reqwest::Client` are both foreign to this crate. A crate that owns either
/// one writes `impl AsyncClient for reqwest::Client` directly.
#[cfg(feature = "reqwest-client")]
#[derive(Debug, Default)]
pub struct Reqwest(pub reqwest::Client);

#[cfg(feature = "reqwest-client")]
impl typed_openapi::AsyncClient for Reqwest {
    type Error = reqwest::Error;

    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, reqwest::Error> {
        let response = self.0.execute(request.try_into()?).await?;
        let mut out = HttpResponse::new(Vec::new());
        *out.status_mut() = response.status();
        *out.version_mut() = response.version();
        *out.headers_mut() = response.headers().clone();
        *out.body_mut() = response.bytes().await?.to_vec();
        Ok(out)
    }
}
