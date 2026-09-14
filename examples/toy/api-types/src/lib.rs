//! The types the adopter owns by hand, and the reason the generated code never
//! has to be edited.
//!
//! The vendor declares `format: money`; the bless step is told that the adopter
//! owns that format, so typify emits [`Money`] wherever the document declares
//! it. The generated `Voucher.total` is this type, with no mirror type in
//! between, no conversion at the boundary, and nothing added to the vendor's
//! `components.schemas` to hang the substitution on.
//!
//! This is a crate rather than a module of `api` because the generated code
//! names it, and the generated code sits *below* `api` so that an adopter's
//! edit does not recompile it. One crate, one job: what `api-generated` must be
//! able to spell and the adopter must be free to edit. It depends on `serde`
//! and on the runtime crate whose rule [`Money`] borrows, and on nothing else.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A decimal amount carried in a string, as `format: money` says.
///
/// The rule is the document's, not this crate's: parsing asks
/// [`typed_openapi::Scalar::Money`], the same check the CLI's `--total` runs, so
/// a value the CLI accepts is a value this type accepts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Money(String);

/// `raw` is not an amount.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("`{raw}` is not an amount (digits, optionally `.` and one or two decimals)")]
pub struct MoneyError {
    pub raw: String,
}

impl FromStr for Money {
    type Err = MoneyError;

    fn from_str(raw: &str) -> Result<Self, MoneyError> {
        typed_openapi::Scalar::Money(None)
            .parse(raw)
            .map(|_| Self(raw.to_owned()))
            .map_err(|_| MoneyError {
                raw: raw.to_owned(),
            })
    }
}

impl TryFrom<String> for Money {
    type Error = MoneyError;

    fn try_from(raw: String) -> Result<Self, MoneyError> {
        raw.parse()
    }
}

impl From<Money> for String {
    fn from(money: Money) -> Self {
        money.0
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_takes_the_documents_rule_not_its_own() {
        assert_eq!(
            "12.50".parse::<Money>().map(|m| m.to_string()),
            Ok("12.50".to_owned())
        );
        for raw in ["12.5x", "", "12.505", "1,50", "1e3"] {
            assert!(raw.parse::<Money>().is_err(), "{raw}");
        }
    }

    #[test]
    fn money_is_a_string_on_the_wire_and_a_type_in_rust() {
        let money: Money = "-3.07".parse().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(serde_json::to_value(&money).ok(), Some("-3.07".into()));
        assert!(serde_json::from_value::<Money>(serde_json::json!(3.07)).is_err());
        assert!(serde_json::from_value::<Money>(serde_json::json!("3.071")).is_err());
    }
}
