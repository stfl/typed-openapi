# Working on typed-openapi

`typed-openapi/` is the published crate. `examples/toy/` is one worked adoption
of it, five crates, all `publish = false`, built and tested with the rest so
that the example cannot rot.

Run `just gate` before saying anything is done. It is `check` (fmt, compile,
clippy, rustdoc — each with warnings denied), `features` (each feature alone
and all together, each with warnings denied, plus the bon-free, clap-free and
bigint-free assertions),
`test` (nextest and the doctests, which nextest cannot run), `blessed` (the
generator still reproduces every committed artefact, and no file in
`api-generated` is untracked), `package` (the crate built and tested from its
own tarball) and `audit` (no crate in the tree has an advisory against it, and
none is ignored; it reads the advisory database over the network, so it fails
offline). CI runs the same recipes, not a copy of them.

## Directory notes

A directory with its own `CLAUDE.md` loads it when you read or edit a file
there with the file tools; a shell `cat` loads nothing. These carry one:
`typed-openapi/src/` (the library's modules, `generate/` and `model/` under it)
and `examples/toy/` (its five crates and `spec/`).

**A change to code in a directory with a note is finished only when that
note is reconciled**: cut what the change made false, add what it made
non-obvious. A rule true of a whole tree goes once, in the highest note it
holds for. A rule that two directories keep stays in this file.

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
caller needs.

**What an owned type is made of never reaches the library.** `num-bigint` is
declared in `examples/toy/money` and in no other manifest, and `just
bigint-free` — inside `features`, beside `bon-free` and `clap-free` — asserts it
is absent from `cargo tree -p typed-openapi --all-features`. `Settings::replace`
exists precisely so that the crate does not have to know what a replaced type
is built from.

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

**An error says its own layer once.** An `#[error]` string describes the layer
that owns it and never prints a source it also marks `#[source]` or `#[from]`.
Whoever reports the error walks `source()`, as the toy's `main` and `xtask` do,
so the cause is printed once, on its own line. `typed-openapi/tests/errors.rs`
walks one chain through every variant that carries a source: a new refusal that
has one adds its case there.

Route a document by who reads it: `typed-openapi/README.md` is the primer for
someone deciding whether to use the crate, `docs/*.md` is reference for someone
looking one thing up, this file and the directory notes it lists are for an
agent changing the code.

## Running the gate and the tests

`just gate` stops at `blessed` while a bless output is uncommitted: commit the
bless, then gate the commit. Run `just fmt` first, because a fmt diff stops the
gate at its first step. Edit nothing in the worktree while a gate runs: its
`package` recipe sees a dirty tree and the run is void.

Clippy reaches test code, and a `just check` that ran before the last edit does
not see what that edit adds. Run `cargo clippy --workspace --all-features
--all-targets -- -D warnings` after the last edit and before every commit and
amend; the gate's `check` also runs `cargo doc` with warnings denied. The
thresholds are in `clippy.toml` and the denied lints in the lint block of
`Cargo.toml`. Four fixes are not plain from the message: a case table that trips
`too_many_lines` goes into a module `const` or a `fn` that returns it; a `mod
tests` in `src/` denies `unwrap_used` and `expect_used`, so it writes `let … else
{ panic!(..) }`; a long string is built with `write!(s, ..)` after `use
std::fmt::Write as _`, because `format_push_string` is denied; and
`string_slice` and `indexing_slicing` are answered by `split_once`,
`strip_prefix` and `<[T; N]>::try_from`.

The `warning[duplicate]` lines of `cargo-deny` in a green gate are bans and not
compiler warnings: grep the log for `^warning:` and `^error`.

A public variant that changes needs a CHANGELOG line for every trait impl it
takes with it (`#[from]` gives a `From`).

`rustfmt.toml` sets `format_code_in_doc_comments = true`. A test directory under
`target/tmp` inherits it, because the nearest `rustfmt.toml` wins and nothing
merges, and the tarball that the `package` recipe tests has no `rustfmt.toml`: a
test that needs a setting writes its own.

A test is a process of its own under nextest and a thread of a shared one under
`cargo test`, so each test that calls `out(name)` takes a directory name of its
own, and no `LazyLock` is shared by both. A new `[[test]]` target with
`required-features` runs in the `package` recipe at once.
`typed-openapi/tests/fixtures/shapes.json` is the library's own fixture for
`fits` and a copy of no toy file.

## Toolchain

`rust-toolchain.toml` pins a dated nightly because four of `rustfmt.toml`'s
options are nightly-only and the coverage recipe wants branch instrumentation.
Each move takes the newest date on which `just gate` passes. CI's `just gate`
job installs what the file names and passes no `toolchain:` of its own, so
moving the date is one edit to one file; Dependabot reads the file too and
proposes the next date weekly, and a date that turns the gate red is repaired in
the pull request that moves it. Only rustup's proxies choose the compiler from
the file: a shell without them runs the `cargo` on its `PATH`. `rustc --version`
prints the compiler's commit date, which is one day before the channel date the
file names, so compare it with that date minus one before trusting a green gate.

**The library itself is stable** — CI's `stable` job builds and tests it there,
and nothing in `typed-openapi/src/` may need a nightly feature.

## Publishing

A version bump is followed by `just bless`. The stamp of the model blob holds
the version of typed-openapi, so `examples/toy/api-generated/src/model.postcard`
changes with each version, and `blessed` fails on a bump that did not write it
again. The version is in `[workspace.package]` of the root `Cargo.toml`, a file
that no directory note covers.

A merge to `main` that carries a version nobody has tagged is the one act that
releases, and `.github/workflows/release.yml` does all of it. Its `version` job
reads the manifest and answers `release` or `rehearse`; no later job looks at
the event itself. `publish` waits for `gate` and `rehearse`, holds `id-token`
and publishes through crates.io Trusted Publishing; `rehearse` has built the
package outside the environment, so `publish` uploads with `--no-verify` and
compiles no dependency beside the two tokens. `announce` then makes the tag and
the GitHub Release, whose body is the version's section of `CHANGELOG.md`. A
release without that section fails in `version`, before anything is written.

There is no tag trigger, so a tag pushed by hand publishes nothing, and a
dispatch from any branch is a dry run: every job that reads runs, none that
writes does. Each writing step looks before it writes, so a release that
stopped half-way finishes when its failed jobs are run again with "Re-run
failed jobs". "Re-run all jobs" is no substitute: after a run that made the tag
and failed at the Release, `version` finds the tag and answers `rehearse`.
`workflow_dispatch` stays declared in the file on `main`, because GitHub
accepts a dispatch of a branch's copy only then. The group of `release.yml`
sets `queue: max`, because the default queue cancels a waiting release when a
third run joins it.

`rehearse` and `publish` run `cargo +stable`, because `rust-toolchain.toml` pins
a nightly and the library is a stable crate, and they set no `RUSTFLAGS`,
because the gate is where warnings are denied. The gate's `package` recipe
builds the tarball on the pinned nightly, and the `rust stable` job of `ci.yml`
is what says the library compiles on stable. Both workflows read with
`contents: read` at the top and a job widens only what it needs. Every `uses:`
is pinned by commit SHA with its version in a comment, and Dependabot proposes
the next SHA. The tools `install-action` fetches are pinned by version in both
files, and move by hand.

The first version has to be published by hand before Trusted Publishing can be
configured for the crate.
