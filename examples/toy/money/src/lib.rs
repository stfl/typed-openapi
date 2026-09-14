//! An amount of money, as fixed point over a whole number of cents.
//!
//! # Why this is not a generated type
//!
//! OpenAPI cannot describe a fixed-point decimal. `type: number` is an IEEE 754
//! double in every JSON parser that matters, so `12.10` is not `12.10` and a
//! column of them does not add up; `multipleOf: 0.01` cannot rescue it, because
//! `0.01` has no binary representation either. `type: string` with a `pattern`
//! says what an amount *looks like* and nothing about arithmetic on it — which
//! is the right thing for a document to say, and not enough to book a ledger
//! with.
//!
//! So the wire form is expressible and the type is not, and that gap is what
//! `Settings::replace` exists for. The document tags the shape with
//! `format: money` and states the lexical rule as `pattern`; the bless step is
//! told that `money::Money` stands for that format. Neither half means anything
//! alone, and the two compose: the command line still enforces the document's
//! rule, and Rust gets a type that can add.
//!
//! # Why this is a crate
//!
//! The generated code names [`Money`], so the crate that owns it has to sit
//! *below* the generated one. That is the whole of the arrangement: one type,
//! `serde` and `thiserror`, and no way for an edit here to recompile anything
//! it does not have to.
//!
//! The rule stays the document's. Nothing in this crate reads the document —
//! `examples/toy/api/tests/money.rs` reads the `pattern` out of the embedded
//! document and holds [`Money::from_str`] to it value for value, so the two
//! cannot come apart without a named failure.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Minor units in one major unit. An amount is fixed point at two decimal
/// places, which is what the document's `pattern` admits and what the currencies
/// this adoption sees are counted in.
const MINOR_PER_MAJOR: u64 = 100;

/// An amount of money, held as a whole number of minor units — cents.
///
/// Arithmetic is exact, and it is the reason the type exists: cents are
/// integers, so [`checked_add`](Money::checked_add) and
/// [`checked_sub`](Money::checked_sub) are `i64` addition with no rounding
/// anywhere in them. They answer `None` rather than wrapping, because an amount
/// that silently becomes its own negative is worse than an amount that is
/// missing. There is no [`Add`](std::ops::Add) impl for the same reason: the
/// operator has nowhere to put the refusal.
///
/// # `Display` and `FromStr` are not inverses
///
/// **This is deliberate, and it is the whole point of the type.** The two
/// directions answer different questions:
///
/// - [`FromStr`] reads what the document says an amount is: `12.50`, a **dot**,
///   one or two decimals — the `Money` schema's own `pattern`. A comma is
///   refused, because the document refuses it.
/// - [`Display`](fmt::Display) writes what a reader of this adoption expects to
///   see: `12,50`, a **comma**, always two decimals. That is a rendering
///   choice, and no OpenAPI document can state it.
///
/// So `amount.to_string().parse()` does not round-trip, and nothing here
/// pretends it does. The wire form has a name of its own — [`Money::wire`] —
/// and that is what [`Serialize`] writes; reach for it rather than for
/// `to_string` whenever the bytes are going somewhere other than a person.
///
/// ```
/// # use money::Money;
/// let amount = Money::from_minor_units(1250);
/// assert_eq!("12.50".parse(), Ok(amount)); // the form the document states
/// assert_eq!(amount.wire(), "12.50"); // the form it goes back as
/// assert_eq!(amount.to_string(), "12,50"); // the form a person reads
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(i64);

/// A string that is not an amount this type can hold.
///
/// Two variants because the document's `pattern` tells them apart: one is a
/// string the pattern refuses too, the other is a string the pattern admits and
/// no fixed-width integer can — an unbounded run of digits is a language
/// [`Money`] is a strict subset of.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MoneyError {
    /// Not an amount at all: digits, then at most two decimals after a `.`.
    #[error("`{0}` is not an amount: digits, then at most two decimals after a `.`")]
    Malformed(String),
    /// Shaped like an amount, and larger than `i64` cents.
    #[error("`{0}` is more money than a 64-bit count of cents holds")]
    TooLarge(String),
}

impl Money {
    /// No money at all — the identity of [`checked_add`](Money::checked_add).
    #[must_use]
    pub const fn zero() -> Self {
        Self(0)
    }

    /// An amount from a count of minor units. `Money::from_minor_units(1250)`
    /// is twelve fifty.
    #[must_use]
    pub const fn from_minor_units(units: i64) -> Self {
        Self(units)
    }

    /// This amount as a count of minor units, which is what it is.
    ///
    /// The way out for arithmetic this type does not offer: proportions,
    /// rounding rules and tax splits are the adopter's, and they all start
    /// here.
    #[must_use]
    pub const fn minor_units(self) -> i64 {
        self.0
    }

