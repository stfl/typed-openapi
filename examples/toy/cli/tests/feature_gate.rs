//! The manifest shape that keeps the document reader out of the binary.
//!
//! `cli` must not be able to name `typed_openapi::Document::load`. Nothing in
//! this file asserts that, because no test in this workspace can: cargo
//! resolves features once per invocation over every package the invocation
//! builds, and the invocation that runs this test builds `api`'s
//! dev-dependency, which asks for the reader. In a test run the symbol exists.
//! `cargo check -p cli` is the build in which it does not, and it is a line in
//! the gate rather than a case in a suite.
//!
//! What a test can hold is the manifest shape that makes that command's answer
//! the right one: the feature is opt-in, its three dependencies are optional
//! with it, and the only member that asks for it from a section a shipping
//! build reads is the bless step. Break any of those and `cargo check -p cli`
//! goes green on a tree that has re-opened the door.

use std::path::{Path, PathBuf};

/// The crates a `cargo build` produces, as paths from the workspace root.
/// None of them reads documents.
const PRODUCT: &[&str] = &[
    "typed-openapi",
    "examples/toy/api-generated",
    "examples/toy/api",
    "examples/toy/cli",
];

/// The bless step, and the one member that does.
const BLESS: &str = "examples/toy/xtask";

/// The three dependencies the `document` feature is there to hold back.
const BEHIND_THE_FEATURE: &[&str] = &["openapiv3", "roas-overlay", "serde_yaml_ng"];

#[test]
fn the_document_reader_stays_out_of_every_build_that_ships() {
    let root = workspace_root();
    let runtime = manifest(&root, "typed-openapi");

    assert!(
        !default_features(&runtime).contains("document"),
        "`document` has reached `typed-openapi`'s default feature set, which is what \
         every consumer gets without asking; it must stay opt-in"
    );
    for dependency in BEHIND_THE_FEATURE {
        let declaration = declaration_of(&runtime, dependency);
        assert!(
            declaration.contains("optional = true"),
            "`typed-openapi` takes {dependency} unconditionally: {declaration}"
        );
    }

    for member in PRODUCT {
        let manifest = manifest(&root, member);
        assert!(
            !asks_for_the_reader_when_it_ships(&manifest),
            "`{member}` enables `document` from a section a build without test targets \
             reads, so its binary compiles the document stack again"
        );
    }

    // The same detector, pointed at the one manifest that should trip it: a
    // check that cannot say yes has not said no about the five above.
    assert!(
        asks_for_the_reader_when_it_ships(&manifest(&root, BLESS)),
        "`{BLESS}` no longer enables `document`, so either the bless step cannot \
         read a document or this check can no longer see that it does"
    );
}

/// Whether this manifest asks for `typed-openapi`'s `document` feature from a
/// section that reaches a build with no test targets in it.
///
/// `[dev-dependencies]` is not such a section, and that is the whole trick: the
/// witness tests may ask for the reader, and the binary still does not compile
/// it.
fn asks_for_the_reader_when_it_ships(manifest: &str) -> bool {
    sectioned(manifest).any(|(section, line)| {
        ships(section) && line.starts_with("typed-openapi") && line.contains("document")
    })
}

/// Whether a manifest section's dependencies reach a build with no test targets
/// in it. `[dependencies]` and `[build-dependencies]` do, under any `[target.…]`
/// prefix; the `dev-` ones do not.
fn ships(section: &str) -> bool {
    section.ends_with("dependencies]") && !section.contains("dev-dependencies")
}

/// Every line of a manifest, paired with the section header above it.
fn sectioned(manifest: &str) -> impl Iterator<Item = (&str, &str)> {
    manifest.lines().scan("", |section, line| {
        let line = line.trim();
        if line.starts_with('[') {
            *section = line;
        }
        Some((*section, line))
    })
}

/// The line that declares `name`, wherever it is declared.
fn declaration_of<'m>(manifest: &'m str, name: &str) -> &'m str {
    match manifest
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(name))
    {
        Some(line) => line,
        None => panic!("no `{name}` in the manifest"),
    }
}

fn manifest(root: &Path, member: &str) -> String {
    let path = root.join(member).join("Cargo.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => panic!("{}: {error}", path.display()),
    }
}

/// What `default = [...]` turns on, as it is written. A feature set spanning
/// several lines is read whole, because a manifest is free to wrap it.
fn declared_default(manifest: &str) -> String {
    let Some(rest) = manifest.split_once("default = [") else {
        return String::new();
    };
    rest.1
        .split_once(']')
        .map_or_else(String::new, |(set, _)| set.to_owned())
}

fn default_features(manifest: &str) -> String {
    declared_default(manifest)
}

/// `cli`'s manifest sits at `examples/toy/cli`, so the workspace root is three
/// directories up. Cargo sets the variable for every test it builds.
fn workspace_root() -> PathBuf {
    let member = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    match member.ancestors().nth(3) {
        Some(root) => root.to_path_buf(),
        None => panic!(
            "{} is not three levels below a workspace root",
            member.display()
        ),
    }
}
