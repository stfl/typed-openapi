# The `builder` feature, and what it costs

`bon`'s named-argument builder on the generated wrappers, so a call with four
arguments names them:

```rust
api.update_voucher().id(5).body(&voucher).call()?.send(&client)?
```

A missing required argument is a compile error rather than a runtime one, and
at four arguments and up that is worth having. It is **off by default**,
because it is not free and because of the constraint in the last section.

## What it costs

Measured on this repository's `examples/toy`, sequentially on an idle machine,
three repetitions per cell, median reported. `api-generated` is the crate that
holds the generated wrappers and therefore the crate the attribute lands in.

| | off | on | difference |
|---|---|---|---|
| crates in the normal dependency graph | 23 | 34 | **+11** |
| clean `cargo build -p api-generated` | 3.50 s | 5.66 s | **+2.16 s (+62%)** |
| warm `cargo check` after one edit to `ops.rs` | 0.105 s | 0.145 s | **+0.040 s (+38%)** |

The eleven crates are `bon`, `bon-macros`, `darling`, `darling_core`,
`darling_macro`, `syn` 2, `prettyplease`, `rustversion`, `fnv`, `ident_case`
and `strsim` — two of them proc-macros, and `syn` 2 alongside the `syn` 3 the
generator already uses.

None of that reaches a binary that leaves the feature off: the dependencies are
optional, so they are not resolved, not downloaded and not compiled.

Reproduce it with:

```sh
for f in "" "--features builder"; do
    rm -rf target && time cargo build -p api-generated $f
done
```

## How it is turned on

The feature lives on the crate that holds the generated code, and flipping it
never regenerates anything. `ops.rs` carries the attribute under `cfg_attr` in
every build, so the committed file is the same bytes either way:

```rust
#[cfg_attr(feature = "builder", ::typed_openapi::bon::bon(crate = ::typed_openapi::bon))]
impl Api { ... }
```

`bon` arrives re-exported as `typed_openapi::bon`, which is what keeps the
proc-macro version the one the generator emitted syntax for, and means the
generated crate adds no dependency of its own — its feature forwards to
`typed-openapi/builder`.

## The constraint: it changes the wrappers' signatures

**This feature replaces each generated method rather than adding to it.** With
it on, `api.enshrine_voucher(5)` does not compile; the call becomes
`api.enshrine_voucher().id(5).call()`. Everything that calls a generated
wrapper positionally has to change with it.

That matters more than it looks, because cargo resolves features once per
build over every package in it. One crate in a dependency graph turning
`builder` on turns it on for every other crate that shares the generated crate
— and breaks their call sites, which they did not ask for.

So, until the generator emits the builder as a second method beside the plain
one rather than in place of it:

- enable `builder` only in an application, never in a library other crates
  depend on;
- expect to convert every call site in the crates you own when you do.

`cargo check -p api-generated --features builder` passing is not evidence to
the contrary: `api-generated` does not call its own wrappers, so that command
compiles the definitions and nothing that uses them.
