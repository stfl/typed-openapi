# Tests and the target directory

**`rustfmt.toml` sets `format_code_in_doc_comments = true`.** A test directory
under `target/tmp` inherits it, because the nearest `rustfmt.toml` wins and
nothing merges. What decides is where the target directory is: the tests of the
tarball that the `package` recipe unpacks write under the same `<target>/tmp`,
so they read the repository's `rustfmt.toml` too, and a target directory outside
the repository (`CARGO_TARGET_DIR`) has none above it. A test that needs a
setting writes its own.

**A test is a process of its own under nextest and a thread of a shared one under
`cargo test`.** Each test that calls `out(name)` takes a directory name of its
own. A `LazyLock` in a test file is built once per test under nextest and once
per target under `cargo test`: it holds a value that both counts leave the same,
never a directory that a test writes. A new `[[test]]` target with
`required-features` runs in the `package` recipe at once.

`typed-openapi/tests/fixtures/shapes.json` is the library's own fixture for
`fits` and a copy of no toy file.

**A test or a task that reads `env!("CARGO_MANIFEST_DIR")` works on the tree that
its binary is built from.** `examples/toy/xtask` blesses that tree, and
`typed-openapi/tests/generate.rs`, `typed-openapi/tests/hostile.rs`,
`typed-openapi/tests/layout.rs` and `examples/toy/cli/tests/feature_gate.rs`
read it. Cargo takes a tree as built when its files are not newer than the last
build in the target directory, so a scratch copy of the tree that shares the
worktree's target directory can run in place of the worktree, and the worktree in
place of the copy. Touching the files of a copy does not part them: cargo builds
the copy, and the older files of the worktree count as built, so the worktree
runs the copy's binary. Only a target directory of its own for each copy keeps
the copy and the worktree apart.
