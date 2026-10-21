# Workspace tasks. Every cargo invocation carries `--workspace` so that a crate
# added later is covered without editing a recipe.

default:
    just --list

# `rustfmt.toml` sets `unstable_features = true`, so this needs a nightly
# toolchain; `rust-toolchain.toml` pins a dated one.

# Format the workspace.
fmt:
    cargo fmt --all

# `-D warnings` is what makes the `warn` levels in `[lints]` a gate
# rather than a suggestion; clippy exits zero on all of them without it, and
# `RUSTDOCFLAGS` is the same promotion for the `[lints.rustdoc]`
# block. Building the docs is the only thing that runs rustdoc's lints: a
# broken intra-doc link warns during a doctest run and fails here.
#
# `--cfg docsrs` is what docs.rs passes (`[package.metadata.docs.rs]`), and
# `typed-openapi/src/lib.rs` turns it into `feature(doc_cfg)`, which prints the
# "Available on crate feature" label on every item a feature adds. The one doc
# build carries it, so that the path docs.rs takes is in the gate: a nightly
# that renames or drops the feature fails here and not on docs.rs.
#
# `--all-features` compiles, lints and documents what only a named feature
# builds: the `builder` wrappers and the reqwest adapter of the example. A
# target the gate builds without linting is one a lint never reaches.

# Verify formatting, compile, then lint code and docs with warnings denied.
check:
    cargo fmt --all --check
    cargo check --workspace --all-targets --all-features
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo doc --workspace --no-deps --all-features

# A feature that adds or removes items has to be compiled with it and without
# it, because nothing else finds an item that only exists in one of them. These
# are the builds: the default, nothing, each feature alone, and all together. No
# pair of features is built. The `builder` runs go further than a compile: the
# example's demonstration of it is a pair of doctests that only exist when the
# feature is on, so this is the only recipe that can run them.
#
# Each build is linted with warnings denied, as `check` lints the widest one. A
# warning that only one feature set produces, such as an item that nothing calls
# without a feature, is absent from the widest build, and a plain `cargo check`
# prints it and exits zero. `clippy` carries `-D warnings` to this workspace's
# crates only; `RUSTFLAGS` would carry it to every dependency and build each
# again.

# Lint the library with each feature alone and with all of them together.
features:
    cargo clippy -p typed-openapi -- -D warnings
    cargo clippy -p typed-openapi --no-default-features -- -D warnings
    cargo clippy -p typed-openapi --no-default-features --features clap -- -D warnings
    cargo clippy -p typed-openapi --no-default-features --features document -- -D warnings
    cargo clippy -p typed-openapi --no-default-features --features generate -- -D warnings
    cargo clippy -p typed-openapi --no-default-features --features builder -- -D warnings
    cargo clippy -p typed-openapi --all-features --all-targets -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc -p typed-openapi --no-deps --no-default-features
    cargo clippy -p cli --features reqwest-client -- -D warnings
    cargo clippy -p api-generated --features builder -- -D warnings
    cargo test -p api-generated --features builder --doc
    # The line that catches a builder which replaces a wrapper instead of
    # adding one: `api-generated` never calls its own wrappers, so only a
    # crate that does can tell the difference.
    cargo clippy -p cli --features builder --all-targets -- -D warnings
    just bon-free
    just clap-free
    just bigint-free

# `bon` is what the `builder` feature costs, so it must be absent without it.
# `cargo tree -e normal` is the build a consumer gets; a proc-macro crate that
# only a dev-dependency pulls in would not show here, and should not.

# Prove `bon` reaches the generated crate only with the builder feature on.
bon-free:
    #!/usr/bin/env bash
    set -euo pipefail
    if cargo tree -p api-generated -e normal --prefix none | grep -q '^bon'; then
        echo "bon reached api-generated without the builder feature:" >&2
        cargo tree -p api-generated -e normal | grep -i bon >&2
        exit 1
    fi
    cargo tree -p api-generated -e normal --features builder --prefix none \
        | grep -q '^bon' || {
        echo "bon is absent even with the builder feature on" >&2
        exit 1
    }
    echo "bon reaches api-generated only with the builder feature"

# `Settings::replace` exists so that an adopter can own a type the document
# cannot describe — which means the library must not know what such a type is
# made of. The example's `Money` counts cents in a `num-bigint` integer; that
# choice belongs to `examples/toy/money` and must not reach the crate that is
# published. `--all-features` is the widest build there is, so absence there is
# absence in every combination.

# Prove the published crate links no bignum, in any feature set.
bigint-free:
    #!/usr/bin/env bash
    set -euo pipefail
    if cargo tree -p typed-openapi --all-features -e normal --prefix none | grep -q '^num-bigint'; then
        echo "num-bigint reached typed-openapi's normal dependency tree:" >&2
        cargo tree -p typed-openapi --all-features -e normal | grep -i num-bigint >&2
        exit 1
    fi
    # The same detector, pointed at the one crate that should trip it.
    cargo tree -p money -e normal --prefix none | grep -q '^num-bigint' || {
        echo "num-bigint is absent from the crate that owns the type" >&2
        exit 1
    }
    echo "num-bigint reaches the example's own type and nothing else"

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

# `cargo deny` reads the advisory database over the network, and `check` runs
# between edits: a check that fails on a fetch timeout is a check people stop
# running. So `audit` is apart from `check` because of the network, and it is
# in `gate` so that an advisory filed against a crate in the tree turns the
# gate red in CI. `deny.toml` says why the licence check is left out.

# Check the dependency tree for advisories, bans and unknown sources.
audit:
    cargo deny check advisories bans sources

# Everything CI runs, in the order CI runs it.
gate: check features test blessed package audit

