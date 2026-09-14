//! The Toy Accounting API as Rust: owned types, typed operations, one embedded
//! document.
//!
//! The document the vendor ships is wrong in five ways, and every correction
//! lives in an Overlay as standard OpenAPI Overlay actions: `spec/corrections.yaml`
//! for what the vendor got wrong, then `spec/cli.yaml` for what only a command
//! line needs. `cargo run -p xtask -- bless` applies them in that order and
//! writes four things into the `api-generated` crate beneath this one:
//!
//! - `spec/toy.overlaid.yaml` — the corrected document, embedded as
//!   [`DOCUMENT`]. It is the reviewable record of what the rest was emitted
//!   from, and `tests/typed.rs` holds the other three to it.
//! - `src/model.postcard` — that document already reduced to the facts a CLI
//!   needs. The CLI builds its whole command tree from it at startup, so the
//!   CLI and this crate cannot disagree about the API and neither one parses
//!   YAML to find out.
//! - `src/types.rs` — the schemas as Rust types. The Overlay names a `Money`
//!   schema and states its rule, so [`Money`] is generated with the rule inside
//!   its `FromStr`. There is no hand-written mirror: the generated types *are*
//!   this crate's types.
//! - `src/ops.rs` — [`OperationId`], one typed method per operation, and the
//!   `(operationId, method, path)` inventory [`ops::documented`] reads.
//!
//! They live one crate down so that an edit here recompiles the lines written
//! here and not the emitted volume. Everything the four artefacts offer is
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
//! Nothing forces the adopter to take a generated type. When they write one
//! themselves, two lints scoped to this crate keep it honest: together they
//! forbid both ways of writing a struct pattern that does not name every field
//! — `..` and `field: _` — so a conversion out of a generated type cannot
//! quietly ignore a field the vendor added. [`Posting`] is the worked example.
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
pub use api_generated::{Api, BodyError, Call, DOCUMENT, Error, NoContent, to_json};
pub use corrections::{CORRECTIONS, Correction};
pub use posting::Posting;
