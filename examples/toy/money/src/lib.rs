//! An amount of money, as fixed point over an arbitrary-precision count of
//! cents.
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
//! `serde`, `thiserror` and `num-bigint`, and no way for an edit here to
//! recompile anything it does not have to. The bignum stops here — the library
//! has no business knowing what an adopter's own type is made of, and
//! `just bigint-free` is what says so rather than promising it.
//!
//! The rule stays the document's. Nothing in this crate reads the document —
//! `examples/toy/api/tests/money.rs` reads the `pattern` out of the embedded
//! document and holds [`Money::from_str`] to it value for value, so the two
//! cannot come apart without a named failure.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, Neg, Sub};
use std::str::FromStr;

use num_bigint::{BigUint, Sign};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Minor units in one major unit. An amount is fixed point at **two** decimal
/// places, and that 2 is not this crate's choice: it is what the `Money`
/// schema's `pattern` admits, which is why `api/tests/money.rs` holds the two
/// together.
const MINOR_PER_MAJOR: u32 = 100;

/// The integer an amount is counted in, for a caller who wants the number
/// [`Money::minor_units`] hands back.
pub use num_bigint::BigInt;

/// An amount of money, held as a whole number of minor units — cents.
///
/// The count is an arbitrary-precision **integer**, not a decimal. The scale is
/// fixed at two places by the document's rule and lives in this type's
/// arithmetic rather than travelling with each value, which is what makes that
/// arithmetic both exact and total: cents are integers, so [`Add`] and [`Sub`]
/// are integer addition with no rounding anywhere in them, and a [`BigInt`]
/// does not overflow, so there is no sum to refuse and no panic path to route
/// around.
///
/// # [`Display`](fmt::Display) and [`FromStr`] are not inverses
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
/// assert_eq!("12.50".parse(), Ok(amount.clone())); // the form the document states
/// assert_eq!(amount.wire(), "12.50"); // the form it goes back as
/// assert_eq!(amount.to_string(), "12,50"); // the form a person reads
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(BigInt);

/// A string that is not an amount.
///
/// One failure, so one type rather than an enum: an arbitrary-precision count
/// of cents holds every value the document's `pattern` admits, and the only way
/// left to fail is to hand this type something that is not an amount at all.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("`{raw}` is not an amount: digits, then at most two decimals after a `.`")]
pub struct MoneyError {
    /// What was handed to [`Money::from_str`].
    pub raw: String,
}

impl Money {
    /// No money at all — the identity of [`Add`], and what an empty column of
    /// amounts sums to.
    #[must_use]
    pub const fn zero() -> Self {
        Self(BigInt::ZERO)
    }

    /// An amount from a count of minor units. `Money::from_minor_units(1250)`
    /// is twelve fifty.
    #[must_use]
    pub fn from_minor_units(units: impl Into<BigInt>) -> Self {
        Self(units.into())
    }

    /// This amount as a count of minor units, which is what it is.
    ///
    /// The way out for arithmetic this type does not offer: proportions,
    /// rounding rules and tax splits are the adopter's, and they all start
    /// here.
    #[must_use]
    pub const fn minor_units(&self) -> &BigInt {
        &self.0
    }

    /// This amount in the form the document's `pattern` accepts: a dot, and
    /// always two decimals.
    ///
    /// What [`Serialize`] writes, and what to send anywhere the reader is a
    /// machine. [`Display`](fmt::Display) is the other form and is not this
    /// one.
    #[must_use]
    pub fn wire(&self) -> String {
        self.rendered('.')
    }

    /// This amount written out with `point` between the units, two decimals
    /// either way.
    ///
    /// The one rendering, so the form a person reads and the form the wire
    /// takes differ in exactly the character that is meant to differ. The sign
    /// is taken off the magnitude and put back in front by hand, because an
    /// amount smaller than one major unit has a zero where the sign would
    /// otherwise ride: minus five cents is `-0,05`, never `0,-05`.
    fn rendered(&self, point: char) -> String {
        let sign = if self.0.sign() == Sign::Minus {
            "-"
        } else {
            ""
        };
        let per_major = BigUint::from(MINOR_PER_MAJOR);
        let units = self.0.magnitude();
        let (major, minor) = (units / &per_major, units % &per_major);
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
        let (sign, digits) = match raw.strip_prefix('-') {
            Some(rest) => (Sign::Minus, rest),
            None => (Sign::Plus, raw),
        };
        let units = minor_units(digits).ok_or_else(|| MoneyError {
            raw: raw.to_owned(),
        })?;
        // `from_biguint` normalises a zero magnitude to no sign at all, so
        // `-0.00` is zero rather than a negative nothing.
        Ok(Self(BigInt::from_biguint(sign, units)))
    }
}

impl fmt::Display for Money {
    /// For a person: a comma, and always two decimals. Not the wire form — see
    /// [`Money::wire`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.rendered(','))
    }
}

impl Add for Money {
    type Output = Self;

    fn add(self, addend: Self) -> Self {
        Self(self.0 + addend.0)
    }
}

impl Sub for Money {
    type Output = Self;

    fn sub(self, subtrahend: Self) -> Self {
        Self(self.0 - subtrahend.0)
    }
}

impl Neg for Money {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Self>>(amounts: I) -> Self {
        amounts.fold(Self::zero(), Add::add)
    }
}

