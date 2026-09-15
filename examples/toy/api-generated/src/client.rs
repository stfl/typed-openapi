//! The three embedded artefacts, and the handle the generated wrappers hang
//! off.
//!
//! Everything generic — building a request, sending it, mapping a status into
//! an error, deserialising the answer — lives in [`typed_openapi::client`].
//! What is specific to this vendor is exactly two things: which bytes the
//! document is, and the name of the type the wrappers are written on.

use http::Uri;
use typed_openapi::client::Client;
pub use typed_openapi::client::{BodyError, Call, Error, NoContent, to_json};
use typed_openapi::{Document, Operation, Values};

use crate::ops::{OPERATIONS, OperationId};

/// The corrected document, written by `cargo run -p xtask -- bless` and
/// committed. This is the single source everything in this crate was emitted
/// from: the types, the wrappers and [`MODEL`] all came out of these bytes in
/// one run, and `api/tests/typed.rs` reduces them again to prove it.
pub const DOCUMENT: &str = include_str!("../spec/toy.overlaid.yaml");

/// [`DOCUMENT`] already reduced to the facts a command line needs — the
/// operations, their flags, their bodies and the write gate — written by the
/// same bless step and read by [`Api::new`].
///
/// The reduction is a YAML parse, an `openapiv3` deserialisation and a walk
/// over every path item resolving every `$ref`. Doing it once at bless is the
/// difference between a CLI that starts in about a millisecond against a large
/// API and one that does not; the tree itself is still built from this list at
/// startup, so a `--help` still describes the document rather than a snapshot
/// someone generated clap code from.
pub const MODEL: &[u8] = include_bytes!("model.postcard");

/// What that reduction did, as a page: the operations, the groups, the reads
/// and writes, what stands behind each named gate, and the parameters carried
/// without a flag. Written by the same bless step, off the same reduction.
///
/// This is what a doc page or a README quotes a count out of. Quoting it is
/// only half the cure — a page that quotes a number still fails nothing when
/// the number moves — so `api/tests/summary.rs` holds this adoption's own prose
/// to [`typed_openapi::Summary`], which is where these bytes came from.
pub const SUMMARY: &str = include_str!("summary.md");

/// The Toy Accounting API, as the document describes it.
///
/// The newtype is what lets `src/ops.rs` write `impl Api` — a generated method
/// on a foreign type would not compile.
#[derive(Debug)]
pub struct Api(Client);

impl Api {
    /// Load the embedded model, and pair it with the generated inventory.
    ///
    /// The pairing is the whole reason [`Api::call`] has no "no such operation"
    /// branch: after it succeeds, every [`OperationId`] names the operation at
    /// its own position in the model. Two committed artefacts that have drifted
    /// apart — a hand-edited blob, a half-finished bless — are a named error
    /// here, before a command tree is built or a request is made.
    ///
    /// Reading [`DOCUMENT`] here instead would make `api`'s witness test
    /// compare a document with itself, and the compiler is what stops it: this
    /// crate depends on `typed-openapi` without its `document` feature, so
    /// `Document::load` does not exist in this build and the blob is the only
    /// reduction there is.
    pub fn new() -> Result<Self, Error> {
        let client = Client::over(Document::from_blob(MODEL)?)?;
        client.document().matches(OPERATIONS)?;
        Ok(Self(client))
    }

    /// Point at a different server than the document's first one.
    #[must_use]
    pub fn with_base(self, base: Uri) -> Self {
        Self(self.0.with_base(base))
    }

    #[must_use]
    pub fn base(&self) -> &Uri {
        self.0.base()
    }

    /// The document itself, for a caller that wants the command tree or an
    /// operation the wrappers do not cover.
    #[must_use]
    pub fn document(&self) -> &Document {
        self.0.document()
    }

    /// What the document says about one operation.
    #[must_use]
    pub fn operation(&self, op: OperationId) -> &Operation {
        #[expect(
            clippy::indexing_slicing,
            reason = "`Api::new` refused any model whose operation list is not \
                      row-for-row `OPERATIONS`, and `OperationId`'s \
                      discriminants are that list's positions"
        )]
        &self.document().operations()[op as usize]
    }

    /// Name an operation and the values for it.
    ///
    /// Every generated wrapper is one call to this. A hand-written operation is
    /// too — see [`crate::ops::documented`] for the assertion that keeps a
    /// hand-written `operationId` honest at compile time.
    pub fn call<T>(&self, op: OperationId, values: Values) -> Result<Call<'_, T>, Error> {
        self.0.call(self.operation(op), values)
    }
}
