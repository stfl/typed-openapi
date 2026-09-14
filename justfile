# Workspace tasks. Every cargo invocation carries `--workspace` so that a crate
# added later is covered without editing a recipe.

default:
    just --list

# `rustfmt.toml` sets `unstable_features = true`, so this needs a nightly
# toolchain; `rust-toolchain.toml` pins one.

# Format the workspace.
fmt:
    cargo fmt --all

# `-D warnings` is what makes the `warn` levels in `[lints]` a gate
# rather than a suggestion; clippy exits zero on all of them without it, and
# `RUSTDOCFLAGS` is the same promotion for the `[lints.rustdoc]`
# block. Building the docs is the only thing that runs rustdoc's lints: a
# broken intra-doc link warns during a doctest run and fails here.

# Verify formatting, compile, then lint code and docs with warnings denied.
check:
    cargo fmt --all --check
    cargo check --workspace --all-targets
    cargo clippy --workspace --all-targets -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# A feature that adds or removes items has to be compiled in every combination
# a consumer can ask for, because nothing else finds an item that only exists
# in one of them. These are the combinations: the default, nothing, each
# feature alone, and everything at once.

# Compile the library under every feature combination a consumer can select.
features:
    cargo check -p typed-openapi
    cargo check -p typed-openapi --no-default-features
    cargo check -p typed-openapi --no-default-features --features clap
    cargo check -p typed-openapi --no-default-features --features document
    cargo check -p typed-openapi --no-default-features --features generate
    cargo check -p typed-openapi --no-default-features --features builder
    cargo check -p typed-openapi --all-features --all-targets
    RUSTDOCFLAGS="-D warnings" cargo doc -p typed-openapi --no-deps --no-default-features
    cargo check -p cli --features reqwest-client
    cargo check -p api-generated --features builder
    just clap-free

# The typed half of an adoption must link no argument parser: `api` takes the
# library with `default-features = false`, so a clap in its tree is a
# regression and not a preference.

# Prove the typed half of the example links no argument parser.
clap-free:
    #!/usr/bin/env bash
    set -euo pipefail
    if cargo tree -p api -e normal --prefix none | grep -q '^clap'; then
        echo "clap reached api's normal dependency tree:" >&2
        cargo tree -p api -e normal | grep -i clap >&2
        exit 1
    fi
    echo "api links no argument parser"

# Two runners because the first cannot do the second's job: nextest gives every
# test its own process and has no doctest support at all, so an example in a
# doc comment is compiled and executed only by the `--doc` run beside it.

# Run the test suites and the doctests.
test:
    cargo nextest run --workspace
    cargo test --workspace --doc

# Everything CI runs, in the order CI runs it.
gate: check features test package

# What crates.io will receive. `--list` is the cheap half — it fails on missing
# metadata and prints the file set — and the build proves the crate stands up
# outside this workspace.

# Verify the published crate packages and builds from its own tarball.
package:
    cargo package -p typed-openapi --list
    cargo package -p typed-openapi --allow-dirty

# Regenerate the example adoption from the vendor document and the Overlay.
# Everything it writes is committed, so a diff after this recipe is the answer
# to "what did the vendor change?".
bless:
    cargo run -p xtask -- bless

# One instrumented run, three renderings of it: `--no-report` leaves the raw
# profile behind so the cobertura file, the browsable HTML and the table all
# describe the same execution. Everything lands under `target/`, which is
# already ignored. Doctests are outside the measurement — the test runner does
# not carry them.
#
# `--branch` adds the branch columns. It rests on rustc's
# `-Z coverage-options=branch`, so it is nightly-only and prints an unstable
# warning; the toolchain is already nightly for `rustfmt`. LLVM counts a branch
# where control flow forks on a condition — `if`, `&&`, `||`, `while let` — and
# a `match` arm is a *region*, not a branch, so code that decides by matching
# reports few branches and a high region count rather than a gap in the tests.

# Measure coverage, branches included, and print the per-file table.
cov:
    cargo llvm-cov clean --workspace
    mkdir -p target/llvm-cov
    cargo llvm-cov nextest --workspace --branch --no-report
    cargo llvm-cov report --branch --cobertura --output-path target/llvm-cov/coverage.cobertura.xml
    cargo llvm-cov report --branch --html
    cargo llvm-cov report --branch
