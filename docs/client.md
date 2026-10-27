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
- [A typed call and the gate](#a-typed-call-and-the-gate)
- [What a call's error says](#what-a-calls-error-says)
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

**A credential is set in the adapter.** The request that `Invocation::request`
builds carries no `Authorization` header, whatever the document declares: a
header parameter named `Authorization`, `Accept` or `Content-Type` is dropped
while the document is reduced, as OpenAPI says it should be
([`docs/cli.md`](cli.md#headers-openapi-says-to-ignore)). The adapter adds the
header to the request it is handed, from wherever the adopter keeps the secret,
so the secret is in no flag and no shell history, and a dry run, which prints
the request before the adapter sees it, prints no secret.

**The base URL has no credential in it.** The host and the path of every request
come from the document's first server or from the base you name with
`Client::with_base` or the `base` argument of `Plan::build` and
`tree::dispatch`. A base with userinfo (`user:password@`) or a query is refused
when the request is built, with `RequestError::Base`, and a server URL in the
document is refused when it loads. Userinfo after a leading `//`
(`//user:password@host/`) is userinfo too, although the `http` crate reads such
a base as a path. These refusals print no part of the URL, and the `Debug` of a
`Client` and of a `Document` prints its base without the userinfo and the query.
A client would send the userinfo as an `Authorization` header and a dry run
would print it, and a query would be dropped from every request in silence. A
fragment never reaches a `Uri`, since the `http` crate drops it when it parses.
Put the credential in the adapter, as above.

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
a command line — or take it from `Call::request` on the typed side, and hand it
to the client's own `send`. Nothing is boxed and the error is the client's own
type:

```rust,ignore
let Plan::Send(request) = Plan::build(op, base, values, &answers)? else {
    // A write nobody confirmed. Print it; send nothing.
};
let response = client.send(request)?; // the client's own error, untouched
```

This route also keeps the gate away from the network: it decides with no socket
and no credential, so a dry run needs neither.

## A typed call and the gate

`Call::send` sends whatever it was built for: a Rust caller is trusted, and
nothing stops a write. A caller that wants the gate a command line has plans the
call with what it answered. The request is built once, in `plan`, and the
`Planned` that comes back holds it:

```rust,ignore
let answers = Answers::new().commit().gate("enshrine");
let planned = api.enshrine_voucher(5)?.plan(&answers)?;

println!("{}", render(planned.request()));   // what a dry run prints ...
let sent = planned.send(&client)?;           // ... is what is sent
```

`send` answers `Some` with the typed answer when the gate let the request
through, and `None` when it did not: a write that was not confirmed, or whose
gates were not all answered, is a dry run, and a dry run has nothing to send and
asks the client for nothing. `send_async` is the same inside a runtime.
`Planned::plan` is the verdict, `Plan::Send` or `Plan::DryRun`, for a caller who
wants to match on it.

The verdict and the request are one value. `Plan::decide` takes the
`Invocation`, which holds its operation, and builds the request from it, so no
signature takes an operation and a request side by side. Only `Call::plan` makes
a `Planned`, so it sends the request of the call it was planned from, and no
other.

## What a call's error says

Four errors carry the `operationId` as a field, `op`, and their messages begin
with it, so a log line needs no context to be read. A generated wrapper raises
`Encode` through `to_json` while it builds the arguments of a call, before the
call exists. `Transport`, `Status` and `Decode` come once the request goes to
the client:

| `client::Error` | message |
|---|---|
| `Encode { op, source }` | `createVoucher: cannot serialise the request body` |
| `Transport { op, source }` | `getVoucher: the transport failed` |
| `Status { op, status, body }` | `getVoucher: the server answered 404 Not Found` |
| `Decode { op, source, raw }` | `getVoucher: the response is not the shape the document promises` |

`Values`, which a call raises when the values do not satisfy the operation,
names the operation too, in the message of its `ValueError`: each of them begins
with it (``getVoucher: `id` is required``).

`Request`, which a call raises while it builds the request and before anything
is sent, names no operation. Its message says what is wrong with the base URL
(`RequestError::Base`). For `RequestError::Http` the message is
`cannot build the request`, and its cause is the error of the `http` crate.

No message of the four prints what the server sent, and the cause of a `Decode`
does not either. The body of a response is the server's: it can hold a
credential or a client's data, and an error is printed where a log reads it. It
stays in the error. `Status` has it in `body`, `Decode` has the text it could
not read in `raw`, and `Error::body()` reads whichever the error holds.

The cause of a `Decode` is a `DecodeError`. It holds the kind of failure
(`DecodeClass`: `Syntax`, the JSON reader refused the body; `Eof`, the body ends
early; or `Data`, the body does not fit the type), its place, and no byte of the
body:

```text
getVoucher: the response is not the shape the document promises
      caused by: the body does not fit the type at line 1 column 48
```

`serde_json` says more, `invalid type: string "…", expected i64`, and can quote
the value, the key or the variant that it refused. That is why the cause does
not say it, and it does not name the expected type either.

The place is the line and the column that `serde_json` gives over the text it
read. That text is the body, or `null` for an empty body, which has nothing in
`raw`. The line counts from 1. The column is the number of bytes of that line
before the place, so it counts bytes and not characters and it can fall inside a
character. The place is at or after the start of what the reader refused. Both
numbers are 0 where `serde_json` has no place to give. That happens for some
types in some positions, such as an untagged enum that no variant matches, an
internally tagged enum whose content is refused, a type whose `try_from`
refuses, and a type whose own `Deserialize` reads a value of another type and
then refuses it, like the type that typify writes for a string with a pattern.
Where the line is 0, read the whole body. `Error::body()` replaces bytes that
are not UTF-8 by U+FFFD, so after such bytes on the line the same column in it
can point earlier than the place in the body, and never later.

The class is that of the first failure the reader found as it read from the
start of the body. It says nothing about the text after the place: a body that
is cut off, or that goes on with text that is not JSON, can be `Syntax`, `Eof`
or `Data`.

- `Syntax` is what `serde_json` calls a syntax error. The text is not JSON, or
  it is JSON that the reader will not take, such as a number past the range of
  an `f64` or a nesting past its recursion limit where the type reads the value,
  `null` or a number where the type is an externally tagged enum, and a key that
  is not a number in quotes where the keys of a map are numbers.
- `Eof` is a text that ends inside a value, or before any value starts when it
  holds white space only.
- `Data` is a text that the reader took as JSON up to the place, and the type
  refuses something that the reader read, such as a string, a list or an object
  where the type takes an integer, `300` where it takes a `u8`, an object that
  lacks a property, a property or a variant that the type does not take, or a
  value that the type refuses itself.

The cause of a `Transport` is the error of the client that you passed, and it
prints what that client prints. A cause is printed once, on its own line, by
whoever walks `source()`.

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
