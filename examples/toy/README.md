# The toy adoption

One adoption of [`typed-openapi`](../../typed-openapi) from end to end: a vendor
document that is wrong in six ways, two Overlay layers that correct it, a bless
step, and two CLIs over the result. It is built, linted and tested with the library on
every push, so it cannot rot into an example that no longer compiles.

The Toy Accounting API is invented, and nothing here reaches the network: the
default server is `http://localhost:9999`, every write is a dry run until
`--commit`, and the tests drive the real commands through a recording client
with no socket.

## Try it

```sh
cargo run -p cli -- --help                       # raw, finalize-voucher, completions
cargo run -p cli -- raw --help                   # one subcommand per resource
cargo run -p cli -- raw vouchers --help          # one per operation under it
cargo run -p cli -- raw vouchers get --help      # flags from the document
cargo run -p cli --example root -- --help        # the same operations as the whole CLI
just bless                                       # regenerate; the diff must be empty
```

The tree is two levels, and the names are the document's paths and methods
rather than its `operationId`s — `PUT /vouchers/{id}` is `vouchers update`
however the vendor spelled it, and the resource word the vendor repeats in
every name is said once, by the group:

```console
$ cargo run -p cli -- raw vouchers --help
Operations on vouchers

Usage: toy raw vouchers [OPTIONS] <COMMAND>

Commands:
  list           List vouchers
  create         Create a voucher
  get            Fetch one voucher
  update         Replace a voucher
  enshrine       Finalize a voucher (irreversible)
  render         Render the voucher to PDF and store it on the server (this GET
                 writes)
  send-by-email  Email the voucher to a recipient
  archive        Archive a voucher (undocumented; vendor ships it)
  help           Print this message or the help of the given subcommand(s)
```

[`../../docs/cli.md`](../../docs/cli.md) has the rule, and the two `x-cli-`
markers that overrule it.

A write prints what it would send and stops:

```console
$ cargo run -p cli -- raw vouchers render --id 5
GET /vouchers/5/render HTTP/1.1
host: localhost:9999
dry run: nothing was sent. Add --commit to send it.
```

The request is on stdout and the line explaining it is on stderr, so a script
that pipes the first gets the request and nothing else.

Two of the writes are more than one question. Finalizing cannot be undone and
mail cannot be recalled, so `spec/cli.yaml` stands each behind a word of its
own, and the word is required as well as `--commit`:

```console
$ cargo run -p cli -- raw vouchers enshrine --id 5 --commit
error: the following required arguments were not provided:
  --enshrine
```

That one is a `GET`. HTTP cannot say "this GET writes", so the Overlay does,
with `x-cli-writes` — and the gate treats it like any `POST`.

## What the vendor gets wrong, and where it is fixed

Every correction is a standard [OpenAPI Overlay][overlay] action, in one of two
layers applied in order. [`spec/toy.yaml`](spec/toy.yaml) — the vendor's
document — is never edited.

There are two files rather than one because they have different audiences.
[`spec/corrections.yaml`](spec/corrections.yaml) holds what is true of the API
and the vendor got wrong or left out, so applying it to the vendor's document
with any Overlay tool yields the document the vendor should have shipped —
useful to the vendor, to a generator for another language, or to a mock server.
[`spec/cli.yaml`](spec/cli.yaml) holds the `x-cli-` markers, which only this
crate reads. `api/tests/corrections.rs` asserts the first file carries no
`x-cli-` key, because one there would quietly spoil that.
[`../../docs/overlay.md`](../../docs/overlay.md) is the how-to, including the
third layer this adoption has no need of.

| the vendor | the correction | what it buys |
|---|---|---|
| declares `format: money` and never says what an amount is | an Overlay `update` naming a `Money` schema with the rule *and* the vendor's format in it, pointing `total` at it, and `xtask`'s `.replace("money", "money::Money")` | `--total` rejects `1,50`, and `Voucher.total` is a fixed-point type that adds |
| calls a bare string an ISO 4217 code, in prose nothing can run | an `update` naming a `Currency` schema with `^[A-Z]{3}$` in it, and pointing `currency` at it | `Voucher.currency` is a `Currency`, and `--currency` rejects `eur` |
| returns an `internal_ref` it never documented | an `update` adding the property | a struct field and a `--internal-ref` flag |
| ships `archiveVoucher` and documents it nowhere | an `update` adding the path | a wrapper and a subcommand, for no Rust at all |
| serves a `GET` that stores a PDF | `x-cli-writes: true`, in `spec/cli.yaml` | `vouchers render` is behind `--commit` |
| serves one upload under a media type nothing here assembles | left alone, and both uploads work | `--raw-body` for the `application/pdf` one, `--file` / `--field` for the multipart one |

