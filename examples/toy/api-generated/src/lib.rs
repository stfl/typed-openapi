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
//! - `src/types.rs` — `components.schemas` as Rust types, with
//!   [`api_types::Money`] substituted in wherever the document declares
//!   `format: money`.
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
//! runtime crate and [`api_types`]; nothing about a generator reaches it.

mod client;

pub mod ops;
pub mod types;

pub use client::{Api, BodyError, Call, DOCUMENT, Error, MODEL, NoContent, to_json};
