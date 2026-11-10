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

`bon` and its proc-macro dependencies are compiled when the feature is on.
The generated crate gets a second `impl` block, and the macro writes the
named setters and their type checks. This adds work to compilation; the
binary contains the code the macro writes.

The dependencies are optional. A build with the feature off compiles no
`bon`; `just bon-free` checks the normal dependency graph in both builds.

Inspect the dependency graph for your adoption with:

```sh
cargo tree -p api-generated -e normal
cargo tree -p api-generated -e normal --features builder
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
