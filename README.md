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
and the request body, values held to every rule the document states about them,
and dynamic shell completion. Mount the tree under `raw`, under any other name,
or as the whole CLI:

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
disagree about what an operation is — and both run the document's `pattern` on
the same regex engine, so they cannot disagree about what a value is either:

```console
$ toy vouchers create --total 1,50 --currency EUR --status open
error: invalid value '1,50' for '--total <STRING>': `1,50` does not match ^-?[0-9]+(\.[0-9]{1,2})?$
```

`pattern`, `minLength`, `maxLength`, `minimum`, `maximum`, the two `exclusive`
flags and `multipleOf` are all enforced, in the document's own numbers. What
that costs is a fifth of the binary — see [docs/validation.md][val].

## The gate

A read runs on sight. A write prints the exact bytes it would have sent and
stops, until `--commit`. Which operations write is the document's answer rather
than a guess from the HTTP method: a `GET` that stores a PDF is marked
`x-cli-writes` in your Overlay and is gated like any `POST`. The gate is
default-closed, and an operation the document does not describe has no
subcommand at all.

One word is not always enough. An operation that cannot be undone, or that
reaches a third party, names its own gates — `x-cli-gates: [enshrine]` grows a
required `--enshrine` that is demanded *beside* `--commit`, so the hazard is
typed out before the request is even built.

This matters most when the CLI's user is an agent, which has to learn caution
from the tool rather than bring it.

## The bless step

One command turns the vendor's document and your Overlays into four committed
files: the corrected document, the schemas as Rust types, one typed wrapper per
operation, and the document already reduced to what a command line needs. A
shipped binary reads that reduction — it parses no YAML and links no OpenAPI
object model.

Corrections come in layers, applied in the order you name them, so the one that
repairs the vendor's mistakes can stay a document worth handing back to the
vendor while the one that marks operations for a command line sits above it:

```rust,ignore
Settings::new("spec/vendor.yaml")
    .overlay("spec/corrections.yaml")   // what the vendor got wrong
    .overlay("spec/cli.yaml")           // what only a command line needs
    .write_to("api-generated")?;
```

The generator ships inside this crate behind the `generate` feature, so your
`xtask` is about twenty lines. See [docs/generating.md][gen].

A vendor revision that moves something you corrected fails the bless step —
naming the layer it is in — instead of silently overwriting the correction, and one that moves or withdraws
an operation or a field your code names fails the compiler. What is *not* caught
— an operation the vendor adds, a field nobody destructures — is listed in
[docs/drift.md][drift] beside what is.

## Features

| feature | default | adds |
|---|---|---|
| `clap` | yes | `tree`: the command tree, and `ArgMatches` back to a sent request |
| `document` | no | `Document::load`, the Overlay engine, the `$ref` resolver |
| `generate` | no | the code generator a bless step calls. Implies `document` |
| `builder` | no | a named-argument builder **beside** each generated wrapper |

Each feature adds and removes whole items and never changes one, so a match that
is exhaustive in one build is exhaustive in all of them.

**No HTTP client, in any combination.** The seam is a two-method trait over
`http::Request<Vec<u8>>`. Adapters for ureq 3 and `reqwest::Client` are about
ten lines each and live in [`examples/toy/cli/src/client.rs`][adapters], written
to be copied rather than depended on. The default feature set is 30 crates; 21
without `clap`.

`Recorder` is the one client the crate does ship: it sends nothing, answers from
a script — queued for a route or for anything, a response or a failure — and
keeps every request it was given, so a test of anything above the seam needs no
socket, no runtime and no fixture server. It is a `SyncClient` and an
`AsyncClient` over one script, so both call paths run off one fixture.

## What it does not do

- **No authentication.** Sign the `http::Request` in your own adapter.
- **No `oneOf` / `allOf` / `anyOf` request bodies.** A nested body is
  `--json-body FILE`; per-field flags exist only for flat ones.
- **No object parameters.** A list of scalars is a repeatable flag, laid out by
  the parameter's own `style` and `explode`. An object, an `in: cookie`, a
  `content`-described parameter and anything declaring a `style` this crate does
  not serialise carry no flag at all: each is named on its subcommand's help,
  and refuses the document only where the document requires it.
- **No async CLI.** The command tree is sync; `AsyncClient` is for the typed
  caller.
- **No check on a whole-body file.** `--json-body FILE` is held to being JSON
  and no further; holding its *content* to a schema needs the generated
  `struct`, which only your crate can name. `tree::select` is the seam for it.
- **A regex engine in every binary.** Enforcing `pattern` costs one, there is no
  feature that removes it, and on the example it is 810 KB of a 4.6 MB stripped
  binary.
- **The reduced model is a binary blob.** It is diffable only by regenerating
  it, not by reading it.
- **Rust 1.88**, in every feature set. The floor is the regex engine a
  `pattern` is checked with: `regress` uses let-chains. Stable throughout —
  only this repository's own formatter and coverage recipes want nightly.

## This repository

| | |
|---|---|
| [`typed-openapi/`](https://github.com/stfl/typed-openapi/tree/main/typed-openapi) | the published crate, and the only thing on crates.io |
| [`examples/toy/`](https://github.com/stfl/typed-openapi/tree/main/examples/toy) | one adoption end to end: an Overlay, a bless step, the generated types and wrappers, and two CLIs over them |
| [`docs/`](https://github.com/stfl/typed-openapi/tree/main/docs) | reference pages |

`just gate` is the whole check: formatting, clippy, rustdoc, every feature
combination, the tests, and the crate built from its own tarball. CI runs those
recipes rather than a copy of them.

## Where to go next

| | |
|---|---|
| [docs/generating.md][gen] | the bless step, the `Settings` interface, wiring your own `xtask` |
| [docs/overlay.md][ov] | writing corrections as Overlay actions, and the tripwire form |
| [docs/cli.md][cli] | mounting the tree, the gate, flag naming, completion |
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
[cli]: https://github.com/stfl/typed-openapi/blob/main/docs/cli.md
[val]: https://github.com/stfl/typed-openapi/blob/main/docs/validation.md
[bu]: https://github.com/stfl/typed-openapi/blob/main/docs/builders.md
[ex]: https://github.com/stfl/typed-openapi/tree/main/examples/toy
[adapters]: https://github.com/stfl/typed-openapi/blob/main/examples/toy/cli/src/client.rs
