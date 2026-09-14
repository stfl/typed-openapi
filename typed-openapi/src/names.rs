//! The two naming rules a command line forces on a document: what an
//! `operationId` is called on the command line, and what happens when two
//! things in one operation want the same flag.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// An `operationId` that is not spellable as a subcommand.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("operationId `{raw}` does not kebab-case into [a-z0-9-]")]
pub struct IdError {
    pub raw: String,
}

/// A subcommand name: the `operationId`, kebab-cased, `[a-z0-9-]` only.
///
/// The wire spelling stays reachable through [`Operation::id`] on the model;
/// this is only what the user types.
///
/// [`Operation::id`]: crate::Operation::id
///
/// A bless step writes one of these into its reduced model, so the name comes
/// back off a blob as well as out of a document. Both doors are the same door:
/// `serde` reads it as a `String` and runs it through
/// [`CommandName::from_operation_id`], so a blob cannot smuggle in a name a
/// document could not have produced.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommandName(String);

impl CommandName {
    /// `createVoucher` becomes `create-voucher`; anything that will not reduce
    /// to `[a-z0-9-]` is rejected rather than mangled.
    pub fn from_operation_id(raw: &str) -> Result<Self, IdError> {
        let name = kebab(raw);
        let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
        if name.is_empty() || !name.bytes().all(allowed) {
            return Err(IdError {
                raw: raw.to_owned(),
            });
        }
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CommandName {
    type Error = IdError;

    fn try_from(raw: String) -> Result<Self, IdError> {
        Self::from_operation_id(&raw)
    }
}

impl From<CommandName> for String {
    fn from(name: CommandName) -> Self {
        name.0
    }
}

impl fmt::Display for CommandName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The flag names one subcommand has already spent.
///
/// *Requires the `document` feature.*
///
/// `PUT /vouchers/{id}` takes an `id` in the path and an `id` in the body; clap
/// panics on a duplicate name, so the second claimant is prefixed rather than
/// dropped. Both values stay reachable from the command line, and the flag that
/// moved says which wire name it carries in its help line — `renamed` is how
/// a flag reports that once the document it came from is gone.
#[cfg(feature = "document")]
#[derive(Debug)]
pub struct Namespace(Vec<String>);

#[cfg(feature = "document")]
impl Namespace {
    /// Start with the CLI's own flags already spent, so a document that happens
    /// to name a field `commit` renames rather than colliding at startup.
    #[must_use]
    pub fn with_reserved<const N: usize>(reserved: [&str; N]) -> Self {
        Self(reserved.iter().map(|s| (*s).to_owned()).collect())
    }

    /// The flag to use: `preferred` when it is free, otherwise prefixed.
    pub fn claim(&mut self, preferred: &str, prefix: &str) -> String {
        let mut candidate = preferred.to_owned();
        let mut renamed = false;
        let mut suffix = 2;
        while self.0.iter().any(|taken| taken == &candidate) {
            candidate = if renamed {
                format!("{prefix}-{preferred}-{suffix}")
            } else {
                format!("{prefix}-{preferred}")
            };
            renamed = true;
            suffix += 1;
        }
        self.0.push(candidate.clone());
        candidate
    }
}

/// Did this flag have to move aside from the plain kebab-case of its wire name?
///
/// `Namespace::claim` prefixes a flag whose preferred name the subcommand has
/// already spent, and a flag that moved has to say which wire name it carries.
/// A parameter and a body field are both flags with wire names, so the rule
/// lives here rather than once in each of them.
pub(crate) fn renamed(flag: &str, wire_name: &str) -> bool {
    flag != kebab(wire_name)
}

/// `createVoucher` and `internal_ref` both become flag-shaped: lowercase words
/// joined by `-`. Characters outside `[A-Za-z0-9]` are separators.
#[must_use]
pub fn kebab(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut chars = name.chars().peekable();
    let mut prev_lower_or_digit = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_uppercase() {
            let starts_word =
                prev_lower_or_digit || chars.peek().is_some_and(char::is_ascii_lowercase);
            if starts_word && !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower_or_digit = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_lower_or_digit = true;
        } else {
            if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
            prev_lower_or_digit = false;
        }
    }
    out.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_ids_kebab_case_into_the_allowed_alphabet() {
        for (raw, want) in [
            ("createVoucher", "create-voucher"),
            ("uploadDocumentMultipart", "upload-document-multipart"),
            ("getHTTPStatus", "get-http-status"),
            ("internal_ref", "internal-ref"),
            ("v2ListVouchers", "v2-list-vouchers"),
            ("Already-Kebab", "already-kebab"),
        ] {
            let name = CommandName::from_operation_id(raw);
            assert_eq!(name.map(|n| n.as_str().to_owned()), Ok(want.to_owned()));
        }
    }

    #[test]
    fn an_operation_id_with_nothing_to_kebab_is_rejected() {
        assert!(CommandName::from_operation_id("").is_err());
        assert!(CommandName::from_operation_id("___").is_err());
    }

    #[cfg(feature = "document")]
    #[test]
    fn a_body_field_moves_aside_for_a_parameter_of_the_same_name() {
        let mut flags = Namespace::with_reserved(["commit", "json-body"]);
        assert_eq!(flags.claim("id", "param"), "id");
        assert_eq!(flags.claim("id", "body"), "body-id");
        assert_eq!(flags.claim("id", "body"), "body-id-3");
        assert_eq!(flags.claim("commit", "body"), "body-commit");
    }
}
