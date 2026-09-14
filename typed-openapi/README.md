# typed-openapi

Typed Rust calls and a clap command tree, both built from one OpenAPI document,
with every write behind a dry-run gate.

What comes out is `api.get_voucher(5)?.send(&client)?` returning your own
`Voucher`, and `toy get-voucher --id 5` on a command line nobody wrote. What
goes in is the vendor's document plus an [OpenAPI Overlay][overlay] holding your
corrections to it. This is not a typed model *of* an OpenAPI document — for
that, use [`openapiv3`].

```console
$ toy create-voucher --total 12.50 --currency EUR --status open
POST /vouchers HTTP/1.1
host: localhost:9999
content-type: application/json

{"total":"12.50","currency":"EUR","status":"open"}

dry run: nothing was sent. Add --commit to send it.
```

## Two consumers, one document

**The Rust caller** gets owned types and one method per operation, with your own
newtypes substituted in: `Voucher.total` is a `Money`, not a `String`, with no
mirror type and no conversion at the boundary.

**The CLI consumer** gets a subcommand per operation, flags from the parameters
and the request body, values checked against the document's own formats and
enums, and dynamic shell completion. Mount the operations under `raw`, under any
other name, or as the whole CLI:

```rust,ignore
let matches = Command::new("toy")
    .subcommands(tree::commands(api.document()))
    .get_matches();

match tree::dispatch(api.document(), api.base(), &client, &matches)? {
    Outcome::Sent(response) => ...,
    Outcome::DryRun(request) => ...,   // a write nobody confirmed
}
```

Both go through the same request builder, so the CLI and the typed caller cannot
disagree about what an operation is.

## The gate

A read runs on sight. A write prints the exact bytes it would have sent and
stops, until `--commit`. Which operations write is the document's answer rather
than a guess from the HTTP method: a `GET` that stores a PDF is marked
`x-cli-writes` in your Overlay and is gated like any `POST`. The gate is
default-closed, and an operation the document does not describe has no
subcommand at all.

This matters most when the CLI's user is an agent, which has to learn caution
from the tool rather than bring it.

## The bless step

One command turns the vendor's document and your Overlay into four committed
files: the corrected document, the schemas as Rust types, one typed wrapper per
operation, and the document already reduced to what a command line needs. A
shipped binary reads that reduction — it parses no YAML and links no OpenAPI
object model.

The generator ships inside this crate behind the `generate` feature, so your
`xtask` is about twenty lines. See [docs/generating.md][gen].

A vendor revision that moves something you corrected fails the bless step
instead of silently overwriting the correction, and one that moves something you
*use* fails the compiler. See [docs/drift.md][drift].

## Features

| feature | default | adds |
|---|---|---|
| `clap` | yes | `tree`: the command tree, and `ArgMatches` back to a sent request |
| `document` | no | `Document::load`, the Overlay engine, the `$ref` resolver |
| `generate` | no | the code generator a bless step calls. Implies `document` |
| `builder` | no | a named-argument builder on the generated wrappers |

Each feature adds and removes whole items and never changes one, so a match that
is exhaustive in one build is exhaustive in all of them.

**No HTTP client, in any combination.** The seam is a two-method trait over
`http::Request<Vec<u8>>`. Adapters for ureq 3 and `reqwest::Client` are about
ten lines each and live in [`examples/toy/cli/src/client.rs`][adapters], written
to be copied rather than depended on. The default feature set is 30 crates; 21
without `clap`.

## What it does not do

- **No authentication.** Sign the `http::Request` in your own adapter.
- **No `oneOf` / `allOf` / `anyOf` request bodies.** A nested body is
  `--json-body FILE`; per-field flags exist only for flat ones.
- **No array or object query parameters**, and no `style` / `explode`.
- **No async CLI.** The command tree is sync; `AsyncClient` is for the typed
  caller.
- **The reduced model is a binary blob.** It is diffable only by regenerating
  it, not by reading it.
- **Rust 1.85**, set between them by clap 4.6, hashbrown 0.17 and edition 2024.
  Stable throughout; only this repository's own formatter and coverage recipes
  want nightly.

## Where to go next

| | |
|---|---|
| [docs/generating.md][gen] | the bless step, the `Settings` interface, wiring your own `xtask` |
| [docs/overlay.md][ov] | writing corrections as Overlay actions, and the tripwire form |
| [docs/cli.md][cli] | mounting the tree, the gate, flag naming, completion |
| [docs/builders.md][bu] | the `builder` feature and what it costs |
| [docs/drift.md][drift] | every way a vendor revision is caught, and where |
| [`examples/toy`][ex] | one adoption end to end, built and tested by CI |

Licensed under either of Apache-2.0 or MIT, at your option.

[overlay]: https://spec.openapis.org/overlay/v1.0.0.html
[`openapiv3`]: https://docs.rs/openapiv3
[gen]: https://github.com/stfl/typed-openapi/blob/main/docs/generating.md
[drift]: https://github.com/stfl/typed-openapi/blob/main/docs/drift.md
[ov]: https://github.com/stfl/typed-openapi/blob/main/docs/overlay.md
[cli]: https://github.com/stfl/typed-openapi/blob/main/docs/cli.md
[bu]: https://github.com/stfl/typed-openapi/blob/main/docs/builders.md
[ex]: https://github.com/stfl/typed-openapi/tree/main/examples/toy
[adapters]: https://github.com/stfl/typed-openapi/blob/main/examples/toy/cli/src/client.rs