# Every artefact under `api-generated` is committed, so the generator has to be
# able to reproduce them. A diff here is either the vendor's document moving or
# the generator's output changing — both worth looking at, and neither should
# reach main unnoticed. Without this recipe nothing in the gate runs the
# generator at all.
#
# `src/summary.md` is in the list for the same reason as the rest and for two
# more. It is the artefact adopters quote counts out of, so an unchecked copy of
# it would be exactly the stale number it exists to retire — and it is the one
# the bless step writes only because `xtask` asks for it, which is a line that
# can be deleted. So the recipe checks two things a diff alone cannot tell
# apart: that the generator still writes each artefact, and that what it wrote
# is what is committed. A file nobody generates any more keeps its bytes and
# passes the diff.
#
# `git diff` does not list a file that is not tracked, so an artefact the
# generator writes and nobody committed would pass it. `git status` lists it,
# and it lists a changed file as well, so it is the one question the recipe
# asks of the artefacts. A file in the generated crate that git does not track
# and the generator does not write is a second question: the working tree has
# it and a clean checkout does not, so a build that passes here can fail in CI.

# Regenerate, and fail if an artefact differs from the commit, is not committed,
# or if a file in the generated crate is untracked.
blessed:
    #!/usr/bin/env bash
    set -euo pipefail
    # Only what the generator writes. `api-generated/src/{lib,client}.rs` are
    # hand-written and live in the same crate, and a recipe that failed on an
    # edit to those would be answering a different question.
    written=(
        examples/toy/api-generated/spec/toy.overlaid.yaml
        examples/toy/api-generated/src/types.rs
        examples/toy/api-generated/src/ops.rs
        examples/toy/api-generated/src/model.postcard
        examples/toy/api-generated/src/summary.md
    )
    # The bless step names every path it wrote, one per line.
    reported=$(just bless)
    echo "$reported"
    for path in "${written[@]}"; do
        grep -qF -- "$path" <<<"$reported" || {
            echo "bless no longer writes $path" >&2
            exit 1
        }
    done
    differing=$(git status --porcelain --untracked-files=all -- "${written[@]}")
    if [[ -n $differing ]]; then
        echo "bless changed or added output that is not committed:" >&2
        echo "$differing" >&2
        git --no-pager diff --stat HEAD -- "${written[@]}" >&2
        exit 1
    fi
    untracked=$(git ls-files --others --exclude-standard -- examples/toy/api-generated)
    if [[ -n $untracked ]]; then
        echo "files in api-generated that git does not track:" >&2
        echo "$untracked" >&2
        exit 1
    fi
    echo "bless writes and reproduces every committed artefact"

# What crates.io will receive. `--list` is the cheap half — it fails on missing
# metadata and prints the file set — and the build proves the crate stands up
# outside this workspace.
#
# The build is not enough: a crate can compile and still ship tests that do not,
# because the dev-dependency on itself that turns their features on is dropped
# from the tarball. So the recipe unpacks the `.crate` file, the very archive
# crates.io receives, and builds its tests twice. With every feature on they run
# and pass. With the default features none of them is built, because each test
# target declares the features it needs: a target that declares none is a
# compile error here and a green run in the workspace.
#
# The unpacked copy lives in a scratch directory outside the repository, because
# cargo refuses to build a package that sits inside a workspace it is not a
# member of, and `target/package/<name>-<version>/` does. `--manifest-path`
# keeps the working directory here, so `rustup` reads `rust-toolchain.toml` for
# the test run as it does for everything else, and `--target-dir` lets the run
# reuse the dependencies already compiled in that directory. `tar -m` stamps the
# unpacked files with the time of unpacking: a `.crate` carries one old date on
# every file, cargo takes a file no newer than its last build for unchanged, and
# the test binaries a former unpacking left behind, with that unpacking's
# directory compiled into them, would run in place of the ones this tarball
# builds.
#
# `--allow-dirty` so the recipe answers the same question while work is in
# progress. It is not a licence to publish a dirty tree: `cargo publish` is
# run by the release workflow on a merge to `main`, never from here.
#
# `cargo package` leaves the tarball and an unpacked copy of it at
# `<target>/package/`, a whole source tree — `src/`, `tests/` and all — inside a
# directory everything else reads as build output. A tool that walks the target
# directory takes that `tests/` for a build profile and looks for the nested
# target directories a profile of that name carries; `Swatinem/rust-cache` does,
# and reports the two it cannot find on every run it caches. The verification
# build's artefacts are in `<target>/debug` with the rest of the workspace's, so
# clearing the package directory costs the next run nothing. The target
# directory can come from `CARGO_TARGET_DIR`, from `CARGO_BUILD_TARGET_DIR` or
# from cargo's configuration, and the recipe reads only the first. So it names
# the directory itself, `--target-dir` on `cargo package`, and reads the tarball
# from and clears the one it named. It clears it before it packages too: a
# tarball left by an earlier version would make the unpacking below ambiguous.

# Verify the published crate packages, builds, and passes its own tests.
package:
    #!/usr/bin/env bash
    set -euo pipefail
    target="${CARGO_TARGET_DIR:-target}"
    packaged="$target/package"
    scratch=$(mktemp -d)
    trap 'rm -rf "$scratch" "$packaged"' EXIT
    rm -rf "$packaged"
    cargo package -p typed-openapi --list --allow-dirty
    cargo package -p typed-openapi --allow-dirty --target-dir "$target"
    tar -xzmf "$packaged"/typed-openapi-*.crate -C "$scratch"
    manifests=("$scratch"/typed-openapi-*/Cargo.toml)
    cargo test --all-features --manifest-path "${manifests[0]}" --target-dir "$target"
    cargo test --no-run --manifest-path "${manifests[0]}" --target-dir "$target"

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
