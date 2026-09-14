# Working on typed-openapi

`typed-openapi/` is the published crate. `examples/toy/` is one worked adoption
of it, four crates, all `publish = false`, built and tested with the rest so
that the example cannot rot.

Run `just gate` before saying anything is done. It is `check` (fmt, compile,
clippy, rustdoc — each with warnings denied), `features` (every feature
combination a consumer can select, plus the clap-free assertion), `test`
(nextest and the doctests, which nextest cannot run), `blessed` (the generator
still reproduces every committed artefact) and `package` (the crate built from
its own tarball). CI runs the same recipes, not a copy of them.

## Invariants a change must keep true

**The gate is default-closed and derived from the document.** `Plan::decide` in
`src/plan.rs` is the only place that decides whether a request is sent. A write
without confirmation is a `DryRun` carrying the exact request a confirmed run
would send — the same value, so the two cannot drift. Whether an operation
writes comes from the HTTP method plus the document's `x-cli-writes` marker,
never from a name or a heuristic.

**Both command names are decided while the document is reduced.** `Grouping` in
`src/names.rs` reads the rule off every path once; an `Operation` carries the
group and the command it was placed under, and both travel in the postcard blob.
A shipped binary reads them and derives nothing. Two operations reducing to one
`<group> <command>` is a `LoadError` naming both `operationId`s — never a silent
rename, because a name that moves when a *second* operation arrives is a name
that moved without anyone asking. `x-cli-group` and `x-cli-command` are the way
out, and they are read in the same place the rule runs.

**Corrections are an ordered list of Overlays, and the library reads nothing
into it.** `Settings::overlay` and `Document::load` take layers and apply them
in order. What an adoption puts in which layer is a convention `docs/overlay.md`
recommends and the example keeps — there is no enum, no schema and no naming
rule in the library. `examples/toy/api/tests/corrections.rs` is what holds the
example to its own convention.

**Every rule the document states about a value is enforced, in one place.**
`Scalar` carries what the schema said — `pattern`, `minLength`/`maxLength`, the
bounds, `multipleOf` — and `Scalar::parse` is the only thing that checks any of
them. A rule that reaches `--help` and not the parser is the defect this
invariant exists to prevent, so `Scalar::note` and the refusal are one
rendering. `pattern` runs on `regress`, the engine typify puts inside a
generated newtype's `FromStr`, so a value the command line accepts is a value
the generated type accepts by construction. No `format` is special-cased: a
format names a rule and is not one, and a document that wants a rule states it
in JSON Schema everything can read.

**One request builder serves both consumers.** A CLI reaches it through `tree`,
a generated wrapper through `Values` directly. Anything that makes the command
line and the typed call disagree about an operation is a bug, not a feature.

**A feature adds and removes whole items; it never changes one.** No type may
gain a variant or a field with a feature. This is why `LoadError` is separate
from `DocumentError` and why `Plan` lives in `plan` rather than in the
clap-gated `tree`. A caller who matches an error exhaustively must write the
same match in every build.

**The crate depends on no HTTP client, in any feature combination.** The seam is
`SyncClient` / `AsyncClient` over `http::Request<Vec<u8>>`. Adapters live in
`examples/toy/cli/src/client.rs` to be copied, not depended on. An adapter must
not turn a status code into an error: the body that came with a 4xx is what a
caller needs.

**The typed half of an adoption links no argument parser.** `api` and
`api-generated` take the library with `default-features = false`.
`just clap-free` is the check.

**Generated files are never edited.** `examples/toy/api-generated/src/{types,ops}.rs`,
`src/model.postcard` and `spec/toy.overlaid.yaml` are written by `just bless`.
A correction belongs in `examples/toy/spec/corrections.yaml` or, where only a
command line cares, `examples/toy/spec/cli.yaml`. `just bless` must
leave `git diff` empty when nothing upstream has changed — a non-empty diff
means either the vendor moved or the generator did.

**The library never reads the example's files.** `typed-openapi/tests/fixtures/`
holds this crate's own copies of the toy document and both Overlay layers. They
have the same content as `examples/toy/spec/` and a different owner: there the
document is the vendor's and the example is free to evolve it. Pointing the
library's tests at that copy would let a change to the example break the
library, and the fixtures would stop shipping in the published tarball.

## Style

The lint block in the root `Cargo.toml` is from the first commit so that no
code is written against a looser setting and grandfathered in. `unsafe_code` is
**forbidden**, not denied. `clippy.toml` tunes the size lints; a hit is a
cleanup item, and `#[expect]` needs a reason a reviewer would accept.

Doc comments are prose that says *why*, in present tense, describing the system
as it is. Never "now", "no longer", "previously", "instead of" — a reader
arriving today never saw the earlier version. The diff belongs in the commit
message.

Route a document by who reads it: `typed-openapi/README.md` is the primer for
someone deciding whether to use the crate, `docs/*.md` is reference for someone
looking one thing up, this file is for an agent changing the code.

## Toolchain

`rust-toolchain.toml` pins nightly because four of `rustfmt.toml`'s options are
nightly-only and the coverage recipe wants branch instrumentation. **The library
itself is stable** — CI's `stable` job builds and tests it there, and nothing in
`typed-openapi/src/` may need a nightly feature.

## Publishing

A tag `v<version>` on a commit CI has passed triggers `.github/workflows/release.yml`,
which checks the tag against the manifest and publishes through crates.io
Trusted Publishing. The first version has to be published by hand before Trusted
Publishing can be configured for the crate.
