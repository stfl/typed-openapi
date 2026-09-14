# Generating

The bless step is the one command that turns a vendor's OpenAPI document and
your Overlay into committed Rust. You run it when the vendor publishes a
revision, review the diff, and commit it. Between revisions nothing generates:
your build compiles checked-in files.

The generator ships inside `typed-openapi` behind the `generate` feature, so
the only thing you write is a binary that calls it.

## Contents

- [The four artefacts](#the-four-artefacts)
- [`Settings`](#settings)
- [Layers](#layers)
- [Wiring your own `xtask`](#wiring-your-own-xtask)
- [The crate it writes into](#the-crate-it-writes-into)
- [The `builder` feature](#the-builder-feature)
- [Requirements and limits](#requirements-and-limits)

## The four artefacts

All four come out of one run of the Overlay chain, which is why none of them can
describe a different API from the others. `<name>` is the vendor document's own
file stem.

| Path | What it is | Who reads it |
|---|---|---|
| `spec/<name>.overlaid.yaml` | the vendor's document with your corrections applied | you, in review — and it is embedded so the crate can be held to it |
| `src/types.rs` | `components.schemas` as Rust types, from [typify]. A named schema stating a `pattern` becomes a newtype that enforces it, with `Display` beside the `FromStr` | your code, and the wrappers |
| `src/ops.rs` | one typed wrapper per operation, the closed `OperationId` set, and the `(operationId, method, path)` inventory | your code, and a CLI's dispatch |
| `src/model.postcard` | the corrected document already reduced to the facts a command line needs | the shipped binary, through `Document::from_blob` |

The reduction is the reason a binary parses no YAML on startup and enables
neither the `document` nor the `generate` feature: the expensive read happened
at bless time, and `src/model.postcard` is its result. The generator writes the
blob only after decoding it again and checking that what comes back is what it
reduced.

Every generated Rust file opens with the command that rewrites it, where a
correction belongs, and one `#![allow(...)]` block — generated source is not
graded on style, and the exemption travels with the file it exempts rather than
with a wrapping module.

## `Settings`

```rust
use typed_openapi::generate::Settings;

Settings::new("spec/vendor.yaml")
    .overlay("spec/corrections.yaml")
    .overlay("spec/cli.yaml")
    .replace("money", "money::Money")
    .regenerated_by("just bless")
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

It is the route for a type that needs *behaviour*, or one the document cannot
describe — a fixed-point decimal, say. A rule needs no Rust at all: name the
schema that states it and the generated newtype enforces it, on the command
line as well as in Rust. The two compose, and
[overlay.md](overlay.md#owning-the-type-yourself) has the trade written out.

### `Settings::regenerated_by(command) -> Settings`

The command a generated file tells its reader to run. Defaults to
`cargo run -p xtask -- bless`. Set it if your bless step is spelled differently;
the line is a promise to whoever opens the file next.

### `Settings::write_to(crate_dir) -> Result<Vec<PathBuf>, GenerateError>`

Write the four artefacts under `crate_dir` and answer with their paths, in the
order of the table above. Directories are created as needed. Each Rust file is
handed to `rustfmt` after it is written, so what lands in the tree is what
`cargo fmt --check` expects.

The sink is a directory rather than four values you place yourself: the
generated crate embeds the corrected document and the reduced model by relative
path, and each Rust header states where the others are, so the layout is not the
caller's to choose.

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
wrote. `examples/toy/xtask` is this file in the worked example.

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
- **The generator emits from what the document names.** A `$ref` to a named
  schema keeps its name; a request body or response the document spelled inline
  becomes `serde_json::Value`, because there was no name to generate a type
  under. `requestBody` `$ref`s are not followed, and only
  `#/components/schemas/` references are.
- **Parameters must be scalars.** A parameter declared with `content` rather
  than `schema`, or one whose schema is an object or an array, is a
  `GenerateError::Unsupported` rather than a guess.

[typify]: https://docs.rs/typify
[bon]: https://bon-rs.com
