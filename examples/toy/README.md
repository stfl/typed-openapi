# The toy adoption

One adoption of [`typed-openapi`](../../typed-openapi) from end to end: a vendor
document that is wrong in five ways, an Overlay that corrects it, a bless step,
and two CLIs over the result. It is built, linted and tested with the library on
every push, so it cannot rot into an example that no longer compiles.

The Toy Accounting API is invented, and nothing here reaches the network: the
default server is `http://localhost:9999`, every write is a dry run until
`--commit`, and the tests drive the real commands through a recording client
with no socket.

## Try it

```sh
cargo run -p cli -- --help                      # raw, finalize-voucher, completions
cargo run -p cli -- raw --help                  # one subcommand per operation
cargo run -p cli -- raw get-voucher --help      # flags from the document
cargo run -p cli --example root -- --help       # the same operations as the whole CLI
just bless                                      # regenerate; the diff must be empty
```

A write prints what it would send and stops:

```console
$ cargo run -p cli -- raw render-voucher --id 5
GET /vouchers/5/render HTTP/1.1
host: localhost:9999
dry run: nothing was sent. Add --commit to send it.
```

The request is on stdout and the line explaining it is on stderr, so a script
that pipes the first gets the request and nothing else.

That one is a `GET`. HTTP cannot say "this GET writes", so the Overlay does,
with `x-cli-writes` — and the gate treats it like any `POST`.

## What the vendor gets wrong, and where it is fixed

Every correction is a standard [OpenAPI Overlay][overlay] action in
[`spec/overlay.yaml`](spec/overlay.yaml). [`spec/toy.yaml`](spec/toy.yaml) — the
vendor's document — is never edited.

| the vendor | the correction | what it buys |
|---|---|---|
| declares `format: money` and never says what an amount is | an Overlay `update` adding the pattern | `Voucher.total` is a `Money`, and `--total` rejects `1,50` |
| returns an `internal_ref` it never documented | an `update` adding the property | a struct field and a `--internal-ref` flag |
| ships `archiveVoucher` and documents it nowhere | an `update` adding the path | a wrapper and a subcommand, for no Rust at all |
| serves a `GET` that stores a PDF | `x-cli-writes: true` | `render-voucher` is behind `--commit` |
| misspells a multipart media type | left alone, and both uploads work | `--raw-body` for one, `--file` / `--field` for the other |

Two of those actions are **tripwires**: their JSONPath states what the vendor
currently says, so under `ErrorOnZeroMatch` a vendor revision that moves the
thing being corrected fails the bless step instead of being silently
overwritten. [`../../docs/overlay.md`](../../docs/overlay.md) explains the form;
[`../../docs/drift.md`](../../docs/drift.md) is the whole table of what is
caught where.

[`api/src/corrections.rs`](api/src/corrections.rs) lists the same five as Rust,
one line each, and `api/tests/corrections.rs` holds that list to both documents
in both directions — a row that no longer describes a real difference fails, and
a real difference with no row fails.

## Why five crates

| crate | lines | written by | holds |
|---|---|---|---|
| [`api-types`](api-types) | 93 | the adopter | `Money`. Generated code names it, so it sits below the generated code |
| [`api-generated`](api-generated) | 529 | `just bless`, except `client.rs` | the corrected document, the Rust types, one wrapper per operation, the reduced model |
| [`api`](api) | 843 | the adopter | the crate an adopter's own code names: corrections, `Posting`, and everything re-exported |
| [`cli`](cli) | 1320 | the adopter | the `toy` binary, and `examples/root.rs` beside it |
| [`xtask`](xtask) | 46 | the adopter | the bless step — the generator itself ships in `typed-openapi` |

The split is about what recompiles. An edit to `api` rebuilds the adopter's own
lines and not the generated volume beneath them, and `api-generated`'s whole
dependency list is `serde`, `http`, `api-types` and the library — nothing about
a generator reaches it.

There is no hand-written mirror of the generated types. The generated
`Voucher.total` *is* `Money`, substituted in by the bless step, with no
conversion at the boundary. Where an adopter does own a type by hand —
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

[`cli/examples/root.rs`](cli/examples/root.rs) is the other shape: the
operations *are* the CLI, 53 lines, no `raw` layer and no dispatch of its own.
That is what an adoption looks like on day one.

[`cli/src/raw.rs`](cli/src/raw.rs) is worth reading if you are adopting. It is
the one thing the library cannot do for you — holding a CLI-built JSON body to
the generated type that operation takes — and it shows where the seam between
`tree::select` and `Selection::send` is for.

## What to copy

The Overlay, the five-crate split, and `xtask/src/main.rs`. That last one is the
whole of a bless step:

```rust
Settings::new(adoption.join("spec/toy.yaml"), adoption.join("spec/overlay.yaml"))
    .replace("money", "api_types::Money")
    .write_to(adoption.join("api-generated"))
```

[overlay]: https://spec.openapis.org/overlay/v1.1.0.html