impl<'a> Sum<&'a Money> for Money {
    /// The shape a ledger actually has: a column of amounts read out of a
    /// collection nobody wants to consume.
    fn sum<I: Iterator<Item = &'a Self>>(amounts: I) -> Self {
        amounts.cloned().sum()
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

/// What an unsigned run of digits is worth in minor units, or `None` when it is
/// not an amount at all.
///
/// One decision — is this an amount, and how much — because the shape and the
/// value are read off the same characters with nothing in between.
fn minor_units(digits: &str) -> Option<BigUint> {
    let (major, fraction) = match digits.split_once('.') {
        Some((major, fraction)) => (major, Some(fraction)),
        None => (digits, None),
    };
    if major.is_empty() || !major.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let minor = match fraction {
        Some(fraction) => scaled(fraction.as_bytes())?,
        None => 0,
    };
    // `major` is a non-empty run of ASCII digits, which is exactly what
    // `BigUint`'s parser takes and the only thing it takes.
    let major = major.parse::<BigUint>().ok()?;
    Some(major * MINOR_PER_MAJOR + minor)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn money(raw: &str) -> Money {
        raw.parse()
            .unwrap_or_else(|error| panic!("`{raw}` should be an amount: {error}"))
    }

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
            let amount = money(raw);
            assert_eq!(amount, Money::from_minor_units(units), "{raw}");
            assert_eq!(amount.to_string(), shown, "{raw}");
            assert_eq!(format!("{amount}"), shown, "{raw}");
        }

        for raw in [
            "12,50", "12.505", "12.", "", ".5", "+12.50", " 12.50", "1e3",
        ] {
            assert_eq!(
                raw.parse::<Money>(),
                Err(MoneyError {
                    raw: raw.to_owned()
                }),
                "`{raw}` is not an amount"
            );
        }
    }

    /// The footgun, pinned so that nobody quietly closes it: the two directions
    /// answer different questions and do not compose.
    #[test]
    fn display_is_not_the_wire_form_and_does_not_parse_back() {
        let amount = money("12.50");
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
            assert_eq!(money(raw).wire(), "12.50");
        }
        assert_eq!(Money::zero().wire(), "0.00");
    }

    /// An amount smaller than one major unit has a zero where the sign would
    /// otherwise ride, and the sign has to survive that in both renderings.
    #[test]
    fn a_negative_amount_under_one_unit_keeps_its_sign_in_front() {
        let amount = money("-0.05");
        assert_eq!(amount, Money::from_minor_units(-5));
        assert_eq!(amount.to_string(), "-0,05");
        assert_eq!(amount.wire(), "-0.05");
        assert_eq!(money("-0.5").wire(), "-0.50");
        // A negative nothing is not a thing, in either rendering.
        assert_eq!(money("-0.00"), Money::zero());
        assert_eq!(money("-0.00").to_string(), "0,00");
        assert_eq!(money("-0.00").wire(), "0.00");
    }

    /// Why the count is arbitrary precision: an amount past every fixed-width
    /// integer is read, added and printed like any other, and the document's
    /// `pattern` never said there was a ceiling.
    #[test]
    fn an_amount_past_every_fixed_width_integer_is_an_amount_like_any_other() {
        // Twenty-six digits of major units: more than a `u64` of cents by some
        // seven orders of magnitude.
        let huge = money("99999999999999999999999999.99");
        assert_eq!(huge.wire(), "99999999999999999999999999.99");
        assert_eq!(huge.to_string(), "99999999999999999999999999,99");
        assert_eq!(
            (huge.clone() + money("0.01")).wire(),
            "100000000000000000000000000.00"
        );
        assert_eq!(huge.clone() - huge, Money::zero());
    }

    /// Arithmetic is why the type exists: cents are integers, so a column of
    /// amounts adds up exactly, and a `BigInt` does not overflow, so there is
    /// nothing to refuse and nothing for a caller to unwrap.
    #[test]
    fn amounts_add_and_subtract_exactly_and_totally() {
        let (ten_ten, twenty) = (money("10.10"), money("20.00"));
        assert_eq!(ten_ten.clone() + ten_ten.clone(), money("20.20"));
        assert_eq!(twenty - ten_ten.clone(), money("9.90"));
        assert_eq!(ten_ten.clone() + Money::zero(), ten_ten);
        assert_eq!(-money("3.07"), money("-3.07"));
        assert_eq!(money("1.00") - money("3.07"), money("-2.07"));

        // The tenth of the sum a binary float would lose.
        let column = vec![money("0.10"); 10];
        assert_eq!(column.iter().sum::<Money>(), money("1.00"));
        assert_eq!(column.into_iter().sum::<Money>(), money("1.00"));
        assert_eq!(std::iter::empty::<Money>().sum::<Money>(), Money::zero());
    }

    /// Ordering is the count of cents, so amounts sort and compare the way the
    /// numbers they stand for do.
    #[test]
    fn amounts_order_by_what_they_are_worth() {
        let mut amounts: Vec<Money> = ["1.00", "-3.07", "0.5", "12.50"]
            .iter()
            .map(|raw| money(raw))
            .collect();
        amounts.sort();
        let sorted: Vec<String> = amounts.iter().map(Money::wire).collect();
        assert_eq!(sorted, ["-3.07", "0.50", "1.00", "12.50"]);
    }

    #[test]
    fn an_amount_is_a_string_on_the_wire_and_never_a_number() {
        let amount = money("-3.07");
        assert_eq!(serde_json::to_value(&amount).ok(), Some("-3.07".into()));
        assert_eq!(
            serde_json::from_value::<Money>(serde_json::json!("12.5")).ok(),
            Some(Money::from_minor_units(1250))
        );
        // A double has already lost whatever it was.
        assert!(serde_json::from_value::<Money>(serde_json::json!(3.07)).is_err());
        assert!(serde_json::from_value::<Money>(serde_json::json!("3.071")).is_err());
    }
}
