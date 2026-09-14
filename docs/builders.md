# The `builder` feature, and what it costs

`bon`'s named-argument builder beside every generated wrapper, so a call with
four arguments names them:

```rust
api.update_voucher_builder().id(5).body(&voucher).call()?.send(&client)?
```

A missing required argument is a compile error rather than a runtime one, and
at four arguments and up — where two `i64`s in a row are a bug waiting to be
written — that is worth having.

It **adds** a method; it never replaces one. `api.update_voucher(5, &voucher)`
means the same thing in both builds, which it has to: cargo resolves features
once for a whole build, so a feature that replaced the wrapper would break
every other crate that shares the generated code, without any of them asking
for it. A doctest in `examples/toy/api-generated/src/lib.rs` holds the two
forms to the same rendered request.

## What it costs

Measured on this repository's `examples/toy`, sequentially on an idle machine
(load 0.84, no other compiler running), three repetitions per cell, median
reported. `api-generated` is the crate that holds the generated wrappers and
therefore the crate the second `impl` block lands in.

| | off | on | difference |
|---|---|---|---|
| crates in the normal dependency graph | 23 | 34 | **+11** |
| clean `cargo build -p api-generated` | 3.78 s | 5.91 s | **+2.13 s (+56%)** |
| warm `cargo check` after one edit to `ops.rs` | 0.108 s | 0.146 s | **+0.038 s (+35%)** |
| clean `cargo build -p cli --release` | 10.49 s | 10.55 s | +0.06 s (noise) |
| the stripped `toy` binary | 4 954 032 B | 4 955 584 B | **+1 552 B (+0.03%)** |

**The cost is compile-time and local.** It lands on the crate holding the
generated code — eleven more crates to fetch and build, and about two seconds
on a clean build of that crate. A whole release build barely notices, because
it is dominated by everything else, and the binary grows by a page and a half:
`bon` is a proc-macro, so what reaches the binary is the code it wrote, and
that code is thin.

None of it reaches a build that leaves the feature off. The dependencies are
optional, so they are not resolved, not downloaded and not compiled.

The eleven crates are `bon`, `bon-macros`, `darling`, `darling_core`,
`darling_macro`, `syn` 2, `prettyplease`, `rustversion`, `fnv`, `ident_case`
and `strsim` — two of them proc-macros, and `syn` 2 alongside the `syn` 3 the
generator already uses.

Reproduce the first two rows with:

```sh
for f in "" "--features builder"; do
    rm -rf target && time cargo build -p api-generated $f
done
```

## How it is turned on

The feature lives on the crate that holds the generated code, and flipping it
never regenerates anything. `ops.rs` carries the second `impl` block under
`#[cfg]` in every build, so the committed file is the same bytes either way:

```rust
impl Api {
    pub fn update_voucher(&self, id: i64, body: &Voucher) -> Result<…> { … }
}

#[cfg(feature = "builder")]
#[::typed_openapi::bon::bon(crate = ::typed_openapi::bon)]
impl Api {
    #[builder]
    pub fn update_voucher_builder(&self, id: i64, body: &Voucher) -> Result<…> {
        self.update_voucher(id, body)
    }
}
```

The builder delegates rather than repeating the wrapper's body, so the two
cannot come to describe different requests.

`bon` arrives re-exported as `typed_openapi::bon`, which keeps the proc-macro
version the one the generator emitted syntax for and means the generated crate
adds no dependency of its own — its `builder` feature forwards to
`typed-openapi/builder`.

## What the gate checks

`cargo check -p api-generated --features builder` is not enough on its own:
that crate never calls its own wrappers, so it compiles the definitions and
nothing that uses them. A wrapper whose signature changed would pass it.

`just features` therefore also runs `cargo check -p cli --features builder
--all-targets`, which compiles a crate that does call them, and
`just bon-free` holds `bon` to appearing in the generated crate's tree only
with the feature on.
