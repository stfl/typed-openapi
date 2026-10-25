//! The value kinds a command-line flag can carry, the rules the document states
//! about them, and how a raw argument string becomes the JSON the wire wants.
//!
//! A [`Scalar`] is the only thing this crate knows about a schema: everything
//! richer than these five kinds is a whole-body affair and never reaches a flag.
//! What one carries is what the document said — a `pattern`, a length, a bound,
//! a step — and every one of those is enforced by [`Scalar::parse`], because a
//! rule that only reaches `--help` is a rule the user finds out about from the
//! server.
//!
//! `pattern` runs on `regress`, the ECMA-262 engine typify puts inside a
//! generated newtype's `FromStr`. One engine, one rule, one set of bytes: a
//! value the command line accepts is a value the generated type accepts by
//! construction, rather than by a hand-written rule kept in step.

use std::fmt;

use regress::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What one flag accepts, as the document describes it.
///
/// [`Eq`] is absent on purpose: a `number`'s bounds are `f64`, which has none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Scalar {
    /// `type: string`, with whatever the document states about it.
    Text(Text),
    /// `type: integer`, with whatever the document states about it.
    Integer(Bounds<i64>),
    /// `type: number`, with whatever the document states about it.
    Number(Bounds<f64>),
    /// `type: boolean`.
    Boolean,
    /// `enum: [..]` on a string schema. These values complete.
    Choice(Vec<String>),
}

/// What a string schema states beyond being a string.
///
/// The fields are public because there is nothing here to keep true: each one
/// is a JSON Schema keyword read straight off the document, and `None` is the
/// document saying nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    /// `pattern`, as ECMA-262 spells it.
    pub pattern: Option<String>,
    /// `minLength`, counted in characters.
    pub min_length: Option<usize>,
    /// `maxLength`, counted in characters.
    pub max_length: Option<usize>,
}

/// What a numeric schema states beyond being a number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Bounds<T> {
    /// `minimum`, with `exclusiveMinimum` folded into it.
    pub low: Option<Limit<T>>,
    /// `maximum`, with `exclusiveMaximum` folded into it.
    pub high: Option<Limit<T>>,
    /// `multipleOf`.
    pub multiple_of: Option<T>,
}

/// One end of a range.
///
/// OpenAPI 3.0 spells exclusivity as a flag beside the number rather than as a
/// number of its own, which leaves `exclusiveMinimum: true` with no `minimum`
/// both sayable and meaningless. Folded into one value it cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Limit<T> {
    /// The value itself is allowed.
    Inclusive(T),
    /// The value itself is not.
    Exclusive(T),
}

/// A raw argument the document's schema rejects.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ScalarError {
    /// Not the kind of value the document asks for at all.
    #[error("`{raw}` is not {wanted}")]
    Kind {
        /// The argument as the operator typed it.
        raw: String,
        /// The kind of value the schema asks for, worded to finish the
        /// sentence: `an integer`, `a number`, or `` `true` or `false` ``.
        wanted: &'static str,
    },
    /// Outside the document's `enum` — the one refusal that can say what to
    /// type instead, which is why it carries the list rather than a sentence.
    #[error("`{raw}` is not one of {}", .allowed.join(", "))]
    Choice {
        /// The argument as the operator typed it.
        raw: String,
        /// The values the document's `enum` lists, in its order, spelled as
        /// the operator types them.
        allowed: Vec<String>,
    },
    /// The right kind of value, refused by a rule the document states. `rule`
    /// finishes the sentence and carries the document's own number, which is
    /// the only part a user can act on; nothing branches on which keyword it
    /// came from.
    #[error("`{raw}` {rule}")]
    Rule {
        /// The argument as the operator typed it.
        raw: String,
        /// The rest of the sentence: what the value broke, with the document's
        /// own number in it.
        rule: String,
    },
    /// A `pattern` the engine cannot read. Every value is refused, because a
    /// rule nobody can run is not a rule everything passed — and a document
    /// stating one is refused outright while it is reduced.
    #[error("`{pattern}` is not a regular expression: {message}")]
    Pattern {
        /// The `pattern` as the document spells it.
        pattern: String,
        /// The reason the regular-expression engine gave.
        message: String,
    },
}

