# typed-openapi

Typed Rust calls and a clap command tree, both built from one OpenAPI document,
with every write behind a dry-run gate.

What comes out is `api.get_voucher(5)?.send(&client)?` returning your own
`Voucher`, and `toy vouchers get --id 5` on a command line nobody wrote. What
goes in is the vendor's document plus [OpenAPI Overlay 1.1][overlay] documents
holding your corrections to it. This is not a typed model *of* an OpenAPI
document — for that, use [`openapiv3`].

```console
$ toy vouchers create --total 12.50 --currency EUR --status open  # operations at the root
POST /vouchers HTTP/1.1
host: localhost:9999
content-type: application/json

{"total":"12.50","currency":"EUR","status":"open"}

dry run: nothing was sent. Add --commit to send it.
```

## Two consumers, one document

**The Rust caller** gets owned types and one method per operation, with a
newtype wherever the document names a rule — `Voucher.currency` is a `Currency`
and cannot be built out of something that is not an ISO 4217 code — and a type
of *your* own wherever it cannot: `Voucher.total` is a fixed-point `Money`,
which no OpenAPI document has a way to describe.

**The CLI consumer** gets a two-level tree — one subcommand per resource the
document's paths name, one per operation under it, so `PUT /vouchers/{id}` is
`vouchers update` whatever the vendor called it — with flags from the parameters
and the request body, and dynamic shell completion. Mount it under `raw`, under
any other name, or as the whole CLI: two calls either way, in
[docs/cli.md][cli].

Both go through the same request builder, so the CLI and the typed caller cannot
disagree about what an operation is — and both run the document's `pattern` on
the same regex engine, so they cannot disagree about what a value is either:

```console
$ toy vouchers create --total 1,50 --currency EUR --status open
error: invalid value '1,50' for '--total <STRING>': `1,50` does not match ^-?[0-9]+(\.[0-9]{1,2})?$
```

Every rule the document states about a value is enforced, in the document's own
numbers — `pattern`, the lengths, the bounds, `multipleOf` — on both roads. What
that costs is a fifth of the binary: [docs/validation.md][val].

## The gate

A read runs on sight. A write prints the exact bytes it would have sent and
stops, until `--commit`. Which operations write is the document's answer rather
than a guess from the HTTP method: a `GET` that stores a PDF is marked
`x-cli-writes` in your Overlay and is gated like any `POST`. The gate is
default-closed, and an operation the document does not describe has no
subcommand at all.

One word is not always enough. An operation that cannot be undone, or that
reaches a third party, names its own gates in the document: `x-cli-gates:
[enshrine]` grows a required `--enshrine` demanded *beside* `--commit`, so the
hazard is typed out before the request is built.

This matters most when the CLI's user is an agent, which has to learn caution
from the tool rather than bring it.

## The bless step

One command turns the vendor's document and your Overlays into five committed
files: the corrected document, the schemas as Rust types, one typed wrapper per
operation, the document reduced to what a command line needs, and a page
counting what that reduction did. A shipped binary reads that reduction — it
parses no YAML and links no OpenAPI object model.

The count is there so that a number in your own docs — how many operations,
how many writes, what stands behind each gate — is a measurement your tests can
assert against rather than one somebody typed.

Corrections come in layers, applied in the order you name them, so the one that
repairs the vendor's mistakes stays a document worth handing back to the vendor
while the one that marks operations for a command line sits above it. The
generator ships inside this crate behind the `generate` feature, so your `xtask`
is about twenty lines — see [docs/generating.md][gen].

A vendor revision that moves something you corrected fails the bless step,
naming the layer it is in, rather than silently overwriting the correction; one
that withdraws an operation or a field your code names fails the compiler. What
is *not* caught is listed in [docs/drift.md][drift] beside what is.

## Features

| feature | default | adds |
|---|---|---|
| `clap` | yes | `tree`: the command tree, and `ArgMatches` back to a sent request |
| `document` | no | `Document::load`, the Overlay engine, the `$ref` resolver |
| `generate` | no | the code generator a bless step calls. Implies `document` |
| `builder` | no | a named-argument builder **beside** each generated wrapper |

