//! <!-- The primer is the repository's README, which is also this crate's
//! front page on crates.io and docs.rs. What follows it here is the part a
//! reader wants *after* deciding to use the crate: the map of the API, and
//! what a feature is allowed to do to it. -->
#![doc = include_str!("../README.md")]
//!
//! # The API
//!
//! The six things a caller learns:
//!
//! - [`Document`] — the document, corrected and resolved.
//!   [`Document::from_blob`] takes the reduction back off the bytes
//!   [`Document::to_blob`] wrote.
//! - [`Values`] — arguments for one operation, under the document's own names.
//!   A CLI builds one from `ArgMatches`; a generated wrapper builds one from
//!   typed arguments.
//! - [`Invocation`] — an operation and values that satisfy it. Making one is
//!   the validation; [`Invocation::request`] is then a rendering.
//! - [`Plan`] — the gate. A read runs on sight; a write runs only once
//!   confirmed, and until then a dry run prints the exact bytes a confirmed
//!   run would send.
//! - `tree::commands` and `tree::dispatch` — the clap tree, and the trip back.
//! - [`SyncClient`] / [`AsyncClient`] — where the request meets the network.
//!
//! The document is data: an operation is a value in a list, not a branch
//! someone wrote, so there is no chance of the CLI disagreeing with the
//! document it shipped with, and the tree, the completion and the request
//! builder all read the same list.
//!
//! Reading that list out of YAML is not, however, something a shipped binary
//! should do on every invocation, and under the default feature set it is not
//! something a shipped binary compiles. `Document::load` is the expensive door
//! and `document` is what opens it; [`Document::from_blob`] is the door a
//! binary uses, and it takes the reduction a bless step already wrote down.
//!
//! # What a feature may do
//!
//! Every feature adds and removes whole items and never changes one. No type
//! on this page gains a variant or a field with one, so a caller who matches
//! an error of this crate exhaustively writes the same match in every build,
//! and what the docs say about a type they can see is true of every build that
//! has it.
//!
//! `clap` is on by default because most adopters want the command tree; a
//! crate that only wants typed calls turns it off and links no argument
//! parser. `document` and `generate` belong to the bless step, and a shipping
//! binary that enabled either would compile a YAML parser, an OpenAPI object
//! model and a code generator it can never reach.

#[cfg(feature = "clap")]
pub mod tree;

#[cfg(feature = "document")]
pub mod overlay;
#[cfg(feature = "document")]
pub mod schema;

#[cfg(feature = "generate")]
pub mod generate;

pub mod client;
pub mod model;
pub mod multipart;
pub mod names;
pub mod plan;
pub mod request;
pub mod scalar;
pub mod transport;
pub mod values;

/// `bon`, for generated code to name the builder macro through.
///
/// A generated `ops.rs` writes `#[bon(crate = ::typed_openapi::bon)]`, so the
/// crate holding it turns the builder on with one feature and adds no
/// dependency of its own — and the proc-macro version stays the one the
/// generator emitted syntax for.
#[cfg(feature = "builder")]
pub use bon;
pub use client::{Call, Client, NoContent};
#[cfg(feature = "document")]
pub use model::LoadError;
pub use model::{
    Body, COMMIT, Document, DocumentError, Effect, Field, JSON_BODY, Location, Operation, Param,
    RAW_BODY,
};
pub use names::{CommandName, kebab};
pub use plan::{Plan, PlanError};
/// `regress`, for generated code to name the regex engine through.
///
/// A generated `types.rs` enforces a schema's `pattern` inside `FromStr`, and
/// the generator points every such check at `::typed_openapi::regress`, so the
/// crate holding the generated code adds no dependency of its own — and the
/// engine stays the one the generator emitted syntax for, which is the same one
/// [`Scalar::parse`] runs the command line's values through.
pub use regress;
pub use request::{Invocation, ValueError, render};
pub use scalar::Scalar;
pub use transport::{
    AsyncClient, HttpRequest, HttpResponse, Recorder, RecorderError, SyncClient, json_response,
};
pub use values::{Part, Payload, Values};
