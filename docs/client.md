# Sending the request

This crate builds requests and reads responses. It does not send anything, in
any feature combination, and it never will. The primer is
[the README](../README.md); what the command tree does with an outcome is
[docs/cli.md](cli.md).

## Contents

- [The seam](#the-seam)
- [Writing an adapter](#writing-an-adapter)
- [`Recorder`, the one client this crate ships](#recorder-the-one-client-this-crate-ships)

## The seam

Two traits over `http::Request<Vec<u8>>`, one for each call style:

```rust,ignore
pub trait SyncClient {
    type Error: std::error::Error + Send + Sync + 'static;
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, Self::Error>;
}

pub trait AsyncClient {
    type Error: std::error::Error + Send + Sync + 'static;
    fn send(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, Self::Error>> + Send;
}
```

`HttpRequest` and `HttpResponse` are `http::Request<Vec<u8>>` and
`http::Response<Vec<u8>>`. The future is `Send` so that a call can be
`tokio::spawn`ed.

Keeping the client out is what lets one generated crate serve a binary that
uses ureq and a service that uses `reqwest`, and it is why the default feature
set is 30 crates — 21 without `clap`. It is also where authentication goes:
nothing here signs a request, so your adapter does.

## Writing an adapter

An adapter is about ten lines.
[`examples/toy/cli/src/client.rs`](../examples/toy/cli/src/client.rs) holds one
for ureq 3 and one for `reqwest::Client`, written to be copied rather than
depended on — they are in the example precisely so that they are not a
dependency of the published crate.

**An adapter must not turn a status code into an error.** The body that came
with a 4xx is what a caller needs, and a `Call` answers with `Outcome::Sent`
carrying the response, status and all. ureq turns a 4xx into an error and
discards the body unless it is built with `http_status_as_error(false)`, which
is why the example's adapter sets it.

## `Recorder`, the one client this crate ships

`Recorder` sends nothing. It answers from a script — queued for a route or for
anything, a response or a failure — and keeps every request it was given, so a
test of anything above the seam needs no socket, no runtime and no fixture
server. It is a `SyncClient` *and* an `AsyncClient` over one script, so both
call paths run off one fixture.

| | |
|---|---|
| `answering_route(method, path, status, body)` | answer one route with a status and a JSON body |
| `answering_route_with(method, path, response)` | answer one route with a response you built |
| `failing_route(method, path, message)` | fail one route, as a transport would |
| `answering(status, body)` / `answering_with(response)` | answer anything |
| `failing(message)` | fail anything |
| `strict()` | panic on a request no queue has an answer for, rather than answering `200 {}` |
| `take()` | every request it was given |
| `unused()` | how many queued answers were never asked for |

`take()` is what turns a test of behaviour into a test of the bytes: build the
call, hand it the recorder, and assert on the request that came out.
`unused()` is the other half — a script that queued three answers and was asked
for one is usually a test that stopped early.

The failure a script queues carries only the message it was queued with.
`RecorderError::message` hands that text back unwrapped, so a test compares
against the constant it scripted rather than against a rendering.
