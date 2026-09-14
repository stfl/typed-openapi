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
    Kind { raw: String, wanted: &'static str },
    /// Outside the document's `enum` — the one refusal that can say what to
    /// type instead, which is why it carries the list rather than a sentence.
    #[error("`{raw}` is not one of {}", .allowed.join(", "))]
    Choice { raw: String, allowed: Vec<String> },
    /// The right kind of value, refused by a rule the document states. `rule`
    /// finishes the sentence and carries the document's own number, which is
    /// the only part a user can act on; nothing branches on which keyword it
    /// came from.
    #[error("`{raw}` {rule}")]
    Rule { raw: String, rule: String },
    /// A `pattern` the engine cannot read. Every value is refused, because a
    /// rule nobody can run is not a rule everything passed — and a document
    /// stating one is refused outright while it is reduced.
    #[error("`{pattern}` is not a regular expression: {message}")]
    Pattern { pattern: String, message: String },
}

impl ScalarError {
    /// `raw`, refused by a rule that finishes the sentence.
    fn rule(raw: &str, rule: String) -> Self {
        Self::Rule {
            raw: raw.to_owned(),
            rule,
        }
    }
}

impl Scalar {
    /// Turn one raw argument into the JSON value the wire wants, or refuse it
    /// the way the document does.
    pub fn parse(&self, raw: &str) -> Result<serde_json::Value, ScalarError> {
        match self {
            Self::Text(text) => text.check(raw).map(|()| raw.into()),
            Self::Integer(bounds) => bounded(bounds, raw),
            Self::Number(bounds) => bounded(bounds, raw),
            Self::Boolean => raw
                .parse::<bool>()
                .map(Into::into)
                .map_err(|_| ScalarError::Kind {
                    raw: raw.to_owned(),
                    wanted: "`true` or `false`",
                }),
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

    /// Whether every rule this carries can be run at all.
    ///
    /// A `pattern` the engine cannot read is the only rule that can fail this
    /// way, and it fails on every value — so a document is held to this while
    /// it is reduced, and the failure names the operation rather than turning
    /// up at a user's flag.
    pub fn runnable(&self) -> Result<(), ScalarError> {
        match self {
            Self::Text(text) => text.regex().map(drop),
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
        // JSON Schema's `pattern` is a search rather than a whole-string match,
        // so an unanchored pattern matches anywhere in the value. Anchoring it
        // here would refuse values the document allows.
        match (&self.pattern, self.regex()?) {
            (Some(pattern), Some(regex)) if regex.find(raw).is_none() => {
                Err(ScalarError::rule(raw, format!("does not match {pattern}")))
            }
            _ => Ok(()),
        }
    }

    /// The document's `pattern`, compiled.
    fn regex(&self) -> Result<Option<Regex>, ScalarError> {
        self.pattern
            .as_deref()
            .map(|pattern| {
                Regex::new(pattern).map_err(|error| ScalarError::Pattern {
                    pattern: pattern.to_owned(),
                    message: error.to_string(),
                })
            })
            .transpose()
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

impl<T: Numeric> Bounds<T> {
    /// Hold one value to every rule the document states about it.
    fn check(&self, value: T, raw: &str) -> Result<(), ScalarError> {
        let refuse = |rule: String| ScalarError::rule(raw, format!("is not {rule}"));
        if let Some(low) = self.low {
            low.floor(value).map_err(&refuse)?;
        }
        if let Some(high) = self.high {
            high.ceiling(value).map_err(&refuse)?;
        }
        if let Some(step) = self.multiple_of
            && !value.divisible_by(step)
        {
            return Err(refuse(format!("a multiple of {step}")));
        }
        Ok(())
    }

    fn notes(&self) -> Vec<String> {
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

impl<T: Numeric> Limit<T> {
    /// This limit as a lower bound. The `Err` is how the rule reads, and it is
    /// how `--help` reads it too — one rendering, so a refusal cannot describe
    /// a different rule from the one the help line advertised.
    fn floor(self, value: T) -> Result<(), String> {
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
    fn ceiling(self, value: T) -> Result<(), String> {
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

    fn note(self, inclusive: &str, exclusive: &str) -> String {
        match self {
            Self::Inclusive(limit) => format!("{inclusive} {limit}"),
            Self::Exclusive(limit) => format!("{exclusive} {limit}"),
        }
    }
}

/// One argument as a number the document's rules admit.
fn bounded<T: Numeric>(bounds: &Bounds<T>, raw: &str) -> Result<serde_json::Value, ScalarError> {
    let (value, json) = T::read(raw).ok_or_else(|| ScalarError::Kind {
        raw: raw.to_owned(),
        wanted: T::KIND,
    })?;
    bounds.check(value, raw)?;
    Ok(json)
}

/// The two number kinds a document can ask one flag for.
///
/// A trait rather than two copies of [`Bounds`] and its rules: the comparisons
/// are the same sentence in both, and only reading a value out of an argument
/// and dividing by one differ.
pub trait Numeric: Copy + PartialOrd + fmt::Display {
    /// What a refusal calls this kind.
    const KIND: &'static str;

    /// This kind read out of one argument, with the JSON it goes out as.
    ///
    /// One step, because the two can disagree: a float that parses but is not
    /// finite is not a number JSON carries, and "is not a number" is the honest
    /// answer rather than a `null` on the wire.
    fn read(raw: &str) -> Option<(Self, serde_json::Value)>;

    /// `multipleOf`: whether dividing by `step` leaves a whole number.
    fn divisible_by(self, step: Self) -> bool;
}

impl Numeric for i64 {
    const KIND: &'static str = "an integer";

    fn read(raw: &str) -> Option<(Self, serde_json::Value)> {
        raw.parse::<Self>().ok().map(|value| (value, value.into()))
    }

    fn divisible_by(self, step: Self) -> bool {
        step != 0 && self % step == 0
    }
}

impl Numeric for f64 {
    const KIND: &'static str = "a number";

    fn read(raw: &str) -> Option<(Self, serde_json::Value)> {
        let value = raw.parse::<Self>().ok()?;
        Some((value, serde_json::Number::from_f64(value)?.into()))
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