Three of those actions are **tripwires** — two in the vendor layer, one in the
CLI layer: their JSONPath states what the vendor currently says, so under
`ErrorOnZeroMatch` a vendor revision that moves the thing being corrected fails
the bless step instead of being silently overwritten, naming the layer it is
in. [`../../docs/overlay.md`](../../docs/overlay.md) explains the form;
[`../../docs/drift.md`](../../docs/drift.md) is the whole table of what is
caught where.

[`api/src/corrections.rs`](api/src/corrections.rs) lists the same six as Rust,
one line each, and `api/tests/corrections.rs` holds that list to both documents
in both directions — a row that no longer describes a real difference fails, and
a real difference with no row fails.

## Why five crates

| crate | lines | written by | holds |
|---|---|---|---|
| [`money`](money) | 428 | the adopter | one type: a fixed-point amount, which the generated code names |
| [`api-generated`](api-generated) | 732 | `just bless`, except `client.rs` | the corrected document, the Rust types, one wrapper per operation, the reduced model |
| [`api`](api) | 1151 | the adopter | the crate an adopter's own code names: corrections, `Posting`, and everything re-exported |
| [`cli`](cli) | 1311 | the adopter | the `toy` binary, and `examples/root.rs` beside it |
| [`xtask`](xtask) | 50 | the adopter | the bless step — the generator itself ships in `typed-openapi` |

The split is about what recompiles, and about what can name what. An edit to
`api` rebuilds the adopter's own lines and not the generated volume beneath
them; `money` sits below `api-generated` because the generated source names it,
and `api-generated`'s whole dependency list is `serde`, `http`, `money` and the
library — nothing about a generator reaches it.

One type here is hand-written, and it is the one no OpenAPI document can
describe. OpenAPI has no fixed-point decimal, so [`money`](money/src/lib.rs)
owns a `Money` over whole cents — an arbitrary-precision count of them, so
`+`, `-` and `sum()` are exact and total and nothing has to be unwrapped — and
`xtask` tells the bless step that `format: money` means that type. The document
keeps the `pattern`, so a command line still refuses `1,50`; what `replace` adds
is the arithmetic, and a `Display` that prints `12,50` for a person while the
wire keeps `12.50`. `Voucher.currency` next door goes the other way — the
Overlay names a `Currency` schema and the generator writes the newtype, rule
included — and the two fields are side by side because neither route replaces
the other.

`num-bigint` is declared in `money`'s manifest and nowhere else: what an owned
type is made of is the adopter's business, never the library's, and
`just bigint-free` is the check rather than the promise.

[`api/tests/money.rs`](api/tests/money.rs) is the price of the first: a
generated type cannot drift from the document, and a hand-written one can, so a
test reads the pattern out of the embedded document and holds `Money::from_str`
to it value for value.

Where an adopter owns a type *above* the generated ones —
[`api/src/posting.rs`](api/src/posting.rs) — two clippy lints scoped to that
crate forbid both ways of writing a struct pattern that skips a field, so a
conversion out of a generated type cannot quietly ignore something the vendor
added.

## Two CLIs over the same API

[`cli/src/app.rs`](cli/src/app.rs) mounts the generated operations under `raw`
and puts a hand-written verb beside them: `finalize-voucher` fetches a voucher,
enshrines it if it is open, then renders it. The decision about *which* calls is
a pure function over the voucher — no client, no runtime, no fixture — and its
match has no `_` arm, so a status the vendor adds is a compile error where
someone has to decide whether it may be enshrined.

The verb is held to the same words the generated subcommand is. Its chain calls
the gated `enshrineVoucher`, so it adds its gate flags with `tree::gates` — the
same call the generated subcommand makes — and reads them back with
`tree::answers`. The word is the document's, not this crate's, and
`toy finalize-voucher` demands `--enshrine` exactly as `toy raw vouchers
enshrine` does.

[`cli/examples/root.rs`](cli/examples/root.rs) is the other shape: the
operations *are* the CLI, 53 lines, no `raw` layer and no dispatch of its own.
That is what an adoption looks like on day one.

[`cli/src/raw.rs`](cli/src/raw.rs) is worth reading if you are adopting. It is
the one thing the library cannot do for you — holding a CLI-built JSON body to
the generated type that operation takes — and it shows where the seam between
`tree::select` and `Selection::send` is for.

## What to copy

The Overlay layers, the five-crate split, and `xtask/src/main.rs`. That last one
is the whole of a bless step:

```rust
Settings::new(adoption.join("spec/toy.yaml"))
    .overlay(adoption.join("spec/corrections.yaml"))
    .overlay(adoption.join("spec/cli.yaml"))
    .replace("money", "money::Money")
    .write_to(adoption.join("api-generated"))
```

[overlay]: https://spec.openapis.org/overlay/v1.1.0.html