impl ScalarError {
    /// `raw`, refused by a rule that finishes the sentence.
    fn rule(raw: &str, rule: String) -> Self {
        Self::Rule {
            raw: raw.to_owned(),
            rule,
        }
    }

    /// `raw`, which is not the kind of value `wanted` finishes the sentence for.
    fn kind(raw: &str, wanted: &'static str) -> Self {
        Self::Kind {
            raw: raw.to_owned(),
            wanted,
        }
    }
}

impl Scalar {
    /// Turn one raw argument into the JSON value the wire wants, or refuse it
    /// the way the document does.
    pub fn parse(&self, raw: &str) -> Result<serde_json::Value, ScalarError> {
        match self {
            Self::Text(text) => text.check(raw).map(|()| raw.into()),
            Self::Integer(bounds) => bounded(bounds, raw).map(|(_, json)| json),
            Self::Number(bounds) => bounded(bounds, raw).map(|(_, json)| json),
            Self::Boolean => raw
                .parse::<bool>()
                .map(Into::into)
                .map_err(|_| ScalarError::kind(raw, "`true` or `false`")),
            Self::Choice(allowed) => {
                if allowed.iter().any(|value| value == raw) {
                    Ok(raw.into())
                } else {
                    Err(ScalarError::Choice {
                        raw: raw.to_owned(),
                        allowed: allowed.clone(),
                    })
                }
            }
        }
    }

    /// The text of one parameter's value as the request carries it: the value
    /// that [`Scalar::parse`] reads out of `raw`, written the way the typed call
    /// writes it.
    ///
    /// The number checked is the number sent. `+5` and `007` go out as `5` and
    /// `7`, `1e3` as `1000`, and a number a typed caller wrote with `ToString` is
    /// sent as it came. Every rule of [`Scalar::parse`] runs first, and it is the
    /// only place one does: a refusal here is the refusal there. A value that is
    /// not a number goes out as it was given.
    pub fn wire(&self, raw: &str) -> Result<String, ScalarError> {
        match self {
            Self::Integer(bounds) => bounded(bounds, raw).map(|(value, _)| value.to_string()),
            Self::Number(bounds) => bounded(bounds, raw).map(|(value, _)| value.to_string()),
            Self::Text(_) | Self::Boolean | Self::Choice(_) => {
                self.parse(raw).map(|_| raw.to_owned())
            }
        }
    }

    /// Whether every rule this carries can be run at all.
    ///
    /// A `pattern` the engine cannot read is the only rule that can fail this
    /// way, and it fails on every value — so a document is held to this while
    /// it is reduced, and the failure names the operation rather than turning
    /// up at a user's flag.
    pub fn runnable(&self) -> Result<(), ScalarError> {
        match self {
            Self::Text(text) => text.pattern.as_deref().map(compiled).transpose().map(drop),
            Self::Integer(_) | Self::Number(_) | Self::Boolean | Self::Choice(_) => Ok(()),
        }
    }

    /// The `<NAME>` shown for the flag's value, and the closest thing the
    /// document gives an agent to a type.
    #[must_use]
    pub fn value_name(&self) -> &'static str {
        match self {
            Self::Text(_) | Self::Choice(_) => "STRING",
            Self::Integer(_) => "INT",
            Self::Number(_) => "NUMBER",
            Self::Boolean => "BOOL",
        }
    }

    /// Everything the document constrains about this value, for the flag's help
    /// line. Every note is a rule [`Scalar::parse`] enforces, in the document's
    /// own numbers — help and refusal cannot come apart, because one rendering
    /// serves both.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        let notes = match self {
            Self::Text(text) => text.notes(),
            Self::Integer(bounds) => bounds.notes(),
            Self::Number(bounds) => bounds.notes(),
            Self::Boolean | Self::Choice(_) => Vec::new(),
        };
        (!notes.is_empty()).then(|| notes.join("; "))
    }
}

