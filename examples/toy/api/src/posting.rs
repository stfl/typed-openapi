//! A type the adopter owns outright, derived from a generated one.
//!
//! Most types should be generated: the Overlay corrects the document and
//! typify emits the result, so `internal_ref` and [`Currency`] cost no
//! hand-written Rust at all. But an adopter's own vocabulary is not the
//! vendor's, and a ledger posting is not a voucher.
//!
//! The conversion is the interesting part. It destructures [`Voucher`] with
//! every field named and no `..`, which is what turns a document change into a
//! compile error:
//!
//! - the vendor **adds** a field → `error[E0027]: pattern does not mention
//!   field ...`, naming it;
//! - the vendor **removes** or **renames** one → `error[E0026]: struct
//!   `Voucher` does not have a field named ...`.
//!
//! The two lints `lib.rs` turns on for this crate — `rest_pattern_accessible_field`
//! and `unneeded_field_pattern` — close the two ways out of that: `..` and
//! `field: _`. Neither compiles here, so the tripwire cannot be disarmed by
//! accident.
//!
//! [`Currency`]: crate::Currency
//! [`Voucher`]: crate::Voucher

use crate::{Currency, Voucher, VoucherStatus};

/// One line of the adopter's ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posting {
    /// The vendor's undocumented reference if there is one, else the voucher
    /// id, else the literal `unbooked` — a posting always has a reference.
    pub reference: String,
    /// The adopter's own amount rather than the newtype the document's `Money`
    /// schema became: a posting is the adopter's vocabulary, and what it wants
    /// of an amount is the arithmetic. The wrapper is transparent, so taking it
    /// off costs a `.0` and nothing else.
    pub amount: money::Money,
    pub currency: Currency,
    /// `true` once the voucher has been enshrined, which is what a draft has
    /// not been.
    pub booked: bool,
}

impl Posting {
    /// Derive a posting from a voucher. Pure; no network, no client.
    #[must_use]
    pub fn of(voucher: &Voucher) -> Self {
        let Voucher {
            currency,
            id,
            internal_ref,
            status,
            total,
        } = voucher;
        Self {
            reference: reference(internal_ref.as_deref(), *id),
            amount: total.0.clone(),
            currency: currency.clone(),
            // No `_` arm: a status the vendor adds is a compile error here,
            // where someone has to decide whether it counts as booked.
            booked: match status {
                VoucherStatus::Draft => false,
                VoucherStatus::Open | VoucherStatus::Paid => true,
            },
        }
    }
}

fn reference(internal_ref: Option<&str>, id: Option<i64>) -> String {
    match (internal_ref, id) {
        (Some(reference), _) => reference.to_owned(),
        (None, Some(id)) => format!("V{id}"),
        (None, None) => "unbooked".to_owned(),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    fn voucher(status: VoucherStatus, internal_ref: Option<&str>, id: Option<i64>) -> Voucher {
        Voucher {
            id,
            total: "12.50".parse().expect("a valid amount"),
            currency: "EUR".parse().expect("a currency code"),
            status,
            internal_ref: internal_ref.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn the_vendors_undocumented_reference_wins_when_there_is_one() {
        let posting = Posting::of(&voucher(VoucherStatus::Open, Some("AB-7"), Some(5)));
        assert_eq!(posting.reference, "AB-7");
        assert!(posting.booked);
    }

    #[test]
    fn a_voucher_without_one_is_referenced_by_id() {
        let posting = Posting::of(&voucher(VoucherStatus::Paid, None, Some(5)));
        assert_eq!(posting.reference, "V5");
    }

    #[test]
    fn a_draft_the_server_has_never_seen_is_neither_booked_nor_referenced() {
        let posting = Posting::of(&voucher(VoucherStatus::Draft, None, None));
        assert_eq!(posting.reference, "unbooked");
        assert!(!posting.booked);
    }
}
