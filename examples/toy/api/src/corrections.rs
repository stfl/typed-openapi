//! Every way this crate's API differs from the vendor's document, in one list.
//!
//! The corrections themselves live in `spec/corrections.yaml` and
//! `spec/cli.yaml`, because a standard Overlay is what the eight tools in the
//! exploration could all read. That makes the Overlay the mechanism — but two
//! YAML files of JSONPath targets are not what a reviewer wants to read to
//! answer "what have we changed, and is it still needed?". [`CORRECTIONS`] is
//! that answer, one line per decision.
//!
//! It is not a second copy of the Overlays, because every row is checked against
//! both documents by `tests/corrections.rs`:
//!
//! - a row that no longer describes a real difference fails — the vendor has
//!   caught up, and the correction should go;
//! - a real difference with no row fails — someone changed an Overlay without
//!   saying so here.
//!
//! So the list cannot drift from the Overlays in either direction, and the
//! survey a regeneration owes the adopter is a test rather than a diff.

/// One difference between the vendor's document and the API this crate offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    /// An operation the vendor ships and the document never mentions. The
    /// Overlay adds it to `paths`, so it becomes a generated wrapper and a
    /// subcommand like any other and costs no Rust at all.
    Undocumented(&'static str),
    /// An operation the document describes that this crate does not offer,
    /// removed from `paths` by the Overlay.
    Skipped(&'static str),
    /// An operation the document declares with a safe method that this crate
    /// nevertheless holds behind `--commit`, marked `x-cli-writes` by the
    /// Overlay. HTTP cannot say "this GET writes"; the document has to.
    Gated(&'static str),
    /// A property the vendor types as a bare string under a `format` it never
    /// states the rule for. The Overlay writes the rule down as the named
    /// schema `named` and points the property at it, so the rule travels with
    /// the document — and the generated Rust carries it as a newtype rather
    /// than a `String`.
    Retyped {
        schema: &'static str,
        property: &'static str,
        named: &'static str,
    },
    /// A property the vendor accepts and returns and never documented, added to
    /// a schema by the Overlay. It becomes a struct field, a CLI flag and a
    /// wrapper argument at once.
    Undeclared {
        schema: &'static str,
        property: &'static str,
    },
}

/// The whole patch surface: every skip, gate, retype and hand-written decision
/// this crate makes about the vendor's document.
///
/// There is no separate list of operations to mount. The document *is* that
/// list — the generated [`OPERATIONS`] inventory is emitted from it and
/// `Api::new` pairs the two — so the only thing left to write down is where
/// this crate and the vendor disagree, which is this.
///
/// [`OPERATIONS`]: crate::OPERATIONS
pub const CORRECTIONS: &[Correction] = &[
    // The vendor types an amount as a bare string and gives it `format: money`
    // without saying what an amount looks like. The Overlay says it, under a
    // name, and the bless step turns the name into a type.
    Correction::Retyped {
        schema: "Voucher",
        property: "total",
        named: "Money",
    },
    // Returned and accepted on every voucher; documented nowhere.
    Correction::Undeclared {
        schema: "Voucher",
        property: "internal_ref",
    },
    // Shipped by the vendor, absent from the document.
    Correction::Undocumented("archiveVoucher"),
    // The vendor's own summary says this GET stores a PDF on the server.
    Correction::Gated("renderVoucher"),
    // Nothing is skipped. Both uploads work — the misspelled `form-data` one
    // through `--raw-body`, the correctly spelled one through `--file`/`--field`
    // — so there is no operation this crate refuses to offer.
];
