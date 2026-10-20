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
$ just gate    # every check of CI's `just gate` job
```

`just gate` is the one command a change passes before it merges. It runs every
check of CI's `just gate` job: formatting, clippy and rustdoc with warnings
denied, every feature combination a consumer can select, the tests and the
doctests, a bless that has to reproduce every committed artefact, the crate
built from its own tarball, and the audit of the dependency tree.

`just bless` regenerates the example's generated files under
`examples/toy/api-generated/`. Nobody edits one by hand: a correction goes in an
Overlay under `examples/toy/spec/`, and `just bless` writes what follows from
it.

The toolchain is the one `rust-toolchain.toml` names, a dated nightly. Some
options in `rustfmt.toml`, such as `group_imports`, need a nightly rustfmt, and
so does the coverage recipe. The library itself builds and tests on stable Rust.
CI's `stable` job holds it to that, and it is the one CI check outside
`just gate`.

Only `rustup` reads `rust-toolchain.toml`, so on a host without it, check the
compiler before trusting a green gate. `rustc --version` prints the commit date
of the compiler, which is one day before the date in the file.

The gate needs `just` and `cargo-nextest` installed, and `just audit` needs
`cargo-deny`. `.github/workflows/ci.yml` pins the versions of the tools CI
installs.

## Dependencies

Each direct dependency has a one-line reason above it in the `Cargo.toml` that
declares it. This holds for every member of the workspace, the example's crates
included.

`just audit` is the gate for a dependency, and `just gate` runs it. It checks
the dependency tree, every feature on, for advisories, unmaintained crates and
sources other than crates.io, and it warns on two versions of one crate. It
reads the advisory database over the network, so it fails offline.

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
