//! The Toy Accounting API as Rust: owned types, typed operations, one embedded
//! document.
//!
//! The document the vendor ships is wrong in six ways, and every correction
//! lives in an Overlay as standard OpenAPI Overlay actions: `spec/corrections.yaml`
//! for what the vendor got wrong, then `spec/cli.yaml` for what only a command
//! line needs. `cargo run -p xtask -- bless` applies them in that order and
//! writes five things into the `api-generated` crate beneath this one:
//!
//! - `spec/toy.overlaid.yaml` — the corrected document, embedded as
//!   [`DOCUMENT`]. It is the reviewable record of what the rest was emitted
//!   from, and `tests/typed.rs` holds the reduction and the inventory to it.
//! - `src/model.postcard` — that document already reduced to the facts a CLI
//!   needs. The CLI builds its whole command tree from it at startup, so the
//!   CLI and this crate cannot disagree about the API and neither one parses
//!   YAML to find out.
//! - `src/types.rs` — the schemas as Rust types. Two fields of `Voucher` show
//!   the two ways a named schema gets one: the Overlay names a `Currency`
//!   schema and states its rule, so [`Currency`] is generated with that rule
//!   inside its `FromStr`; the Overlay tags an amount `format: money` and the
//!   bless step is told that [`money::Money`] stands for it, so [`Money`] is a
//!   transparent newtype over a fixed-point type this adoption owns and no
//!   OpenAPI document could have described. There is no hand-written mirror of
//!   a generated type anywhere.
//! - `src/ops.rs` — [`OperationId`], one typed method per operation, and the
//!   `(operationId, method, path)` inventory [`ops::documented`] reads.
//! - `src/summary.md` — what that reduction did, counted off it and embedded as
//!   [`SUMMARY`]. Every count this adoption states in prose is asserted against
//!   it by `tests/summary.rs`, so a sentence about how many operations there
//!   are fails when an Overlay adds one.
//!
//! They live one crate down so that an edit here recompiles the lines written
//! here and not the emitted volume. Everything the five artefacts offer is
//! re-exported from this crate, which is the only one an adopter's own code
//! needs to name.
//!
//! There is no hand-written list of operations to keep beside those: the
//! document is the list. What the adopter does keep by hand is [`CORRECTIONS`]
//! — one line per way this crate disagrees with the vendor — and
//! `tests/corrections.rs` holds it to both documents.
//!
//! ```no_run
//! let api = api::Api::new()?;
//! let client = ureq::Agent::config_builder().http_status_as_error(false).build();
//! # struct A; impl typed_openapi::SyncClient for A {
//! #   type Error = std::io::Error;
//! #   fn send(&self, _: typed_openapi::HttpRequest) -> Result<typed_openapi::HttpResponse, Self::Error> { unimplemented!() } }
//! # let client = A;
//! let voucher = api.get_voucher(5)?.send(&client)?;
//! println!("{} {}", voucher.total, voucher.currency);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Owning a type by hand
//!
//! Nothing forces the adopter to take a generated type, and this adoption owns
//! two. [`money::Money`] sits *below* the generated code, which names it: the
//! document says which shape an amount is and `Settings::replace` says which
//! Rust type that shape is, so the [`Money`] the document declares is a
//! transparent newtype over it and there is no conversion at the boundary.
//! [`Posting`] sits *above*, derived from a whole [`Voucher`] by the adopter's
//! own reading of it — and that is where the wrapper comes off, because a
//! ledger posting is the adopter's vocabulary and holds their own amount.
//!
//! Two lints scoped to this crate keep the second honest: together they forbid
//! both ways of writing a struct pattern that does not name every field — `..`
//! and `field: _` — so a conversion out of a generated type cannot quietly
//! ignore a field the vendor added.
#![warn(
    clippy::rest_pattern_accessible_field,
    clippy::unneeded_field_pattern,
    reason = "a hand-written type that silently skips a field of a generated \
              one is the bug these lints exist to prevent"
)]

mod corrections;
mod posting;

pub use api_generated::ops;
pub use api_generated::ops::{OPERATION_COUNT, OPERATIONS, OperationId, documented};
pub use api_generated::types::*;
pub use api_generated::{Api, BodyError, Call, DOCUMENT, Error, NoContent, SUMMARY, to_json};
pub use corrections::{CORRECTIONS, Correction};
// The crate holding the one type the generated code names rather than defines.
// It is re-exported whole so that an adopter's own code names `api` and nothing
// below it, and as a module rather than by item so that the two amounts keep
// their own names: [`Money`] is the schema the document declares, and
// [`money::Money`] is what this adoption counts cents with.
pub use money;
pub use posting::Posting;
