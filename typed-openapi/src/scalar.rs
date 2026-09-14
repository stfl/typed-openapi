//! The value kinds a command-line flag can carry, and how a raw argument
//! string becomes the JSON the wire wants.
//!
//! A [`Scalar`] is the only thing this crate knows about a schema: everything
//! richer than these six kinds is a whole-body affair and never reaches a flag.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What one flag accepts, as the document describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scalar {
    /// A string. Carries the document's `pattern`, which reaches `--help` but
    /// is not enforced — see [`Scalar::parse`].
    Text(Option<String>),
    /// `type: string, format: money` — a decimal amount carried in a string.
    /// Carries the document's `pattern` for the help line, as `Text` does.
    Money(Option<String>),
    /// `type: integer`.
    Integer,
    /// `type: number`.
    Number,
    /// `type: boolean`.
    Boolean,
    /// `enum: [..]` on a string schema. These values complete.
    Choice(Vec<String>),
}

/// A raw argument the document's schema rejects.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ScalarError {
    #[error("`{raw}` is not an integer")]
    Integer { raw: String },
    #[error("`{raw}` is not a number")]
    Number { raw: String },
    #[error("`{raw}` is not `true` or `false`")]
    Boolean { raw: String },
    #[error("`{raw}` is not an amount (digits, optionally `.` and one or two decimals)")]
    Money { raw: String },
    #[error("`{raw}` is not one of {}", .allowed.join(", "))]
    Choice { raw: String, allowed: Vec<String> },
}

impl Scalar {
    /// Turn one raw argument into the JSON value the wire wants.
    ///
    /// [`Scalar::Text`]'s `pattern` is documentation only: enforcing an
    /// arbitrary ECMA-262 pattern costs a regex engine, and the one constraint
    /// this document actually cares about — an amount — is [`Scalar::Money`],
    /// which is enforced.
    pub fn parse(&self, raw: &str) -> Result<serde_json::Value, ScalarError> {
        let owned = || raw.to_owned();
        match self {
            Self::Text(_) => Ok(serde_json::Value::String(owned())),
            Self::Money(_) => {
                if is_amount(raw) {
                    Ok(serde_json::Value::String(owned()))
                } else {
                    Err(ScalarError::Money { raw: owned() })
                }
            }
            Self::Integer => raw
                .parse::<i64>()
                .map(Into::into)
                .map_err(|_| ScalarError::Integer { raw: owned() }),
            Self::Number => raw
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(serde_json::Value::Number)
                .ok_or_else(|| ScalarError::Number { raw: owned() }),
            Self::Boolean => raw
                .parse::<bool>()
                .map(Into::into)
                .map_err(|_| ScalarError::Boolean { raw: owned() }),
            Self::Choice(allowed) => {
                if allowed.iter().any(|v| v == raw) {
                    Ok(serde_json::Value::String(owned()))
                } else {
                    Err(ScalarError::Choice {
                        raw: owned(),
                        allowed: allowed.clone(),
                    })
                }
            }
        }
    }

    /// The `<NAME>` shown for the flag's value, and the closest thing the
    /// document gives an agent to a type.
    #[must_use]
    pub fn value_name(&self) -> &'static str {
        match self {
            Self::Text(_) | Self::Choice(_) => "STRING",
            Self::Money(_) => "AMOUNT",
            Self::Integer => "INT",
            Self::Number => "NUMBER",
            Self::Boolean => "BOOL",
        }
    }

    /// The constraint worth appending to a flag's help line, if any.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Money(None) => Some("e.g. 12.50".to_owned()),
            Self::Money(Some(pattern)) => Some(format!("e.g. 12.50; matches {pattern}")),
            Self::Text(Some(pattern)) => Some(format!("matches {pattern}")),
            Self::Text(None) | Self::Choice(_) | Self::Integer | Self::Number | Self::Boolean => {
                None
            }
        }
    }
}

/// `-?` digits, optionally `.` and one or two more digits.
fn is_amount(raw: &str) -> bool {
    let unsigned = raw.strip_prefix('-').unwrap_or(raw);
    let (units, cents) = match unsigned.split_once('.') {
        Some((units, cents)) => (units, Some(cents)),
        None => (unsigned, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(units) && cents.is_none_or(|c| c.len() <= 2 && digits(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_accepts_the_shapes_an_accountant_types() {
        for raw in ["12.50", "0", "0.0", "-3.07", "1000000.00"] {
            assert_eq!(
                Scalar::Money(None).parse(raw),
                Ok(serde_json::Value::String(raw.to_owned())),
                "{raw}"
            );
        }
    }

    #[test]
    fn money_rejects_everything_else() {
        for raw in ["12.5x", "", "12.505", ".5", "1,50", "-", "1e3"] {
            assert!(Scalar::Money(None).parse(raw).is_err(), "{raw}");
        }
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
        assert_eq!(Scalar::Integer.parse("5"), Ok(serde_json::json!(5)));
        assert!(Scalar::Integer.parse("5.0").is_err());
    }

    #[test]
    fn a_pattern_reaches_help_but_not_the_parser() {
        let text = Scalar::Text(Some("^[A-Z]+$".to_owned()));
        assert_eq!(text.note().as_deref(), Some("matches ^[A-Z]+$"));
        assert!(text.parse("lowercase").is_ok());
    }
}
