# Generating

The bless step is the one command that turns a vendor's OpenAPI document and
your Overlay into committed Rust. You run it when the vendor publishes a
revision, review the diff, and commit it. Between revisions nothing generates:
your build compiles checked-in files.

The generator ships inside `typed-openapi` behind the `generate` feature, so
the only thing you write is a binary that calls it.

## Contents

- [The artefacts](#the-artefacts)
- [`Settings`](#settings)
- [Layers](#layers)
- [Wiring your own `xtask`](#wiring-your-own-xtask)
- [The crate it writes into](#the-crate-it-writes-into)
- [The `builder` feature](#the-builder-feature)
- [Requirements and limits](#requirements-and-limits)

## The artefacts

Four files, plus one you ask for. All of them come out of one run of the Overlay
chain, which is why none of them can describe a different API from the others.
`<name>` is the vendor document's own file stem.

| Path | What it is | Who reads it |
|---|---|---|
| `spec/<name>.overlaid.yaml` | the vendor's document with your corrections applied | you, in review — and it is embedded so the crate can be held to it |
| `src/types.rs` | `components.schemas` as Rust types, from [typify]. A named schema stating a `pattern` becomes a newtype that enforces it, with `Display` beside the `FromStr` | your code, and the wrappers |
| `src/ops.rs` | one typed wrapper per operation, the closed `OperationId` set, and the `(operationId, method, path)` inventory | your code, and a CLI's dispatch |
| `src/model.postcard` | the corrected document already reduced to the facts a command line needs | the shipped binary, through `Document::from_blob` |

Those four land where the table says, because they name each other by relative
path. The fifth is the [summary page](#the-summary-page): it appears only where
[`Settings::summary_page`](#settingssummary_pagepath---settings) names a path,
and that path is yours, because nothing generated reads it.

The reduction is the reason a binary parses no YAML on startup and enables
neither the `document` nor the `generate` feature: the expensive read happened
at bless time, and `src/model.postcard` is its result. The generator writes the
blob only after decoding it again and checking that what comes back is what it
reduced.

Every generated Rust file opens with the command that rewrites it, where a
correction belongs, and one `#![allow(...)]` block — generated source is not
graded on style, and the exemption travels with the file it exempts rather than
with a wrapping module.

### The summary page

Adoptions that count something about their own API — how many operations there
are, how many of them write, which stand behind which gate — put the number in a
doc comment, a README or a reference page. Nothing re-counts it when an Overlay
adds an operation, so the page goes quietly stale and reads exactly like a page
that is right.

The summary page is that count taken off the reduction instead. Name a path and
the bless step writes it there:

```rust
Settings::new("spec/vendor.yaml")
    .summary_page("src/summary.md")
    .write_to("api-generated")?;
```

It opens with an HTML comment naming the command that counts it again and every
document it was counted from, then carries a table of the tallies, the groups by
name, one row per gate, and one row per parameter carried without a flag — the
operation, the parameter, and why. Nothing on it is stored in the model:
`Document::summary` derives every number from the operations the model already
holds, so the page and the blob cannot come apart, and neither can carry a
number the other does not.

Two consumers, one measurement. Quote the page in your prose, and assert against
[`Summary`] in a test:

```rust
let summary = api.document().summary();
assert!(README.contains(&format!("**{} operations**", summary.operations())));
```

`Summary` needs no page and no feature: it is derived from a reduced `Document`,
so a binary that loaded a blob answers the question a bless step answered. That
is the split — the count is always available, the *file* is what you opt into.
`examples/toy/api/tests/summary.rs` is that test in the worked example, holding
the toy adoption's own README paragraph to the reduction behind it — which is
the half that matters, because a page quoting a count fails nothing when the
count moves unless something holds the page to it.

If you ask for the page, put it in the list your bless check compares against
the committed tree. An artefact nothing checks is an artefact that can rot, and
this is the one adopters quote numbers out of. If your prose quotes no count,
ask for nothing: a Markdown file in a Rust source directory is one more thing to
commit, to review and to remember.

[`Summary`]: https://docs.rs/typed-openapi/latest/typed_openapi/summary/struct.Summary.html

## `Settings`

```rust
use typed_openapi::generate::Settings;

Settings::new("spec/vendor.yaml")
    .overlay("spec/corrections.yaml")
    .overlay("spec/cli.yaml")
    .replace("money", "money::Money")
    .regenerated_by("just bless")
    .summary_page("src/summary.md")
    .write_to("api-generated")?;
```

### `Settings::new(document) -> Settings`

The vendor's document, as a path. It is read when `write_to` runs, and it is
named in the header of every file written, so a reader of a generated file can
find what it came from.

### `Settings::overlay(overlay) -> Settings`

Lay one Overlay over the document, after every Overlay already named. Call it
once per layer; calls accumulate, and the order of the calls is the order the
layers are applied.

Every layer is named in the headers too, in that order. A layer that does not
read, or does not apply, names itself in the error — which is the practical
reason to have more than one. [overlay.md](overlay.md#layers) has the split
this repository recommends, and what each layer buys.

### `Settings::replace(format, rust_type) -> Settings`

Emit `rust_type` wherever the document declares `format`. Call it once per
format; calls accumulate.

This is where a type you already own replaces the `String` the document would
otherwise produce. A schema of `format: money` becomes your own `Money`, with
no hand-written mirror on top of the generated struct and no conversion at the
boundary.

The key is the format, not a schema name, so a document that spells one format
two ways gets one substitution per spelling rather than a silent miss on the
second.

`rust_type` is written into the generated source verbatim, so it must be a path
the generated crate can name — which means the crate that owns the type is a
dependency of the generated crate.

**What your type has to implement.** Your type needs the `Serialize`,
`Deserialize`, `Clone`, `Debug` and `PartialEq` that every generated type
derives, wherever it appears. Each missing one is a single error naming your
type and the trait.

It needs `FromStr` and `Display` as well whenever the document *names* a schema
carrying the format. A named schema becomes a `#[serde(transparent)]` newtype
**over** your type — `Cents` against `money::Cents` and `Amount` against
`money::Cents` alike, because what the schema is called has nothing to do with
it — and that wrapper's impls are written in terms of yours: `Display` forwards
to it, `FromStr` parses into it and names `<YourType as FromStr>::Err` as its
own error, and both `TryFrom`s go through that. `Deref` is there too, so a
method on your type is a method on the wrapper.

The wrapper is `#[serde(transparent)]`, so the wire form is your type's: it
costs a name and a `.0`, never a byte. Reach through it where you want the type
itself — an operator or an associated function is your type's, not the
wrapper's.

Where the format is declared **without** a name — on a property, on a list's
items, on a body the document states where it uses it — your type stands alone.
Nothing is written in terms of it there, so neither trait is asked for.

`types.rs` carries a `const _` block asserting both traits against your type
wherever a wrapper is written, so a missing one is a single error naming your
type, the trait, and the line that asked for it — rather than the `E0271`s and
`E0276`s about an unresolvable associated type that the wrapper's own impls
would otherwise produce, tens of thousands of lines into a file you did not
write.

Your type is reached through a definition the bless step adds to the schemas it
hands the generator, under a name of its own. A vendor schema whose Rust name
is that one is refused by name — *"the schema `…` reduces to a Rust name this
crate declares a type `Settings::replace` substitutes under"* — because the
alternative is that schema quietly becoming your type everywhere it is used.
Rename it in an Overlay.

`FromStr` is also why the rule and the type do not check each other. A schema
whose format is replaced hands the whole reading to your `FromStr`: the
document's `pattern` reaches the command line, and nothing re-runs it inside the
wrapper. The two can therefore disagree, and only a test holds them together —
see [owning the type yourself](overlay.md#owning-the-type-yourself).

It is the route for a type that needs *behaviour*, or one the document cannot
describe — a fixed-point decimal, say. A rule needs no Rust at all: name the
schema that states it and the generated newtype enforces it, on the command
line as well as in Rust. The two compose, and
[overlay.md](overlay.md#owning-the-type-yourself) has the trade written out.

### `Settings::regenerated_by(command) -> Settings`

The command a generated file tells its reader to run. Defaults to
`cargo run -p xtask -- bless`. Set it if your bless step is spelled differently;
the line is a promise to whoever opens the file next.

### `Settings::summary_page(path) -> Settings`

Render the [summary page](#the-summary-page) too, at `path`. Call it once; the
last call wins. Leave it out and no page is written and none is reported.

`path` is relative to the `crate_dir` you hand `write_to`, so `src/summary.md`
sits beside the blob where an `include_str!` in your own `lib.rs` can reach it.
An absolute path lands where it says, and a relative one may climb out —
`../docs/how-big.md` puts the page next to the prose that quotes it.

The path is yours because nothing generated reads this page. The other four
artefacts embed each other by relative path, which is why `write_to` decides
where *they* go; a summary is reached only by whatever you point at it.

### `Settings::write_to(crate_dir) -> Result<Vec<PathBuf>, GenerateError>`

Write the artefacts under `crate_dir` and answer with their paths: the four of
the table above, in that order, then the summary page if you asked for one.
Directories are created as needed. Each Rust file is handed to `rustfmt` after
it is written, so what lands in the tree is what `cargo fmt --check` expects.

The answer is what was written, so a run that asked for no page reports four
paths and a script that copies or checks the answer copies or checks four files.

The sink is a directory rather than four values you place yourself: the
generated crate embeds the corrected document and the reduced model by relative
path, and each Rust header states where the others are, so that layout is not
the caller's to choose.

### `GenerateError`

One `thiserror` enum, non-exhaustive. A bless step reports and exits, so nothing
in it is meant to be branched on — it names the thing to go and fix. The cases
worth knowing about in advance:

| Case | Means |
|---|---|
| `Overlay` | a document did not read, or a correction did not apply — the message opens with the file. See [drift.md](drift.md) |
| `Unusable` | the corrected document does not describe a usable CLI |
| `Unsupported` | the document declares something the generator has no Rust spelling for |
| `RustfmtMissing` | `rustfmt` is not on `PATH` |

Every Overlay is applied with `ErrorOnZeroMatch`. A correction whose target the
vendor has renamed or retyped fails here rather than lapsing quietly, which is
the loudest thing a vendor revision can do.

## Wiring your own `xtask`

Add a workspace member with one dependency:

```toml
[package]
name = "xtask"
edition = "2024"
publish = false

[dependencies]
typed-openapi = { version = "0.1", features = ["generate"] }
```

and one file:

```rust
use std::path::Path;
use std::process::ExitCode;

use typed_openapi::generate::Settings;

fn main() -> ExitCode {
    // The adoption this xtask blesses is the directory holding it.
    let adoption = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let blessed = Settings::new(adoption.join("spec/vendor.yaml"))
        .overlay(adoption.join("spec/corrections.yaml"))
        .overlay(adoption.join("spec/cli.yaml"))
        .write_to(adoption.join("api-generated"));

    match blessed {
        Ok(written) => {
            for path in written {
                println!("blessed {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bless: {error}");
            ExitCode::FAILURE
        }
    }
}
```

Then `cargo run -p xtask -- bless`, review the diff, and commit everything it
wrote. `examples/toy/xtask` is this file in the worked example, with a
`.summary_page("src/summary.md")` line because that adoption quotes counts in
its README.

Keep `xtask` out of `default-members`. Cargo resolves features once per
invocation over the packages it builds, so leaving the bless step out of the
everyday set is what stops a plain `cargo build` from resolving `typed-openapi`
with `generate` and compiling typify. `--workspace` still selects it.

## The crate it writes into

`write_to` fills `src/` and `spec/` of a crate you own; it never writes a
`Cargo.toml` or a `lib.rs`. Those are yours, and they are short. The example's
is `examples/toy/api-generated`, whose hand-written half is two files: a
`lib.rs` declaring `pub mod ops; pub mod types;`, and a `client.rs` holding the
newtype the generated `impl` block hangs off — a generated method on a foreign
type would not compile, so the handle has to live beside what is generated for
it.

Its dependencies are the runtime crate with `default-features = false`, `serde`,
`http`, and whatever crate owns the types you passed to `replace` — which has to
sit *below* it, because the generated source names it. Nothing about a generator
reaches it.

Make it a crate of its own rather than a module. An edit to your hand-written
code then recompiles your lines and not the generated volume.

## The `builder` feature

`typed-openapi`'s `builder` feature puts [bon]'s named-argument builder on every
generated wrapper:

```rust
let voucher = api.update_voucher().id(5).body(&draft).call()?.send(&client)?;
```

A missing required argument is a compile error, not a wrong request.

Turn it on from the crate that holds the generated code:

```toml
[features]
builder = ["typed-openapi/builder"]
```

Three things are worth knowing:

- **Flipping it never regenerates.** The generated `ops.rs` carries the
  attributes under `cfg_attr` in every build, so the committed file is the same
  bytes with the feature on and off, and the feature flag is the whole
  difference.
- **You add no dependency.** `bon` is named through `typed_openapi::bon`, so the
  proc-macro that expands the attribute is the version the generator emitted
  syntax for.
- **It costs compile time.** Turning it on puts two proc-macro crates in the
  dependency graph of the crate that holds the generated code: TBD — measured in
  [builders.md](builders.md).

## Requirements and limits

- **`rustfmt` must be on `PATH`.** The bless step formats every Rust file it
  writes; without it, `write_to` fails by name rather than leaving unformatted
  source in your tree.
- **`generate` implies `document`,** and pulls in typify, an OpenAPI object
  model and a syntax-tree printer. Enable it only in the bless step. A shipping
  binary that enabled it would compile a code generator it can never reach.
- **`generate` needs a newer compiler than the crate's MSRV.** typify reaches
  `regress`, which uses let-chains. That costs a shipping binary nothing,
  because a shipping binary never enables the feature.
- **The generator emits from what the document describes, named or not.** A
  `$ref` to a named schema keeps that schema's name; a body or response an
  operation states inline is converted under its own `title`, or under a name
  derived from the operation. `serde_json::Value` is left for a JSON body the
  document states no schema for. `requestBody` `$ref`s are not followed, and
  only `#/components/schemas/` references are.
- **A `content` key must be a media type.** A request body declared under a key
  with no `type/subtype` in it — `form-data` where `multipart/form-data` was
  meant — is a `LoadError` naming the operation and the key, rather than bytes
  sent under a `Content-Type` no server reads. The repair is an Overlay action,
  and [overlay.md](overlay.md#1-plain-corrections) writes one out.
- **A parameter becomes an argument only where a command line could spell it.**
  A scalar is one argument; an array of scalars is a `Vec` argument the wrapper
  fills by repeating the wire name, which is what a repeated flag does. A
  parameter neither can supply — `in: cookie`, one declared with `content`
  rather than `schema`, one whose schema is an object, one whose name has no
  kebab-case spelling — is an argument on neither, and the wrapper's own
  documentation says which parameter it does not carry and why. An array must
  declare its `items` inline: a `$ref` to an array schema is a
  `GenerateError::Unsupported` naming the parameter.
- **Two values that reduce to one Rust word get one argument each.** A document
  may name a parameter `self` and another `Self`, or use one wire name in the
  path and again in the query; a function signature binds each name once, so
  the later of the two is `self_2` or `ref_2`. What it sends is unaffected —
  the wire name travels beside the argument as a literal — and the wrapper's
  own documentation says which name the moved argument carries, the way the
  flag that moved aside says so on its help line.

[typify]: https://docs.rs/typify
[bon]: https://bon-rs.com