    /// This amount in the form the document's `pattern` accepts: a dot, and
    /// always two decimals.
    ///
    /// What [`Serialize`] writes, and what to send anywhere the reader is a
    /// machine. [`Display`](fmt::Display) is the other form and is not this
    /// one.
    #[must_use]
    pub fn wire(self) -> String {
        self.rendered('.')
    }

    /// The sum, or `None` if it is more money than an `i64` of cents holds.
    #[must_use]
    pub fn checked_add(self, addend: Self) -> Option<Self> {
        self.0.checked_add(addend.0).map(Self)
    }

    /// The difference, or `None` if it is further from zero than an `i64` of
    /// cents reaches.
    #[must_use]
    pub fn checked_sub(self, subtrahend: Self) -> Option<Self> {
        self.0.checked_sub(subtrahend.0).map(Self)
    }

    /// This amount written out with `point` between the units, two decimals
    /// either way.
    ///
    /// The one rendering, so the form a person reads and the form the wire
    /// takes differ in exactly the character that is meant to differ.
    /// `unsigned_abs` rather than `abs` because the most negative `i64` has no
    /// positive counterpart and a panic is not an answer.
    fn rendered(self, point: char) -> String {
        let sign = if self.0.is_negative() { "-" } else { "" };
        let units = self.0.unsigned_abs();
        let (major, minor) = (units / MINOR_PER_MAJOR, units % MINOR_PER_MAJOR);
        format!("{sign}{major}{point}{minor:02}")
    }
}

impl FromStr for Money {
    type Err = MoneyError;

    /// Read the form the document states: an optional `-`, digits, and at most
    /// two decimals after a `.`.
    ///
    /// One decimal place is a whole amount — `12.5` is twelve fifty — because
    /// the document's `pattern` admits it.
    fn from_str(raw: &str) -> Result<Self, MoneyError> {
        let malformed = || MoneyError::Malformed(raw.to_owned());
        let (negative, unsigned) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let (major, minor) = split(unsigned).ok_or_else(malformed)?;
        let units = magnitude(major, minor).ok_or_else(|| MoneyError::TooLarge(raw.to_owned()))?;
        Ok(Self(if negative { -units } else { units }))
    }
}

impl fmt::Display for Money {
    /// For a person: a comma, and always two decimals. Not the wire form — see
    /// [`Money::wire`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.rendered(','))
    }
}

impl Serialize for Money {
    /// The wire form, as the string the document says an amount is.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.wire())
    }
}

impl<'de> Deserialize<'de> for Money {
    /// The wire form, held to the same rule a command line's `--total` is. A
    /// JSON number is refused: an amount the vendor sent as a double has
    /// already lost whatever it was.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// The digits of an unsigned amount: everything before the point, and what
/// follows it as minor units. `None` is a string that is not an amount.
fn split(unsigned: &str) -> Option<(&str, u8)> {
    let (major, fraction) = match unsigned.split_once('.') {
        Some((major, fraction)) => (major, Some(fraction)),
        None => (unsigned, None),
    };
    if major.is_empty() || !major.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let minor = match fraction {
        Some(fraction) => scaled(fraction.as_bytes())?,
        None => 0,
    };
    Some((major, minor))
}

/// The digits after the point as minor units: one digit is tenths, two is
/// exact, and anything else — no digits, three digits, a second point — is more
/// than this scale carries.
fn scaled(fraction: &[u8]) -> Option<u8> {
    match *fraction {
        [tenths] => Some(digit(tenths)? * 10),
        [tenths, hundredths] => Some(digit(tenths)? * 10 + digit(hundredths)?),
        _ => None,
    }
}

/// One ASCII digit as the number it spells.
fn digit(byte: u8) -> Option<u8> {
    byte.is_ascii_digit().then(|| byte - b'0')
}

/// The magnitude of an amount in minor units, or `None` when it is more than an
/// `i64` of them holds.
///
/// Counted unsigned and narrowed at the end, so the sign is applied to a value
/// that is already known to fit.
fn magnitude(major: &str, minor: u8) -> Option<i64> {
    let units = major
        .parse::<u64>()
        .ok()?
        .checked_mul(MINOR_PER_MAJOR)?
        .checked_add(u64::from(minor))?;
    i64::try_from(units).ok()
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "a test that cannot build its fixture should fail loudly"
)]
mod tests {
    use super::*;

