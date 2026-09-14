//! The naming rules a command line forces on a document: where an operation
//! sits in the two-level command tree, and what happens when two things in one
//! operation want the same flag.

use std::fmt;

#[cfg(feature = "document")]
use http::Method;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A name the document offers that is not spellable as a command.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("the {origin} `{raw}` does not kebab-case into [a-z0-9-]")]
pub struct NameError {
    /// What the name was read off, so the message says where to go and look:
    /// a path segment, or the `x-cli-` marker that overrode it.
    pub origin: &'static str,
    /// The name, as the document spells it.
    pub raw: String,
}

/// One half of a command name: kebab-cased, `[a-z0-9-]` only.
///
/// A command is two of these, `<group> <command>` — `vouchers update`. Which
/// path segment each half is taken from is the grouping rule a bless step
/// runs; this type is only what the user types.
///
/// A bless step writes both into its reduced model, so a name comes back off a
/// blob as well as out of a document. Both doors are the same door: `serde`
/// reads it as a `String` and runs it through [`CommandName::new`], so a blob
/// cannot smuggle in a name a document could not have produced.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommandName(String);

impl CommandName {
    /// `renderVoucher` becomes `render-voucher`; anything that will not reduce to
    /// `[a-z0-9-]` is rejected rather than mangled.
    ///
    /// `origin` is what the raw name was read off, and it is there for the
    /// error: a document whose own words cannot be spelled is told which word.
    pub fn new(origin: &'static str, raw: &str) -> Result<Self, NameError> {
        let name = kebab(raw);
        let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
        if name.is_empty() || !name.bytes().all(allowed) {
            return Err(NameError {
                origin,
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
    type Error = NameError;

    fn try_from(raw: String) -> Result<Self, NameError> {
        Self::new("reduced model", &raw)
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

/// Where each of a document's operations sits in the command tree.
///
/// *Requires the `document` feature.*
///
/// The group is a path segment, and which one is a fact about the whole
/// document: the first, unless every operation shares it — a document served
/// entirely under `/v1` must not collapse into one group named `v1`. So the
/// rule is read off every path once, with [`Grouping::of`], and then applied
/// to one path at a time.
///
/// Grouping is bless-time work. Both names travel in the reduced model, so a
/// shipped binary reads them off its blob and never runs this rule at all.
#[cfg(feature = "document")]
#[derive(Debug, Clone, Copy)]
pub struct Grouping {
    depth: usize,
}

#[cfg(feature = "document")]
impl Grouping {
    /// Read the rule off every path the document declares.
    ///
    /// Descend while every path says the same thing — a segment they all share
    /// tells nothing apart — and stop at the first segment that distinguishes
    /// them, or before a path parameter, which names a value rather than a
    /// resource.
    #[must_use]
    pub fn of(paths: &[&str]) -> Self {
        let mut depth = 0;
        while descends(paths, depth) {
            depth += 1;
        }
        Self { depth }
    }

    /// The group `path` belongs to.
    pub fn group(&self, path: &str) -> Result<CommandName, NameError> {
        CommandName::new(
            "path segment",
            segments(path).nth(self.depth).unwrap_or(path),
        )
    }

    /// What one operation is called under its group: the last literal segment
    /// below the group, or — where the path has none left to spend — whatever
    /// its method makes of it.
    pub fn leaf(&self, path: &str, method: &Method) -> Result<CommandName, NameError> {
        match segments(path)
            .skip(self.depth + 1)
            .filter(|segment| !templated(segment))
            .last()
        {
            Some(last) => CommandName::new("path segment", last),
            None => CommandName::new("method", verb(method, segments(path).any(templated))),
        }
    }
}

/// A path's segments: `/vouchers/{id}/render` is three.
#[cfg(feature = "document")]
fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|segment| !segment.is_empty())
}

/// A segment the caller fills in, `{id}`.
#[cfg(feature = "document")]
fn templated(segment: &str) -> bool {
    segment.contains('{')
}

/// Is the group one segment further down than `depth`?
///
/// Only when every path says the same thing at `depth`, and every one of them
/// has a literal segment below it to be grouped by instead.
#[cfg(feature = "document")]
fn descends(paths: &[&str], depth: usize) -> bool {
    let Some(shared) = paths.first().and_then(|path| segments(path).nth(depth)) else {
        return false;
    };
    paths.iter().all(|path| {
        let mut below = segments(path).skip(depth);
        below.next() == Some(shared) && below.next().is_some_and(|next| !templated(next))
    })
}

/// What an operation whose path has nothing left to say is called.
///
/// A safe method reads: one resource where the path addresses one, the
/// collection where it does not. Everything else is named for what it does.
#[cfg(feature = "document")]
fn verb(method: &Method, addressed: bool) -> &'static str {
    match *method {
        Method::POST => "create",
        Method::PUT => "update",
        Method::PATCH => "patch",
        Method::DELETE => "delete",
        Method::HEAD => "head",
        Method::OPTIONS => "options",
        Method::TRACE => "trace",
        _ if addressed => "get",
        _ => "list",
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
#[expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    #[test]
    fn names_kebab_case_into_the_allowed_alphabet() {
        for (raw, want) in [
            ("createVoucher", "create-voucher"),
            ("uploadDocumentMultipart", "upload-document-multipart"),
            ("getHTTPStatus", "get-http-status"),
            ("internal_ref", "internal-ref"),
            ("VoucherLineItem", "voucher-line-item"),
            ("Already-Kebab", "already-kebab"),
        ] {
            let name = CommandName::new("path segment", raw);
            assert_eq!(name.map(|n| n.as_str().to_owned()), Ok(want.to_owned()));
        }
    }

    #[test]
    fn a_name_with_nothing_to_kebab_is_rejected_and_says_where_it_came_from() {
        assert!(CommandName::new("path segment", "").is_err());
        let error = CommandName::new("x-cli-command", "___").expect_err("nothing to kebab");
        assert_eq!(
            error.to_string(),
            "the x-cli-command `___` does not kebab-case into [a-z0-9-]"
        );
    }

    #[cfg(feature = "document")]
    #[test]
    fn the_group_is_the_segment_that_first_tells_operations_apart() {
        let grouping = Grouping::of(&["/vouchers", "/vouchers/{id}", "/contacts"]);
        let group = |path| {
            grouping
                .group(path)
                .map(|name| name.as_str().to_owned())
                .expect("a spellable segment")
        };
        assert_eq!(group("/vouchers/{id}"), "vouchers");
        assert_eq!(group("/contacts"), "contacts");
    }

    /// A document served entirely under one prefix must not collapse into one
    /// group named for the prefix.
    #[cfg(feature = "document")]
    #[test]
    fn a_shared_prefix_is_descended_past() {
        let grouping = Grouping::of(&["/v1/vouchers", "/v1/vouchers/{id}", "/v1/contacts"]);
        let group = |path| {
            grouping
                .group(path)
                .map(|name| name.as_str().to_owned())
                .expect("a spellable segment")
        };
        assert_eq!(group("/v1/vouchers/{id}"), "vouchers");
        assert_eq!(group("/v1/contacts"), "contacts");
    }

    /// Descending stops before a path parameter: `{tenant}` names a value the
    /// user supplies, not a resource to group by.
    #[cfg(feature = "document")]
    #[test]
    fn descending_stops_before_a_path_parameter() {
        let grouping = Grouping::of(&["/v1/{tenant}/vouchers", "/v1/{tenant}/contacts"]);
        assert_eq!(
            grouping
                .group("/v1/{tenant}/vouchers")
                .map(|name| name.as_str().to_owned()),
            Ok("v1".to_owned())
        );
    }

    #[cfg(feature = "document")]
    #[test]
    fn a_leaf_is_the_last_literal_segment_below_the_group() {
        let grouping = Grouping::of(&["/vouchers/{id}/render", "/contacts"]);
        assert_eq!(
            grouping
                .leaf("/vouchers/{id}/render", &Method::GET)
                .map(|name| name.as_str().to_owned()),
            Ok("render".to_owned())
        );
    }

    /// Where the path has no segment left to spend, the method names the
    /// operation — and a safe method says which of the two reads it is.
    #[cfg(feature = "document")]
    #[test]
    fn a_path_with_nothing_left_to_say_is_named_by_its_method() {
        let grouping = Grouping::of(&["/vouchers", "/vouchers/{id}"]);
        let leaf = |path, method: Method| {
            grouping
                .leaf(path, &method)
                .map(|name| name.as_str().to_owned())
                .expect("a method always spells one")
        };
        assert_eq!(leaf("/vouchers", Method::GET), "list");
        assert_eq!(leaf("/vouchers/{id}", Method::GET), "get");
        assert_eq!(leaf("/vouchers", Method::POST), "create");
        assert_eq!(leaf("/vouchers/{id}", Method::PUT), "update");
        assert_eq!(leaf("/vouchers/{id}", Method::PATCH), "patch");
        assert_eq!(leaf("/vouchers/{id}", Method::DELETE), "delete");
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
