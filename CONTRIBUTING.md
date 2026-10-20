# Contributing

## Outside issues and pull requests

For now the project takes no outside issues and no outside pull requests. The
work is tracked outside GitHub, so an issue or a pull request opened here has
no place in it.

A vulnerability is not an issue to open: it goes through the private channel
[SECURITY.md](SECURITY.md) describes.

## Building and checking a change

These steps are for the project's own sessions.

```console
$ just test    # nextest, then the doctests nextest cannot run
$ just bless   # regenerate every artefact the example commits
$ just gate    # everything CI runs
```

`just gate` is the one command a change passes before it merges. It runs every
check CI runs: formatting, clippy and rustdoc with warnings denied, every
feature combination a consumer can select, the tests and the doctests, a bless
that has to reproduce every committed artefact, the crate built from its own
tarball, and the audit of the dependency tree.

`just bless` regenerates the example's generated files under
`examples/toy/api-generated/`. Nobody edits one by hand: a correction goes in an
Overlay under `examples/toy/spec/`, and `just bless` writes what follows from
it.

The toolchain is the one `rust-toolchain.toml` names, a dated nightly. The
options in `rustfmt.toml` need a nightly rustfmt, and so does the coverage
recipe. The library itself builds and tests on stable Rust, and CI's `stable`
job holds it to that.

Only `rustup` reads `rust-toolchain.toml`: a shell without it runs whichever
`cargo` is on its `PATH`, so compare `rustc --version` with the date in the file
before trusting a green gate. The gate needs `just` and `cargo-nextest`
installed, and `just audit` needs `cargo-deny`; `.github/workflows/ci.yml` pins
the versions CI installs.

## Dependencies

Each direct dependency has a one-line reason above it in the `Cargo.toml` that
declares it. This holds for every member of the workspace, the example's crates
included.

`just audit` is the gate for a dependency, and `just gate` runs it. It checks
the dependency tree, every feature on, for advisories, unmaintained crates and
sources other than crates.io. It reads the advisory database over the network,
so it fails offline.

## `unsafe`

`unsafe` is forbidden in every crate. The workspace sets `unsafe_code` to
`"forbid"`, every member inherits it with `[lints] workspace = true`, and an
inner `#[allow]` cannot lower a `forbid`.

## The changelog

A change an adopter can see adds its line to `CHANGELOG.md`, under the
`## Unreleased` heading, with the repair it asks for. Three kinds of change are
always seen: a break of the public API, a change to the generated code or to the
committed blob, and a document that an earlier release loaded and this one
refuses.
