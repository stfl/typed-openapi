//! Everything `cargo run -p xtask -- bless` writes, and the handle the
//! generated methods are written on.
//!
//! Four files here are emitted by the bless step and none of them is edited:
//!
//! - `spec/toy.overlaid.yaml` — the corrected document, embedded as
//!   [`DOCUMENT`]. It is the reviewable record of what this crate was built
//!   from, and the witness `api/tests/typed.rs` holds [`MODEL`] to.
//! - `src/model.postcard` — that same document already reduced to the facts a
//!   CLI needs, embedded as [`MODEL`]. This is what [`Api::new`] loads, so a
//!   run of the CLI parses no YAML at all.
//! - `src/types.rs` — `components.schemas` as Rust types. A named schema that
//!   states a `pattern` becomes a newtype enforcing it, so
//!   [`types::Currency`] cannot be built out of something that is not a
//!   currency code. A schema tagged with a `format` the bless step was given a
//!   Rust path for becomes that type instead, which is why `Voucher.total` is
//!   a [`money::Money`] and this crate declares the crate holding it.
//! - `src/ops.rs` — [`ops::OperationId`], one typed method per operation, and
//!   the `(operationId, method, path)` inventory [`ops::documented`] reads.
//!
//! `src/client.rs` is the exception: [`Api`] itself, the newtype the generated
//! `impl` block hangs off. A generated method on a foreign type would not
//! compile, so the handle has to live beside what is generated for it. That
//! file and this one are the only hand-written lines in the crate, and the
//! bless step never touches either.
//!
//! This is a separate crate so that an edit to `api` recompiles the adopter's
//! lines and not this volume. Its whole dependency list is `serde`, `http`, the
//! runtime crate and `money` — the regex engine a generated `pattern` check
//! runs on arrives re-exported through the runtime crate, so there is nothing
//! to declare for that and nothing about a generator reaches this crate.

mod client;

pub mod ops;
pub mod types;

pub use client::{Api, BodyError, Call, DOCUMENT, Error, MODEL, NoContent, to_json};

/// The named-argument builder the `builder` feature adds beside every
/// generated wrapper.
///
/// It holds nothing; it is where the feature is documented and demonstrated,
/// because the wrappers themselves live in a generated file that says nothing
/// about style.
///
/// A wrapper with four arguments reads as four positional values at the call
/// site, and two `i64`s in a row are a bug waiting to be written. The feature
/// adds a `_builder` method beside each wrapper, where every argument is named
/// and every required one is enforced by the type system rather than by
/// argument order:
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use api_generated::Api;
/// use api_generated::types::{Voucher, VoucherStatus};
///
/// let api = Api::new()?;
/// let voucher = Voucher {
///     currency: "EUR".parse()?,
///     id: Some(5),
///     internal_ref: None,
///     status: VoucherStatus::Draft,
///     total: "12.50".parse()?,
/// };
///
/// let named = api.update_voucher_builder().id(5).body(&voucher).call()?;
/// assert_eq!(named.request()?.uri().path(), "/vouchers/5");
///
/// // The positional wrapper is still there and still means what it meant.
/// // That is the point of the suffix: cargo resolves features once for a
/// // whole build, so a feature that *replaced* `update_voucher` would break
/// // every other crate that shares this one.
/// let positional = api.update_voucher(5, &voucher)?;
/// assert_eq!(
///     typed_openapi::render(&positional.request()?),
///     typed_openapi::render(&named.request()?),
/// );
/// # Ok(())
/// # }
/// ```
///
/// A required argument left out does not compile, so the builder cannot turn a
/// four-argument call into a three-argument one that sends the wrong request:
///
/// ```compile_fail
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let api = api_generated::Api::new()?;
/// let call = api.update_voucher_builder().id(5).call()?;
/// # Ok(())
/// # }
/// ```
///
/// Turning the feature off costs nothing but the names: `src/ops.rs` carries
/// the second `impl` block under `#[cfg]` in every build, so the committed
/// file is the same bytes either way and no regeneration is involved.
#[cfg(feature = "builder")]
pub mod builder {}