impl Text {
    /// Hold one argument to every rule the document states about it.
    fn check(&self, raw: &str) -> Result<(), ScalarError> {
        // `minLength` and `maxLength` count characters, not bytes.
        let length = raw.chars().count();
        if let Some(least) = self.min_length
            && length < least
        {
            return Err(ScalarError::rule(
                raw,
                format!("is shorter than {least} characters"),
            ));
        }
        if let Some(most) = self.max_length
            && length > most
        {
            return Err(ScalarError::rule(
                raw,
                format!("is longer than {most} characters"),
            ));
        }
        let Some(pattern) = self.pattern.as_deref() else {
            return Ok(());
        };
        // JSON Schema's `pattern` is a search rather than a whole-string match,
        // so an unanchored pattern matches anywhere in the value. Anchoring it
        // here would refuse values the document allows.
        if compiled(pattern)?.find(raw).is_none() {
            return Err(ScalarError::rule(raw, format!("does not match {pattern}")));
        }
        Ok(())
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if let Some(least) = self.min_length {
            notes.push(format!("at least {least} characters"));
        }
        if let Some(most) = self.max_length {
            notes.push(format!("at most {most} characters"));
        }
        if let Some(pattern) = &self.pattern {
            notes.push(format!("matches {pattern}"));
        }
        notes
    }
}

impl<T> Bounds<T> {
    /// Hold one value to every rule the document states about it.
    fn check(&self, value: T, raw: &str) -> Result<(), ScalarError>
    where
        T: Numeric,
    {
        let refuse = |rule: String| ScalarError::rule(raw, format!("is not {rule}"));
        if let Some(low) = self.low {
            low.floor(value).map_err(refuse)?;
        }
        if let Some(high) = self.high {
            high.ceiling(value).map_err(refuse)?;
        }
        if let Some(step) = self.multiple_of
            && !value.divisible_by(step)
        {
            return Err(refuse(format!("a multiple of {step}")));
        }
        Ok(())
    }

    fn notes(&self) -> Vec<String>
    where
        T: Numeric,
    {
        let mut notes = Vec::new();
        if let Some(low) = self.low {
            notes.push(low.note("at least", "more than"));
        }
        if let Some(high) = self.high {
            notes.push(high.note("at most", "less than"));
        }
        if let Some(step) = self.multiple_of {
            notes.push(format!("a multiple of {step}"));
        }
        notes
    }
}

impl<T> Limit<T> {
    /// This limit as a lower bound. The `Err` is how the rule reads, and it is
    /// how `--help` reads it too — one rendering, so a refusal cannot describe
    /// a different rule from the one the help line advertised.
    fn floor(self, value: T) -> Result<(), String>
    where
        T: Numeric,
    {
        let admits = match self {
            Self::Inclusive(limit) => value >= limit,
            Self::Exclusive(limit) => value > limit,
        };
        if admits {
            Ok(())
        } else {
            Err(self.note("at least", "more than"))
        }
    }

    /// This limit as an upper bound.
    fn ceiling(self, value: T) -> Result<(), String>
    where
        T: Numeric,
    {
        let admits = match self {
            Self::Inclusive(limit) => value <= limit,
            Self::Exclusive(limit) => value < limit,
        };
        if admits {
            Ok(())
        } else {
            Err(self.note("at most", "less than"))
        }
    }

    fn note(self, inclusive: &str, exclusive: &str) -> String
    where
        T: Numeric,
    {
        match self {
            Self::Inclusive(limit) => format!("{inclusive} {limit}"),
            Self::Exclusive(limit) => format!("{exclusive} {limit}"),
        }
    }
}

/// One ECMA-262 pattern, ready to run.
fn compiled(pattern: &str) -> Result<Regex, ScalarError> {
    Regex::new(pattern).map_err(|error| ScalarError::Pattern {
        pattern: pattern.to_owned(),
        message: error.to_string(),
    })
}

/// One argument as a number the document's rules admit, and the JSON it goes
/// out as in a body.
fn bounded<T: Numeric>(
    bounds: &Bounds<T>,
    raw: &str,
) -> Result<(T, serde_json::Value), ScalarError> {
    let (value, json) = T::read(raw)?;
    bounds.check(value, raw)?;
    Ok((value, json))
}