Each feature adds and removes whole items and never changes one, so a match
exhaustive in one build is exhaustive in all.

**No HTTP client, in any combination.** The seam is a two-method trait over
`http::Request<Vec<u8>>`, and an adapter is about ten lines. The default feature
set is 30 crates; 21 without `clap`. The one client the crate does ship is
`Recorder`, which sends nothing and answers from a script, so a test of anything
above the seam needs no socket and no fixture server. Both are
[docs/client.md][client].

## What it does not do

- **No authentication.** Sign the `http::Request` in your adapter.
- **No per-field flags for a nested body.** A body that is nested, `oneOf`,
  `allOf` or `anyOf` is `--json-body FILE` on the command line, with
  `--json-body-template` to print the skeleton that goes in it; flags exist
  only for flat ones. The typed wrapper takes the generated type either way.
- **No object parameters.** A list of scalars is a repeatable flag, laid out by
  its own `style` and `explode`. An object, an `in: cookie`, a
  `content`-described parameter, an unserialisable `style` and a name that will
  not kebab-case carry no flag: each is named on its subcommand's help, and
  refuses the document only where the document requires it.
- **No async CLI.** The command tree is sync; `AsyncClient` is for the typed
  caller.
- **No check on a whole-body file.** `--json-body FILE` is held to being JSON
  and no further; holding its *content* to a schema needs the generated
  `struct`, which only your crate can name.
- **A regex engine in every binary.** Enforcing `pattern` costs one and no
  feature removes it: 810 KB of the example's 4.6 MB stripped binary.
- **The reduced model is a binary blob**, diffable only by regenerating it.
- **Rust 1.88**, in every feature set, set by the regex engine. Stable
  throughout — only this repository's own recipes want nightly.

## Where to go next

`typed-openapi/` is the published crate and the only thing on crates.io;
everything under `examples/toy/` is one adoption of it, held by the same
`just gate` CI runs — formatting, clippy, rustdoc, every feature combination,
the tests, and the crate built from its own tarball.

| | |
|---|---|
| [docs/generating.md][gen] | the bless step, the `Settings` interface, wiring your own `xtask` |
| [docs/overlay.md][ov] | writing corrections as Overlay actions, and the tripwire form |
| [docs/adoption.md][ad] | taking this to a vendor's document: the judgement calls no check reaches |
| [docs/cli.md][cli] | mounting the tree, the gate, flag naming, completion |
| [docs/client.md][client] | the client seam, writing an adapter, `Recorder` |
| [docs/validation.md][val] | every rule that is enforced, where it runs, and what the engine costs |
| [docs/builders.md][bu] | the `builder` feature and what it costs |
| [docs/drift.md][drift] | every way a vendor revision is caught, and where |
| [`examples/toy`][ex] | one adoption end to end, built and tested by CI |

Licensed under either of Apache-2.0 or MIT, at your option.

[overlay]: https://spec.openapis.org/overlay/v1.1.0.html
[`openapiv3`]: https://docs.rs/openapiv3
[gen]: https://github.com/stfl/typed-openapi/blob/main/docs/generating.md
[drift]: https://github.com/stfl/typed-openapi/blob/main/docs/drift.md
[ov]: https://github.com/stfl/typed-openapi/blob/main/docs/overlay.md
[ad]: https://github.com/stfl/typed-openapi/blob/main/docs/adoption.md
[cli]: https://github.com/stfl/typed-openapi/blob/main/docs/cli.md
[val]: https://github.com/stfl/typed-openapi/blob/main/docs/validation.md
[bu]: https://github.com/stfl/typed-openapi/blob/main/docs/builders.md
[client]: https://github.com/stfl/typed-openapi/blob/main/docs/client.md
[ex]: https://github.com/stfl/typed-openapi/tree/main/examples/toy
