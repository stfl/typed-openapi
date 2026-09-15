# Sending the request

This crate builds requests and reads responses. It does not send anything, in
any feature combination, and it never will. The primer is
[the README](../README.md); what the command tree does with an outcome is
[docs/cli.md](cli.md).

## Contents

- [The seam](#the-seam)
- [A reference to a client is a client](#a-reference-to-a-client-is-a-client)
- [Writing an adapter](#writing-an-adapter)
- [Classifying a transport failure](#classifying-a-transport-failure)
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

## A reference to a client is a client

`&C` implements both traits wherever `C` does, which is what the `&self`
receiver on `send` promises. So a caller holding a reference passes it through
untouched, and a run that picks between a live client and a `Recorder` holds the
choice as a trait object and hands that to any `send` in the crate:

```rust,ignore
let client: &dyn SyncClient<Error = MyError> = if recording { &recorder } else { &live };

tree::dispatch(&document, document.base(), &client, &matches)?;
```

The extra `&` is not a slip: every `send` here is generic over `C: SyncClient`
and a generic parameter is sized, so the type that satisfies it is
`&dyn SyncClient<Error = MyError>` rather than the trait object itself.

`AsyncClient` returns `impl Future`, which makes it dyn-incompatible — there is
no `&dyn AsyncClient` to hold. A run choosing between two async clients holds
them in an enum of its own and implements `AsyncClient` on that.

These impls spend `&C` on the crate: an `impl SyncClient for &Theirs` in a crate
of your own collides with the blanket one. Write the impl on the client itself
and take the reference from here.

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

## Classifying a transport failure

`DispatchError::Transport` and `client::Error::Transport` carry the client's
error in a `Box<dyn Error + Send + Sync>`. The box is what keeps a type
parameter for the client off every signature that mentions either error, and
what lets an adopter's own error type absorb one whole.

Whether a failed request *left* — on a write, the difference between an
operation that did nothing and one that may have done everything — is a reading
of what a particular client's error means. This crate has no far side to read:
it sends nothing. So the reading belongs in the crate that wrote the adapter,
and a failure nobody has classified counts as one that may have arrived. That
is the conservative answer, and the only safe one to reach for by default: a
retry rule that treats every failure as harmless will re-send a write that
already landed.

Two routes reach the concrete error.

**Downcast it.** The box holds `C::Error` exactly as the client returned it. A
run that picks its client at run time holds the seam as a trait object, which
fixes the associated type — so the catch site names the error type and never a
client:

```rust,ignore
fn run(
    document: &Document,
    client: &dyn SyncClient<Error = MyError>,
    matches: &ArgMatches,
) -> Result<Outcome, DispatchError> {
    tree::dispatch(document, document.base(), &client, matches)
}

if let Err(DispatchError::Transport(boxed)) = run(&document, client, &matches)
    && let Some(mine) = boxed.downcast_ref::<MyError>()
{
    // `mine` is whatever the adapter put there, and `run` named no client.
}
```

**Send it yourself.** Build the request through `Plan` — `Selection::plan` from
a command line, `Call::request` on the typed side — and hand it to the client's
own `send`. Nothing is boxed and the error is the client's own type:

```rust,ignore
let Plan::Send(request) = Plan::build(op, base, values, &answers)? else {
    // A write nobody confirmed. Print it; send nothing.
};
let response = client.send(request)?; // the client's own error, untouched
```

This route also keeps the gate away from the network: it decides with no socket
and no credential, so a dry run needs neither.

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
| `failing_route(method, path, reach, message)` | fail one route, as a transport would |
| `answering(status, body)` / `answering_with(response)` | answer anything |
| `failing(reach, message)` | fail anything |
| `strict()` | panic on a request no queue has an answer for, rather than answering `200 {}` |
| `take()` | every request it was given |
| `unused()` | how many queued answers were never asked for |

`take()` is what turns a test of behaviour into a test of the bytes: build the
call, hand it the recorder, and assert on the request that came out.
`unused()` is the other half — a script that queued three answers and was asked
for one is usually a test that stopped early.

A scripted failure carries two things, and `RecorderError` hands both back:
`message()` is the sentence a client would have failed with, unwrapped, so a
test compares against the constant it scripted rather than against a rendering;
`reach()` is how far the request got.

`Reach` is a closed pair, and `failing` takes it because there is no safe
default:

| | |
|---|---|
| `Reach::NeverLeft` | the request never left — nothing was written, and it may be sent again |
| `Reach::NeverAnswered` | the request left and nothing came back — it may have done everything, and it may never be sent again |

On a write those are opposite instructions to a retry rule, which is why the
distinction is a state rather than a phrase inside the message: an adopter maps
a variant to a variant, and a reworded message cannot silently reclassify a
suite. A `Recorder` can state the reach because the script invented the failure.
It is no reading of what a failure means: a real client's failure is classified
by whoever wrote the adapter, and nothing above the seam classifies one.

The state survives `Error::Transport` and `DispatchError::Transport`:
`downcast_ref::<RecorderError>()` hands back the error the client returned,
`reach()` and all, so a test can drive both branches of a retry rule through
the boxed variant.