/// Is `raw` an integer's digits, with a sign or without one?
fn is_whole(raw: &str) -> bool {
    let digits = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Is `raw` spelled with the characters of a decimal number only (digits, a
/// sign, a point and an exponent mark), whatever number it says: `1e400` is, and
/// `inf` is not.
fn has_only_number_characters(raw: &str) -> bool {
    raw.bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.' | b'e' | b'E'))
}

/// Is `value`, the float nearest to `raw`, the number that `raw` spells?
///
/// A float holds the nearest of its digits, and a request that sends `value`
/// sends another number than the one that was checked when the two differ:
/// `9007199254740993` is sent as `9007199254740992`, and `1e-400` as `0`. The
/// shortest text that reads back as `value` is the number the float stands for,
/// so the two are compared as decimal numbers, whatever the spelling: `1.0`,
/// `1e0` and `001` are one number.
fn exact(raw: &str, value: f64) -> bool {
    match (Decimal::of(raw), Decimal::of(&format!("{value:e}"))) {
        (Some(spelled), Some(printed)) => spelled == printed,
        (Some(_), None) | (None, _) => false,
    }
}

/// A decimal number, without its spelling: the digits from the first one that is
/// not zero to the last one that is not zero, and the power of ten they are
/// multiplied by. Zero has no digits and no sign.
#[derive(Debug, PartialEq, Eq)]
struct Decimal {
    negative: bool,
    digits: String,
    exponent: i64,
}

impl Decimal {
    /// `text` as a number, when it is spelled as `f64::from_str` reads one:
    /// a sign, digits with a point somewhere among them, and an exponent. Zero
    /// is zero whatever its exponent says, so `0e99999999999999999999` is read
    /// before the exponent is. Any other number whose exponent is past an `i64`
    /// has no reading.
    fn of(text: &str) -> Option<Self> {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (mantissa, power) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
        if !is_whole(power) {
            return None;
        }
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = format!("{whole}{fraction}");
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let significant = digits.trim_matches('0');
        if significant.is_empty() {
            return Some(Self {
                negative: false,
                digits: String::new(),
                exponent: 0,
            });
        }
        let exponent = power
            .parse::<i64>()
            .ok()?
            .checked_sub(i64::try_from(fraction.len()).ok()?)?;
        let trailing = digits.len() - digits.trim_end_matches('0').len();
        Some(Self {
            negative,
            digits: significant.to_owned(),
            exponent: exponent.checked_add(i64::try_from(trailing).ok()?)?,
        })
    }
}

/// The two number kinds a document can ask one flag for.
///
/// A trait rather than two copies of [`Bounds`] and its rules: the comparisons
/// are the same sentence in both, and only reading a value out of an argument
/// and dividing by one differ. It is crate-private: `i64` and `f64` are what
/// OpenAPI's `integer` and `number` are, so there is no third kind for a caller
/// to implement it for.
pub(crate) trait Numeric: Copy + PartialOrd + fmt::Display {
    /// What a refusal calls this kind.
    const KIND: &'static str;

    /// This kind read out of one argument, with the JSON it goes out as.
    ///
    /// One step, because three questions about `raw` can have three different
    /// answers: is it a number of this kind, does the type hold it, and does
    /// JSON carry it as it is. A float that parses but is not finite is not a
    /// number JSON carries, and "is not a number" is the honest answer rather
    /// than a `null` on the wire. A number that the type cannot hold is out of
    /// range, and a float that holds only the nearest of its digits would send
    /// another number than the one that was checked.
    fn read(raw: &str) -> Result<(Self, serde_json::Value), ScalarError>;

    /// `multipleOf`: whether dividing by `step` leaves a whole number.
    fn divisible_by(self, step: Self) -> bool;
}

impl Numeric for i64 {
    const KIND: &'static str = "an integer";

    fn read(raw: &str) -> Result<(Self, serde_json::Value), ScalarError> {
        raw.parse::<Self>()
            .map(|value| (value, value.into()))
            .map_err(|_| {
                // An integer that is too long for an `i64` is a number all the
                // same, and `typify` writes a `u64` for `minimum: 0`. A text
                // of digits, with or without a sign, that fails to parse can
                // only have overflowed. A text with a point or an exponent in it
                // is no integer however long it is.
                if is_whole(raw) {
                    ScalarError::rule(
                        raw,
                        format!(
                            "is out of range: an integer here is between {} and {}",
                            Self::MIN,
                            Self::MAX
                        ),
                    )
                } else {
                    ScalarError::kind(raw, Self::KIND)
                }
            })
    }

    fn divisible_by(self, step: Self) -> bool {
        step != 0 && self % step == 0
    }
}

impl Numeric for f64 {
    const KIND: &'static str = "a number";

    fn read(raw: &str) -> Result<(Self, serde_json::Value), ScalarError> {
        let kind = || ScalarError::kind(raw, Self::KIND);
        let value = raw.parse::<Self>().map_err(|_| kind())?;
        if value.is_infinite() && has_only_number_characters(raw) {
            return Err(ScalarError::rule(
                raw,
                format!(
                    "is out of range: a number here is between {:e} and {:e}",
                    Self::MIN,
                    Self::MAX
                ),
            ));
        }
        let json = serde_json::Number::from_f64(value).ok_or_else(kind)?;
        if !exact(raw, value) {
            return Err(ScalarError::rule(
                raw,
                format!(
                    "does not survive the round trip through a 64-bit float, which would send \
                     {value}"
                ),
            ));
        }
        Ok((value, json.into()))
    }

    // `multipleOf` is exact division by definition, so the comparison is the
    // document's rule rather than an approximation of it.
    fn divisible_by(self, step: Self) -> bool {
        step != 0.0 && (self / step).fract() == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(pattern: &str) -> Scalar {
        Scalar::Text(Text {
            pattern: Some(pattern.to_owned()),
            ..Text::default()
        })
    }

    /// Why `scalar` refuses `raw`.
    fn refusal(scalar: &Scalar, raw: &str) -> ScalarError {
        let Err(refused) = scalar.parse(raw) else {
            panic!("`{raw}` was accepted");
        };
        refused
    }

    #[test]
    fn a_pattern_is_enforced_and_reads_back_in_the_documents_own_spelling() {
        let amount = text(r"^-?[0-9]+(\.[0-9]{1,2})?$");
        for raw in ["12.50", "0", "0.0", "-3.07", "1000000.00"] {
            assert_eq!(amount.parse(raw), Ok(raw.into()), "{raw}");
        }
        assert_eq!(
            amount.parse("1,50"),
            Err(ScalarError::Rule {
                raw: "1,50".to_owned(),
                rule: r"does not match ^-?[0-9]+(\.[0-9]{1,2})?$".to_owned(),
            })
        );
        for raw in ["12.5x", "", "12.505", ".5", "-", "1e3"] {
            assert!(amount.parse(raw).is_err(), "{raw}");
        }
    }

    /// JSON Schema's `pattern` is a search. Anchoring it here would refuse
    /// values the document allows, and a document that wants anchoring says so.
    #[test]
    fn an_unanchored_pattern_matches_anywhere_in_the_value() {
        let digits = text("[0-9]+");
        assert!(digits.parse("ab12cd").is_ok());
        assert!(digits.parse("abcd").is_err());
        assert!(text("^[A-Z]+$").parse("xyZ").is_err());
    }

    /// A rule nobody can run is not a rule everything passed.
    #[test]
    fn a_pattern_the_engine_cannot_read_refuses_every_value() {
        let broken = text("[unterminated");
        assert!(matches!(
            broken.runnable(),
            Err(ScalarError::Pattern { .. })
        ));
        assert!(matches!(
            broken.parse("anything"),
            Err(ScalarError::Pattern { .. })
        ));
        assert_eq!(text("^ok$").runnable(), Ok(()));
    }

    #[test]
    fn lengths_are_counted_in_characters_and_refused_with_the_documents_number() {
        let code = Scalar::Text(Text {
            min_length: Some(3),
            max_length: Some(3),
            ..Text::default()
        });
        assert!(code.parse("EUR").is_ok());
        // Three characters, nine bytes.
        assert!(code.parse("€€€").is_ok());
        assert_eq!(
            code.parse("EU"),
            Err(ScalarError::Rule {
                raw: "EU".to_owned(),
                rule: "is shorter than 3 characters".to_owned(),
            })
        );
        assert_eq!(
            code.parse("EURO"),
            Err(ScalarError::Rule {
                raw: "EURO".to_owned(),
                rule: "is longer than 3 characters".to_owned(),
            })
        );
    }

    #[test]
    fn an_inclusive_bound_admits_its_own_value_and_an_exclusive_one_does_not() {
        let inclusive = Scalar::Integer(Bounds {
            low: Some(Limit::Inclusive(1)),
            high: Some(Limit::Inclusive(100)),
            multiple_of: None,
        });
        assert!(inclusive.parse("1").is_ok());
        assert!(inclusive.parse("100").is_ok());
        assert_eq!(
            inclusive.parse("0"),
            Err(ScalarError::Rule {
                raw: "0".to_owned(),
                rule: "is not at least 1".to_owned(),
            })
        );
        assert_eq!(
            inclusive.parse("101"),
            Err(ScalarError::Rule {
                raw: "101".to_owned(),
                rule: "is not at most 100".to_owned(),
            })
        );

        let exclusive = Scalar::Number(Bounds {
            low: Some(Limit::Exclusive(0.0)),
            high: Some(Limit::Exclusive(1.0)),
            multiple_of: None,
        });
        assert!(exclusive.parse("0.5").is_ok());
        assert_eq!(
            exclusive.parse("0"),
            Err(ScalarError::Rule {
                raw: "0".to_owned(),
                rule: "is not more than 0".to_owned(),
            })
        );
        assert_eq!(
            exclusive.parse("1"),
            Err(ScalarError::Rule {
                raw: "1".to_owned(),
                rule: "is not less than 1".to_owned(),
            })
        );
    }

    #[test]
    fn a_step_is_enforced_for_both_number_kinds() {
        let by_five = Scalar::Integer(Bounds {
            multiple_of: Some(5),
            ..Bounds::default()
        });
        assert!(by_five.parse("15").is_ok());
        assert_eq!(
            by_five.parse("7"),
            Err(ScalarError::Rule {
                raw: "7".to_owned(),
                rule: "is not a multiple of 5".to_owned(),
            })
        );

        let by_quarter = Scalar::Number(Bounds {
            multiple_of: Some(0.25),
            ..Bounds::default()
        });
        assert!(by_quarter.parse("1.75").is_ok());
        assert!(by_quarter.parse("1.3").is_err());
    }

    #[test]
    fn a_choice_outside_the_enum_is_rejected_with_the_alternatives() {
        let status = Scalar::Choice(vec!["draft".to_owned(), "paid".to_owned()]);
        assert_eq!(
            status.parse("void"),
            Err(ScalarError::Choice {
                raw: "void".to_owned(),
                allowed: vec!["draft".to_owned(), "paid".to_owned()],
            })
        );
        assert!(status.parse("paid").is_ok());
    }

    #[test]
    fn integers_stay_integers_in_json() {
        let plain = Scalar::Integer(Bounds::default());
        assert_eq!(plain.parse("5"), Ok(serde_json::json!(5)));
        assert_eq!(
            plain.parse("5.0"),
            Err(ScalarError::Kind {
                raw: "5.0".to_owned(),
                wanted: "an integer",
            })
        );
    }

    /// A float JSON cannot carry is not a number, rather than a `null` sent in
    /// place of one.
    #[test]
    fn a_number_json_cannot_carry_is_refused_rather_than_nulled() {
        let plain = Scalar::Number(Bounds::default());
        assert_eq!(plain.parse("1.5"), Ok(serde_json::json!(1.5)));
        for raw in ["inf", "-inf", "NaN"] {
            assert_eq!(
                plain.parse(raw),
                Err(ScalarError::Kind {
                    raw: raw.to_owned(),
                    wanted: "a number",
                }),
                "{raw}"
            );
        }
    }

    /// `probes/A3b/scalars` (`integer`): `typify` writes a `u64` for `minimum: 0`,
    /// so an integer past `i64` is a number, and "not an integer" is false of it.
    #[test]
    fn an_integer_past_i64_is_out_of_range_and_not_a_non_integer() {
        let plain = Scalar::Integer(Bounds::default());
        for raw in [
            "9223372036854775808",
            "-9223372036854775809",
            "+18446744073709551615",
        ] {
            let refused = refusal(&plain, raw);
            assert!(
                matches!(&refused, ScalarError::Rule { raw: given, rule }
                    if given == raw && rule.starts_with("is out of range")),
                "{refused:?}"
            );
            assert_eq!(plain.wire(raw), Err(refused), "{raw}");
        }
        assert_eq!(
            plain.parse("9223372036854775807"),
            Ok(serde_json::json!(i64::MAX))
        );
        assert_eq!(
            plain.parse("-9223372036854775808"),
            Ok(serde_json::json!(i64::MIN))
        );
        // What is no integer at all is still said to be none.
        for raw in ["5.0", "1e3", "", "five", "9223372036854775808.0"] {
            assert!(
                matches!(
                    plain.parse(raw),
                    Err(ScalarError::Kind {
                        wanted: "an integer",
                        ..
                    })
                ),
                "{raw}"
            );
        }
    }

    /// `probes/A3b/scalars` (`number`): `9007199254740993` goes out as
    /// `9007199254740992.0` and `1e-400` as `0.0` unless it is refused.
    #[test]
    fn a_number_that_does_not_survive_the_round_trip_through_f64_is_refused() {
        let plain = Scalar::Number(Bounds::default());
        for raw in [
            "9007199254740993",
            "1e-400",
            "123456789012345678",
            "0.1000000000000000055511151231257827",
            "-9007199254740993",
            "4.9e-325",
            "1e-99999999999999999999",
        ] {
            let refused = refusal(&plain, raw);
            assert!(
                matches!(&refused, ScalarError::Rule { raw: given, rule }
                    if given == raw && rule.starts_with("does not survive the round trip")),
                "{raw}: {refused:?}"
            );
            assert_eq!(plain.wire(raw), Err(refused), "{raw}");
        }
        // The refusal says what would have been sent.
        assert_eq!(
            refusal(&plain, "9007199254740993").to_string(),
            "`9007199254740993` does not survive the round trip through a 64-bit float, \
             which would send 9007199254740992"
        );
    }

    #[test]
    fn a_number_that_is_the_shortest_text_of_its_float_is_accepted_however_it_is_spelled() {
        let plain = Scalar::Number(Bounds::default());
        for raw in [
            "0.1",
            "19.99",
            "+5",
            "007",
            "1.0",
            "1e3",
            "1E3",
            "-0",
            ".5",
            "5.",
            "0.30000000000000004",
            "9007199254740992",
            "1.7976931348623157e308",
            "5e-324",
            "100000000000000000000000",
            "0.00000000000000000000000000000000000000000000000001",
            "0e99999999999999999999",
            "-0.000E-99999999999999999999",
        ] {
            assert!(plain.parse(raw).is_ok(), "{raw}");
        }
    }

    /// Zero is zero whatever its exponent says, and any other number needs an
    /// exponent that an `i64` holds.
    #[test]
    fn a_decimal_reads_zero_before_it_reads_the_exponent() {
        let zero = Decimal::of("0");
        assert!(zero.is_some());
        for text in [
            "0e99999999999999999999",
            "-0.00E-99999999999999999999",
            "0e0",
        ] {
            assert_eq!(Decimal::of(text), zero, "{text}");
        }
        for text in ["1e99999999999999999999", "1e-99999999999999999999"] {
            assert_eq!(Decimal::of(text), None, "{text}");
        }
        // What `f64::from_str` refuses has no reading, zero or not.
        for text in ["0e", "0ex", "0e+", "1e", "e5", "", "0.0.0"] {
            assert_eq!(Decimal::of(text), None, "{text:?}");
        }
    }

    /// What is no finite number is "not a number", and a spelling that is a
    /// number past what a float holds is "out of range".
    #[test]
    fn a_number_past_what_a_float_holds_is_out_of_range() {
        let plain = Scalar::Number(Bounds::default());
        for raw in ["1e400", "-1e400", "1e99999999999999999999"] {
            let refused = refusal(&plain, raw);
            assert!(
                matches!(&refused, ScalarError::Rule { rule, .. }
                    if rule.starts_with("is out of range")),
                "{raw}: {refused:?}"
            );
        }
        for raw in ["inf", "-Infinity", "NaN", "1e", "e5", ".", "1_0", " 1"] {
            assert!(
                matches!(
                    plain.parse(raw),
                    Err(ScalarError::Kind {
                        wanted: "a number",
                        ..
                    })
                ),
                "{raw}"
            );
        }
    }

    /// The text of a parameter on the wire is the parsed value: the sign of a
    /// positive number and the zeros before a number are not part of it, and
    /// what the typed call sends for a number is what the command line sends.
    #[test]
    fn the_wire_text_is_the_parsed_value() {
        let integer = Scalar::Integer(Bounds::default());
        for (raw, sent) in [("+5", "5"), ("007", "7"), ("-0", "0"), ("-12", "-12")] {
            assert_eq!(integer.wire(raw).as_deref(), Ok(sent), "{raw}");
        }
        let number = Scalar::Number(Bounds::default());
        for (raw, sent) in [
            ("1.0", "1"),
            ("1e3", "1000"),
            ("+0.50", "0.5"),
            ("007.25", "7.25"),
            ("1e-7", "0.0000001"),
        ] {
            assert_eq!(number.wire(raw).as_deref(), Ok(sent), "{raw}");
        }
        // The typed call writes a number with `ToString`, and what it wrote
        // is sent as it is.
        for typed in [0.1_f64, 19.99, 1e21, 1.5e-7, 123.0, f64::MAX, 5e-324] {
            let text = typed.to_string();
            assert_eq!(number.wire(&text), Ok(text), "{typed}");
        }
        assert_eq!(Scalar::Boolean.wire("true").as_deref(), Ok("true"));
        assert_eq!(
            Scalar::Text(Text::default()).wire("+5 ").as_deref(),
            Ok("+5 ")
        );
        assert_eq!(
            Scalar::Choice(vec!["007".to_owned()])
                .wire("007")
                .as_deref(),
            Ok("007")
        );
        // Every rule still runs, and the one refusal is `parse`'s.
        let bounded = Scalar::Integer(Bounds {
            low: Some(Limit::Inclusive(1)),
            ..Bounds::default()
        });
        assert_eq!(bounded.wire("0"), Err(refusal(&bounded, "0")));
    }

    /// The help line and the refusal are one rendering, so what `--help`
    /// advertises is the rule that runs.
    #[test]
    fn every_note_is_a_rule_that_is_enforced() {
        let code = Scalar::Text(Text {
            pattern: Some("^[A-Z]+$".to_owned()),
            min_length: Some(2),
            max_length: Some(4),
        });
        assert_eq!(
            code.note().as_deref(),
            Some("at least 2 characters; at most 4 characters; matches ^[A-Z]+$")
        );
        assert!(code.parse("A").is_err());
        assert!(code.parse("ABCDE").is_err());
        assert!(code.parse("ab").is_err());
        assert!(code.parse("AB").is_ok());

        let count = Scalar::Integer(Bounds {
            low: Some(Limit::Inclusive(1)),
            high: Some(Limit::Exclusive(10)),
            multiple_of: Some(3),
        });
        assert_eq!(
            count.note().as_deref(),
            Some("at least 1; less than 10; a multiple of 3")
        );
        assert_eq!(Scalar::Boolean.note(), None);
        assert_eq!(Scalar::Text(Text::default()).note(), None);
    }
}
