# Toolchain and publishing

## Toolchain

`rust-toolchain.toml` pins a dated nightly; its comments give the reason and the
upgrade path. CI's `just gate` job installs what the file names and passes no
`toolchain:` of its own, so moving the date is one edit to one file. A date that
turns the gate red is repaired in the pull request that moves it.

Only rustup's proxies choose the compiler from the file: a shell without them
runs the `cargo` on its `PATH`. `rustc --version` prints the compiler's commit
date, which is one day before the channel date the file names, so compare it
with that date minus one before trusting a green gate.

## Publishing

A version bump is followed by `just bless`. The stamp of the model blob holds
the version of typed-openapi, so `examples/toy/api-generated/src/model.postcard`
changes with each version, and `blessed` fails on a bump that did not write it
again. The version is in `[workspace.package]` of the root `Cargo.toml`.

A push to `main` that changes the version to one nobody has tagged releases,
and `.github/workflows/release.yml` does all of it. Its `version` job reads the
root manifest at the event's commit and its `before` commit. Unavailable
comparison history or an unreadable before-version fails the job. An unchanged
version or a dispatch answers `rehearse`; no later job looks at the event
itself. `publish` waits for `gate` and `rehearse`, holds `id-token`
and publishes through crates.io Trusted Publishing; `rehearse` has built the
package outside the environment, so `publish` uploads with `--no-verify` and
compiles no dependency beside the two tokens. `announce` then makes the tag and
the GitHub Release, whose body is the version's section of `CHANGELOG.md`. A
release without that section fails in `version`, before anything is written.

There is no tag trigger, so a tag pushed by hand publishes nothing, and a
dispatch from any branch is a dry run: every job that reads runs, none that
writes does. Each writing step looks before it writes, so a release that
stopped half-way finishes when its failed jobs are run again with "Re-run
failed jobs", at the original event and commit. A later push with the same
version rehearses even if the failed run has not made its tag. "Re-run all
jobs" is no substitute: after a run that made the tag and failed at the
Release, `version` finds the tag and answers `rehearse`.
`workflow_dispatch` stays declared in the file on `main`, because GitHub
accepts a dispatch of a branch's copy only then. The group of `release.yml`
sets `queue: max`, because the default queue cancels a waiting release when a
third run joins it.

`rehearse` and `publish` run `cargo +stable`, because `rust-toolchain.toml` pins
a nightly and the library is a stable crate, and they set no `RUSTFLAGS`,
because the gate is where warnings are denied. The gate's `package` commands
also run on stable; its tests of the unpacked tarball use the pinned nightly.
The `rust stable` job of `ci.yml` builds and tests the library on stable, and
sets `RUSTUP_TOOLCHAIN=stable` so its cache probes that compiler too. Both
workflows read with `contents: read` at the top and a job widens only what it
needs. Every `uses:` is pinned by commit SHA with its version in a comment, and
Dependabot proposes the next SHA. The tools `install-action` fetches are pinned
by version in both files, and move by hand.

The first version has to be published by hand before Trusted Publishing can be
configured for the crate.
