# Working on typed-openapi

`typed-openapi/` is the published crate. `examples/toy/` is one worked adoption
of it, all `publish = false`, built and tested with the rest so that the
example cannot rot.

Run `just gate` before saying anything is done. CI's gate job runs the same
recipes; its separate stable job builds and tests the library with all
features.

## Where the rest is

- Library: before changing `typed-openapi/src/`, read `docs/agents/library-modules.md`.
- Toy: before changing `examples/toy/`, read `docs/agents/toy-example.md`.
- Tests: before adding a test or fixture, or touching target-directory handling, read `docs/agents/testing.md`.
- Release: before changing the version, a workflow or the toolchain pin, read `docs/agents/releasing.md`.

**A change under one of those paths is finished only when its document is
reconciled**: cut what the change made false, add what it made non-obvious. A
rule goes once, in the nearest document above every path that keeps it.

Route a document by who reads it: `typed-openapi/README.md` is the primer for
someone deciding whether to use the crate, `docs/*.md` is reference for someone
looking one thing up, and this file with `docs/agents/` is for an agent changing
the code.

## Invariants a change must keep true

**A feature adds and removes whole items; it never changes one.** No type may
gain a variant or a field with a feature. This is why `LoadError` is separate
from `DocumentError` and why `Plan` lives in `plan` rather than in the
clap-gated `tree`. A caller's match on an error is the same in every build, `_`
arm included. The error enums are `#[non_exhaustive]` except `ScalarError`,
`RefError` and `ConfirmationError`, so a new refusal in any other is a new
variant and not a break.

**The crate depends on no HTTP client, in any feature combination.** The seam is
`SyncClient` / `AsyncClient` over `http::Request<Vec<u8>>`. Adapters live in
`examples/toy/cli/src/client.rs` to be copied, not depended on. An adapter must
not turn a status code into an error: the body that came with a 4xx is what a
caller needs. An adapter must not follow redirects: the original 3xx
status, headers and body belong to the caller.

**What an owned type is made of never reaches the library.** `num-bigint` is
declared in `examples/toy/money` and in no other manifest, and `just
bigint-free` asserts it is absent from `cargo tree -p typed-openapi
--all-features`. `Settings::replace` exists precisely so that the crate does not
have to know what a replaced type is built from.

**The library never reads the example's files.** `typed-openapi/tests/fixtures/`
holds this crate's own copies of the toy document and both Overlay layers. They
have the same content as `examples/toy/spec/` and a different owner: there the
document is the vendor's and the example is free to evolve it. Pointing the
library's tests at that copy would let a change to the example break the
library, and the fixtures would stop shipping in the published tarball.

**The library itself is stable.** Nothing in `typed-openapi/src/` may need a
nightly feature; the repository's own tooling is nightly.

## Style

The lint block in the root `Cargo.toml` is from the first commit so that no
code is written against a looser setting and grandfathered in. `clippy.toml`
tunes the size lints; a hit is a cleanup item, and `#[expect]` needs a reason a
reviewer would accept.

Doc comments say *why* and describe the system as it is. They carry no
history: a reader arriving today never saw the earlier version. A comparison
of present choices can use "instead of". The diff belongs in the commit message.

Name the item, do not count it. Replace a count, a copied value or a source
line number with the item or the property it stands for. A figure that a reader
needs stays only when a test holds it to the system, as the toy's summary test
holds its README counts.

**An error says its own layer once.** An `#[error]` string describes the layer
that owns it and never prints a source it also marks `#[source]` or `#[from]`.
Whoever reports the error walks `source()`, as the toy's `main` and `xtask` do,
so the cause is printed once, on its own line. `typed-openapi/tests/errors.rs`
walks one chain through every variant that carries a source: a new refusal that
has one adds its case there.

A public variant that changes needs a CHANGELOG line for every trait impl it
takes with it (`#[from]` gives a `From`).

## Running the gate

`just gate` stops at `blessed` while a bless output is uncommitted: commit the
bless, then gate the commit. Run `just fmt` first, because a fmt diff stops the
gate at its first step. Edit nothing in the worktree while a gate runs: each
recipe reads the files as they are when it reaches them, so after an edit the
result is the result of no commit. `audit` reads the advisory database over the
network and fails offline.

Clippy reaches test code, and a `just check` that ran before the last edit does
not see what that edit adds. Run `cargo clippy --workspace --all-features
--all-targets -- -D warnings` after the last edit and before every commit and
amend. Four fixes are not plain from the message: a case table that trips
`too_many_lines` goes into a module `const` or a `fn` that returns it;
`unwrap_used` and `expect_used` reach every target, a test module and a file of
`tests/` included, and a test answers them with `let … else { panic!(..) }` or
with `#[expect(clippy::expect_used, reason = ..)]` (the `unwrap_used` form is the
same) on the module or, as `#![expect(..)]`, on the file; a long string is built
with `write!(s, ..)` after `use std::fmt::Write as _`, because
`format_push_string` is denied; and `string_slice` and `indexing_slicing` are
answered by `split_once`, `strip_prefix` and `<[T; N]>::try_from`.

The `warning[duplicate]` lines of `cargo-deny` in a green gate are bans and not
compiler warnings: grep the log for `^warning:` and `^error`.