    /// The contract, as a table. `api/tests/money.rs` holds the left column to
    /// the document's own `pattern`; this holds the rest to what the type
    /// promises.
    #[test]
    fn the_documents_form_is_read_and_a_readers_form_is_written() {
        // raw, what it is worth in minor units, how a person reads it back
        let accepted = [
            ("12.50", 1250, "12,50"),
            ("12.5", 1250, "12,50"),
            ("12", 1200, "12,00"),
            ("0", 0, "0,00"),
            ("0.07", 7, "0,07"),
            ("-3.07", -307, "-3,07"),
            ("-0.5", -50, "-0,50"),
            ("007.10", 710, "7,10"),
        ];
        for (raw, units, shown) in accepted {
            let amount: Money = raw.parse().unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!(amount.minor_units(), units, "{raw}");
            assert_eq!(amount.to_string(), shown, "{raw}");
            assert_eq!(format!("{amount}"), shown, "{raw}");
        }

        for raw in [
            "12,50", "12.505", "12.", "", ".5", "+12.50", " 12.50", "1e3",
        ] {
            assert!(raw.parse::<Money>().is_err(), "`{raw}` is not an amount");
        }
    }

    /// The footgun, pinned so that nobody quietly closes it: the two directions
    /// answer different questions and do not compose.
    #[test]
    fn display_is_not_the_wire_form_and_does_not_parse_back() {
        let amount: Money = "12.50".parse().unwrap();
        assert_eq!(amount.wire(), "12.50");
        assert_eq!(amount.to_string(), "12,50");
        assert!(amount.to_string().parse::<Money>().is_err());
        assert_eq!(amount.wire().parse::<Money>(), Ok(amount));
    }

    /// The wire form is always two decimals, whatever it was read from, because
    /// the document's `pattern` accepts both spellings and one of them is
    /// canonical.
    #[test]
    fn the_wire_form_is_canonical() {
        for raw in ["12.5", "12.50"] {
            assert_eq!(raw.parse::<Money>().unwrap().wire(), "12.50");
        }
        assert_eq!("-0.5".parse::<Money>().unwrap().wire(), "-0.50");
        assert_eq!(Money::zero().wire(), "0.00");
    }

    #[test]
    fn a_string_the_shape_admits_and_an_i64_of_cents_does_not_is_refused_as_such() {
        let huge = "92233720368547758.08";
        assert_eq!(
            huge.parse::<Money>(),
            Err(MoneyError::TooLarge(huge.to_owned()))
        );
        assert_eq!(
            "12,50".parse::<Money>(),
            Err(MoneyError::Malformed("12,50".to_owned()))
        );
        // One cent below the boundary is an amount.
        assert_eq!(
            "92233720368547758.07"
                .parse::<Money>()
                .map(Money::minor_units),
            Ok(i64::MAX)
        );
    }

    /// Arithmetic is why the type exists: cents are integers, so a column of
    /// amounts adds up exactly, and a sum that will not fit is `None` rather
    /// than a number pointing the wrong way.
    #[test]
    fn amounts_add_and_subtract_exactly_or_not_at_all() {
        let (ten_ten, twenty) = (Money::from_minor_units(1010), Money::from_minor_units(2000));
        assert_eq!(
            ten_ten.checked_add(ten_ten),
            Some(Money::from_minor_units(2020))
        );
        assert_eq!(
            twenty.checked_sub(ten_ten),
            Some(Money::from_minor_units(990))
        );
        assert_eq!(ten_ten.checked_add(Money::zero()), Some(ten_ten));

        // The tenth of the sum that a binary float would lose.
        let dime: Money = "0.10".parse().unwrap();
        let sum = std::iter::repeat_n(dime, 10)
            .try_fold(Money::zero(), Money::checked_add)
            .unwrap();
        assert_eq!(sum, "1.00".parse::<Money>().unwrap());

        assert_eq!(Money::from_minor_units(i64::MAX).checked_add(dime), None);
        assert_eq!(Money::from_minor_units(i64::MIN).checked_sub(dime), None);
    }

    /// Ordering is the count of cents, so amounts sort and compare the way the
    /// numbers they stand for do.
    #[test]
    fn amounts_order_by_what_they_are_worth() {
        let mut amounts: Vec<Money> = ["1.00", "-3.07", "0.5", "12.50"]
            .iter()
            .map(|raw| raw.parse().unwrap())
            .collect();
        amounts.sort_unstable();
        let sorted: Vec<String> = amounts.iter().map(|a| a.wire()).collect();
        assert_eq!(sorted, ["-3.07", "0.50", "1.00", "12.50"]);
    }

    #[test]
    fn an_amount_is_a_string_on_the_wire_and_never_a_number() {
        let amount: Money = "-3.07".parse().unwrap();
        assert_eq!(serde_json::to_value(amount).ok(), Some("-3.07".into()));
        assert_eq!(
            serde_json::from_value::<Money>(serde_json::json!("12.5")).ok(),
            Some(Money::from_minor_units(1250))
        );
        // A double has already lost whatever it was.
        assert!(serde_json::from_value::<Money>(serde_json::json!(3.07)).is_err());
        assert!(serde_json::from_value::<Money>(serde_json::json!("3.071")).is_err());
    }
}
