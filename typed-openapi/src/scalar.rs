//! The value kinds a command-line flag can carry, the rules the document states
//! about them, and how a raw argument string becomes the JSON the wire wants.
//!
//! A [`Scalar`], and the [`Items`] of a list parameter, are the only things this
//! crate knows about a schema: everything richer than these five kinds is a
//! whole-body affair and never reaches a flag. What a `Scalar` carries is what the
//! document said — a `pattern`, a length, a bound, a step, an `enum` — and every
//! one of those is enforced by [`Scalar::parse`], because a rule that only reaches
//! `--help` is a rule the user finds out about from the server. [`Items`] holds
//! what an array says about its values taken together, which no single value can
//! break: `minItems`, `maxItems` and `uniqueItems`.
//!
//! `pattern` runs on `regress`, the ECMA-262 engine typify puts inside a
//! generated newtype's `FromStr`. One engine, one rule, one set of bytes: a
//! value the command line accepts is a value the generated type accepts by
//! construction, rather than by a hand-written rule kept in step.
//!
//! A rule has one note, the words that finish "is not …", and its refusal is
//! those words after "is not": the help line and the refusal cannot describe
//! two rules.

use std::collections::HashSet;
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
    /// `type: boolean`. `Some` is the one value that its `enum` lists, and
    /// `None` is a schema that admits both: an `enum` of `true` and `false`
    /// states nothing.
    Boolean(Option<bool>),
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bounds<T> {
    /// `minimum`, with `exclusiveMinimum` folded into it.
    pub low: Option<Limit<T>>,
    /// `maximum`, with `exclusiveMaximum` folded into it.
    pub high: Option<Limit<T>>,
    /// `multipleOf`.
    pub multiple_of: Option<T>,
    /// `enum`, in the document's order, and empty where the document lists no
    /// value. A value is admitted when it is one of these, whatever its spelling.
    pub allowed: Vec<T>,
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

/// What an array schema states about the values of a list parameter taken
/// together, which no single value can break.
///
/// One value, one check and one note: [`Items::check`] is the only place
/// `minItems`, `maxItems` and `uniqueItems` are enforced, and [`Items::note`]
/// the only place they are rendered. The default states nothing.
///
/// An adopter's own command that takes a list asks them as one that takes a
/// value asks [`Scalar::parse`] and [`Scalar::note`]: the check for the values
/// the operator gave, and the note for the help line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Items {
    /// `minItems`.
    pub min: Option<usize>,
    /// `maxItems`.
    pub max: Option<usize>,
    /// `uniqueItems`: no two values are the same value.
    pub unique: bool,
}

/// What the document's schema refuses: a value the operator typed, the values
/// given for a list parameter, or a rule of the document that cannot be run.
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
    #[error("`{raw}` is not {}", one_of(.allowed))]
    Choice {
        /// The argument as the operator typed it.
        raw: String,
        /// The values the document's `enum` lists, in its order, spelled as
        /// the operator types them.
        allowed: Vec<String>,
    },
    /// What a rule the document states says is not so, in the document's own
    /// number: a value of the right kind that breaks the rule, a list that breaks
    /// the rules of its array, or a rule that has no meaning, such as a `pattern`
    /// that cannot be run as the vendor means it. `rule` finishes the sentence and
    /// carries the document's own number, which is the only part a user can act
    /// on; nothing branches on which keyword it came from.
    #[error("`{raw}` {rule}")]
    Rule {
        /// What the sentence is about. The argument as the operator typed it, for
        /// a value; the values given for a list parameter, joined with `, `; the
        /// `pattern` or the `multipleOf` step as the document spells it, for a
        /// rule that cannot be run.
        raw: String,
        /// The rest of the sentence: what `raw` is not, or what it holds, with the
        /// document's own number in it.
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

    /// `raw`, which is not what a rule's note says. Every refusal of a rule but an
    /// `enum`'s is built here, from the note that `--help` shows for it, so a
    /// refusal cannot describe a different rule from the one the help line
    /// advertised. [`ScalarError::outside`] builds the refusal of an `enum`, from
    /// the list that its note spells (`one_of`).
    fn refusal(raw: &str, note: &str) -> Self {
        Self::rule(raw, format!("is not {note}"))
    }

    /// `raw`, outside an `enum` that lists `allowed`.
    fn outside<T: fmt::Display>(raw: &str, allowed: &[T]) -> Self {
        Self::Choice {
            raw: raw.to_owned(),
            allowed: spelled(allowed),
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
            Self::Boolean(only) => {
                let value = raw
                    .parse::<bool>()
                    .map_err(|_| ScalarError::kind(raw, "`true` or `false`"))?;
                match only {
                    Some(allowed) if *allowed != value => {
                        Err(ScalarError::outside(raw, &[allowed]))
                    }
                    Some(_) | None => Ok(value.into()),
                }
            }
            Self::Choice(allowed) => {
                if allowed.iter().any(|value| value == raw) {
                    Ok(raw.into())
                } else {
                    Err(ScalarError::outside(raw, allowed))
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
            Self::Text(_) | Self::Boolean(_) | Self::Choice(_) => {
                self.parse(raw).map(|_| raw.to_owned())
            }
        }
    }

    /// Whether every rule this carries can be run as the document means it.
    ///
    /// Three rules fail this way, and each fails on every value or on a value
    /// that means something else elsewhere, so a document is held to this while
    /// it is reduced and the failure names the operation rather than turning up
    /// at a user's flag. A `pattern` the engine cannot read refuses every value.
    /// A `pattern` with `\p{`, `\P{` or `\u{` means one thing with the Unicode
    /// flag and another without it, and a generated type cannot be given the
    /// flag. A `multipleOf` that is not more than 0 has no multiples to admit.
    pub fn runnable(&self) -> Result<(), ScalarError> {
        match self {
            Self::Text(text) => text.runnable(),
            Self::Integer(bounds) => bounds.runnable(),
            Self::Number(bounds) => bounds.runnable(),
            Self::Boolean(_) | Self::Choice(_) => Ok(()),
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
            Self::Boolean(_) => "BOOL",
        }
    }

    /// Everything the document constrains about this value, for the flag's help
    /// line. Every note is a rule [`Scalar::parse`] enforces, in the document's
    /// own numbers — help and refusal cannot come apart, because one rendering
    /// serves both.
    ///
    /// A string `enum` has no note: the flag lists its values, and they
    /// complete.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        let notes = match self {
            Self::Text(text) => text.notes(),
            Self::Integer(bounds) => bounds.notes(),
            Self::Number(bounds) => bounds.notes(),
            Self::Boolean(only) => only.iter().map(|only| one_of(&[only])).collect(),
            Self::Choice(_) => Vec::new(),
        };
        joined(&notes)
    }
}

impl Items {
    /// Hold the values given for one list parameter to the rules of the array.
    ///
    /// `raws` are the arguments as the operator typed them, in the order they
    /// were given, and `scalar` is the rule each of them has to meet. Two values
    /// are the same value when `scalar` reads them as the same: `+1` and `01` are
    /// one integer, and `0` and `-0` are one number.
    pub fn check(&self, scalar: &Scalar, raws: &[&str]) -> Result<(), ScalarError> {
        let refuse = |note: String| ScalarError::refusal(&raws.join(", "), &list_of(&note));
        if let Some(least) = self.min
            && raws.len() < least
        {
            return Err(refuse(at_least(least)));
        }
        if let Some(most) = self.max
            && raws.len() > most
        {
            return Err(refuse(at_most(most)));
        }
        if self.unique && repeats(scalar, raws)? {
            return Err(refuse(DIFFERENT.to_owned()));
        }
        Ok(())
    }

    /// What the document states about the list, for the flag's help line, and
    /// the words [`Items::check`] refuses with.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        joined(&self.notes())
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        // A list of at least 0 values is any list, so the rule states nothing.
        if let Some(least) = self.min.filter(|least| *least > 0) {
            notes.push(at_least(least));
        }
        if let Some(most) = self.max {
            notes.push(at_most(most));
        }
        if self.unique {
            notes.push(DIFFERENT.to_owned());
        }
        notes
    }
}

/// What `uniqueItems` asks of the values, finishing "a list of …".
const DIFFERENT: &str = "different values";

/// "a list of …", which a refusal of a list says and its notes leave out: the
/// help line is beside a flag that is a list already.
fn list_of(note: &str) -> String {
    format!("a list of {note}")
}

/// `minItems`.
fn at_least(count: usize) -> String {
    format!("at least {}", n_values(count))
}

/// `maxItems`.
fn at_most(count: usize) -> String {
    format!("at most {}", n_values(count))
}

fn n_values(count: usize) -> String {
    if count == 1 {
        "1 value".to_owned()
    } else {
        format!("{count} values")
    }
}

/// Whether one value is given twice, as `scalar` reads the values.
fn repeats(scalar: &Scalar, raws: &[&str]) -> Result<bool, ScalarError> {
    let mut seen = HashSet::new();
    for raw in raws {
        if !seen.insert(identity(&scalar.parse(raw)?)) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One value as `uniqueItems` compares it. A value is the JSON that `Scalar`
/// reads, so two spellings of one number are one key, and `-0` is `0` as it is to
/// JSON Schema.
fn identity(value: &serde_json::Value) -> String {
    if let serde_json::Value::Number(number) = value
        && number.as_f64() == Some(0.0)
    {
        return "0".to_owned();
    }
    value.to_string()
}

/// The notes of one value, joined as a help line carries them.
fn joined(notes: &[String]) -> Option<String> {
    (!notes.is_empty()).then(|| notes.join("; "))
}

/// An `enum`, as a note and as the end of a refusal.
fn one_of<T: fmt::Display>(allowed: &[T]) -> String {
    format!("one of {}", spelled(allowed).join(", "))
}

/// The values of an `enum`, spelled as the operator types them.
fn spelled<T: fmt::Display>(allowed: &[T]) -> Vec<String> {
    allowed.iter().map(ToString::to_string).collect()
}

impl Text {
    /// Hold one argument to every rule the document states about it.
    fn check(&self, raw: &str) -> Result<(), ScalarError> {
        // `minLength` and `maxLength` count characters, not bytes.
        let length = raw.chars().count();
        if let Some(least) = self.min_length
            && length < least
        {
            return Err(ScalarError::refusal(raw, &at_least_long(least)));
        }
        if let Some(most) = self.max_length
            && length > most
        {
            return Err(ScalarError::refusal(raw, &at_most_long(most)));
        }
        let Some(pattern) = self.pattern.as_deref() else {
            return Ok(());
        };
        // JSON Schema's `pattern` is a search rather than a whole-string match,
        // so an unanchored pattern matches anywhere in the value. Anchoring it
        // here would refuse values the document allows.
        if compiled(pattern)?.find(raw).is_none() {
            return Err(ScalarError::refusal(raw, &a_match_for(pattern)));
        }
        Ok(())
    }

    /// Whether the pattern can be run as the document means it.
    fn runnable(&self) -> Result<(), ScalarError> {
        let Some(pattern) = self.pattern.as_deref() else {
            return Ok(());
        };
        compiled(pattern)?;
        unflagged(pattern)
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if let Some(least) = self.min_length {
            notes.push(at_least_long(least));
        }
        if let Some(most) = self.max_length {
            notes.push(at_most_long(most));
        }
        if let Some(pattern) = &self.pattern {
            notes.push(a_match_for(pattern));
        }
        notes
    }
}

/// `minLength`.
fn at_least_long(count: usize) -> String {
    format!("at least {count} characters")
}

/// `maxLength`.
fn at_most_long(count: usize) -> String {
    format!("at most {count} characters")
}

/// `pattern`.
fn a_match_for(pattern: &str) -> String {
    format!("a match for {pattern}")
}

/// Refuse a `pattern` that means one thing with the Unicode flag and another
/// without it, naming the escape. The reduction asks it of every `pattern` that
/// becomes a flag, and `patterns` asks it of every schema the document states.
pub(crate) fn unflagged(pattern: &str) -> Result<(), ScalarError> {
    match flagged_escape(pattern) {
        Some(escape) => Err(ScalarError::rule(
            pattern,
            format!(
                "holds `{escape}`, which means one thing with the Unicode flag and another \
                 without it, and a generated type cannot be given the flag"
            ),
        )),
        None => Ok(()),
    }
}

/// The escape in `pattern` that the Unicode flag gives another meaning: `\p{`,
/// `\P{` or `\u{`.
///
/// Without the flag `\p{L}` is the text `p{L}` and `\u{41}` is the letter `u`
/// forty-one times, and with it they are a property and a code point. The
/// command line and the generated type run a pattern without the flag, so a
/// vendor's server that runs it with does not agree with either. A backslash
/// takes the character after it, so `\\p{3}` is a backslash and then a `p`.
fn flagged_escape(pattern: &str) -> Option<&'static str> {
    let mut chars = pattern.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            continue;
        }
        let escaped = chars.next();
        let opens = chars.clone().next() == Some('{');
        if opens {
            match escaped {
                Some('p') => return Some(r"\p{"),
                Some('P') => return Some(r"\P{"),
                Some('u') => return Some(r"\u{"),
                Some(_) | None => {}
            }
        }
    }
    None
}

impl<T> Bounds<T> {
    /// Hold one value to every rule the document states about it.
    fn check(&self, value: T, raw: &str) -> Result<(), ScalarError>
    where
        T: Numeric,
    {
        if !self.allowed.is_empty() && !self.allowed.contains(&value) {
            return Err(ScalarError::outside(raw, &self.allowed));
        }
        if let Some(low) = self.low {
            low.floor(value)
                .map_err(|note| ScalarError::refusal(raw, &note))?;
        }
        if let Some(high) = self.high {
            high.ceiling(value)
                .map_err(|note| ScalarError::refusal(raw, &note))?;
        }
        if let Some(step) = self.multiple_of
            && !value.is_multiple_of(raw, step)
        {
            return Err(ScalarError::refusal(raw, &a_multiple_of(step)));
        }
        Ok(())
    }

    /// Whether the step, if there is one, has multiples to admit.
    fn runnable(&self) -> Result<(), ScalarError>
    where
        T: Numeric,
    {
        match self.multiple_of {
            Some(step) if !step.is_step() => Err(ScalarError::rule(
                &step.to_string(),
                "is not a step: `multipleOf` has to be more than 0".to_owned(),
            )),
            Some(_) | None => Ok(()),
        }
    }

    fn notes(&self) -> Vec<String>
    where
        T: Numeric,
    {
        let mut notes = Vec::new();
        if !self.allowed.is_empty() {
            notes.push(one_of(&self.allowed));
        }
        if let Some(low) = self.low {
            notes.push(low.note("at least", "more than"));
        }
        if let Some(high) = self.high {
            notes.push(high.note("at most", "less than"));
        }
        if let Some(step) = self.multiple_of {
            notes.push(a_multiple_of(step));
        }
        notes
    }
}

/// `multipleOf`.
fn a_multiple_of<T: fmt::Display>(step: T) -> String {
    format!("a multiple of {step}")
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
    Decimal::beside(raw, value).is_some_and(|(spelled, printed)| spelled == printed)
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
    /// The number `raw` spells and the number `float` stands for, which is the
    /// shortest text that reads back as the float, or `None` when either has no
    /// reading.
    fn beside(raw: &str, float: f64) -> Option<(Self, Self)> {
        Some((Self::of(raw)?, Self::of(&format!("{float:e}"))?))
    }

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

    /// Whether this number is a whole number of `step`s.
    ///
    /// Both are `digits × 10^exponent`, so the quotient is whole when the digits
    /// of the step divide the digits of the value once the powers of ten are
    /// lined up. Zero is a multiple of every step. The digits of a number that
    /// `exact` has accepted, and of the step of a float, are the shortest text of
    /// a float: at most 17, so they and the remainders between them fit a `u128`
    /// with room to spare, and no digit is lost to a rounding.
    fn is_multiple_of(&self, step: &Self) -> bool {
        // A step of 0 has no digits and no multiples, not even 0.
        let Ok(step_digits) = step.digits.parse::<u128>() else {
            return false;
        };
        if self.digits.is_empty() {
            return true;
        }
        let Ok(value) = self.digits.parse::<u128>() else {
            return false;
        };
        let Some(lift) = self.exponent.checked_sub(step.exponent) else {
            return false;
        };
        if lift >= 0 {
            // `value × 10^lift` is divided one digit at a time, as a long division
            // is, so the product is never written down.
            let mut rest = value % step_digits;
            for _ in 0..lift {
                rest = rest * 10 % step_digits;
            }
            rest == 0
        } else {
            // `value` is divided by `step × 10^-lift`. A divisor past a `u128` is
            // past any value, and then only zero is a multiple.
            u32::try_from(lift.unsigned_abs())
                .ok()
                .and_then(|power| 10_u128.checked_pow(power))
                .and_then(|scale| scale.checked_mul(step_digits))
                .is_some_and(|divisor| value % divisor == 0)
        }
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

    /// `multipleOf`: whether `self`, which `raw` spells, is a whole number of
    /// `step`s. No remainder panics and no float divides: a number is divisible
    /// by what its digits say.
    fn is_multiple_of(self, raw: &str, step: Self) -> bool;

    /// Whether a `multipleOf` can be this number: more than 0.
    fn is_step(self) -> bool;
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

    fn is_multiple_of(self, _raw: &str, step: Self) -> bool {
        // `i64::MIN % -1` overflows, and its remainder is 0 as it is for every
        // multiple. A step of 0 has no multiple but 0, and that is no rule.
        self.checked_rem(step).map_or(step != 0, |rest| rest == 0)
    }

    fn is_step(self) -> bool {
        self > 0
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
    // document's rule rather than an approximation of it. `0.07 / 0.01` is
    // `7.000000000000001` in a binary float and `1e17 / 3.0` is a whole number,
    // so neither the value nor the step is divided as a float: the value is
    // the digits `raw` spells, and the step the shortest text that reads back as
    // the float, which is the number the document wrote.
    fn is_multiple_of(self, raw: &str, step: Self) -> bool {
        Decimal::beside(raw, step).is_some_and(|(value, step)| value.is_multiple_of(&step))
    }

    fn is_step(self) -> bool {
        self.is_finite() && self > 0.0
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
                rule: r"is not a match for ^-?[0-9]+(\.[0-9]{1,2})?$".to_owned(),
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
                rule: "is not at least 3 characters".to_owned(),
            })
        );
        assert_eq!(
            code.parse("EURO"),
            Err(ScalarError::Rule {
                raw: "EURO".to_owned(),
                rule: "is not at most 3 characters".to_owned(),
            })
        );
    }

    #[test]
    fn an_inclusive_bound_admits_its_own_value_and_an_exclusive_one_does_not() {
        let inclusive = Scalar::Integer(Bounds {
            low: Some(Limit::Inclusive(1)),
            high: Some(Limit::Inclusive(100)),
            ..Bounds::default()
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
            ..Bounds::default()
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

    fn stepped(step: f64) -> Scalar {
        Scalar::Number(Bounds {
            multiple_of: Some(step),
            ..Bounds::default()
        })
    }

    /// `probes/A3b/scalars` (`multiple`): a binary float divides `0.07` by `0.01`
    /// to `7.000000000000001`, which is no whole number, and the same is true of 159
    /// of the 1000 amounts in cents. The value is the digits it was written with,
    /// and so is the step.
    #[test]
    fn a_decimal_step_is_decided_on_the_digits_and_not_in_a_float() {
        let cents = stepped(0.01);
        for raw in ["0.07", "0.29", "19.99"] {
            assert_eq!(
                cents.parse(raw),
                Ok(serde_json::json!(raw.parse::<f64>().unwrap_or(f64::NAN))),
                "{raw}"
            );
        }
        for cent in 0..1000_u32 {
            let raw = format!("{}.{:02}", cent / 100, cent % 100);
            assert!(cents.parse(&raw).is_ok(), "{raw}");
        }
        let tenths = stepped(0.1);
        for tenth in 0..100_u32 {
            let raw = format!("{}.{}", tenth / 10, tenth % 10);
            assert!(tenths.parse(&raw).is_ok(), "{raw}");
        }
        // The same number, spelled another way.
        for raw in ["7e-2", "+0.07", ".07", "0.070", "700e-4", "0.07E0", "-0.07"] {
            assert!(cents.parse(raw).is_ok(), "{raw}");
        }
        // What is no multiple of a cent is refused with the document's number.
        for raw in ["0.005", "0.071", "19.995", "1e-3", "0.0699999", "100.001"] {
            assert_eq!(
                cents.parse(raw),
                Err(ScalarError::Rule {
                    raw: raw.to_owned(),
                    rule: "is not a multiple of 0.01".to_owned(),
                }),
                "{raw}"
            );
        }
        // Zero is a multiple of every step, however it is spelled.
        for raw in ["0", "-0", "0.00", "0e5"] {
            assert!(cents.parse(raw).is_ok(), "{raw}");
        }
    }

    /// A quotient in a float also says yes where it should say no: `1e17 / 3.0` is
    /// `33333333333333332`, a whole number, and `1e17` is not a multiple of 3.
    #[test]
    fn a_step_is_not_met_because_a_float_rounded_the_quotient_to_a_whole_number() {
        let by_three = stepped(3.0);
        assert!(by_three.parse("100000000000000000").is_err());
        assert!(by_three.parse("9007199254740996").is_ok());
        assert!(by_three.parse("9007199254740991").is_err());
        assert!(stepped(2.0).parse("9007199254740992").is_ok());
    }

    /// The digits of a float's text are at most 17, and the powers of ten between
    /// the value and the step reach a few hundred.
    #[test]
    fn a_step_is_decided_across_the_whole_range_of_a_float() {
        let tiny = stepped(1e-300);
        assert!(tiny.parse("1e300").is_ok());
        assert!(tiny.parse("3e-300").is_ok());
        assert!(tiny.parse("1.5e-300").is_err());
        let huge = stepped(1e300);
        assert!(huge.parse("1e300").is_ok());
        assert!(huge.parse("5").is_err());
        assert!(huge.parse("0").is_ok());
        assert!(stepped(5e-324).parse("1e-323").is_ok());
        assert!(stepped(0.5).parse("1e-323").is_err());
        assert!(stepped(1.5).parse("4.5").is_ok());
        assert!(stepped(1.5).parse("-4.5").is_ok());
        assert!(stepped(1.5).parse("4.6").is_err());
    }

    /// `probes/A3b/scalars` (`panic`): `i64::MIN % -1` overflows, and the remainder
    /// of every integer by a step of 0 is a division by zero. No value makes
    /// `parse` panic. A document that states either step is refused while it is
    /// reduced (`runnable`), so these are the answers for a model nobody checked.
    #[test]
    fn no_value_makes_the_remainder_panic() {
        let by = |step: i64| {
            Scalar::Integer(Bounds {
                multiple_of: Some(step),
                ..Bounds::default()
            })
        };
        let minus_one = by(-1);
        assert_eq!(
            minus_one.parse("-9223372036854775808"),
            Ok(serde_json::json!(i64::MIN))
        );
        assert!(minus_one.parse("5").is_ok());
        let zero = by(0);
        for raw in ["0", "5", "-9223372036854775808"] {
            assert!(zero.parse(raw).is_err(), "{raw}");
        }
        // The same step as a number has no multiples either, not even 0.
        for raw in ["0", "-0", "5", "0.5"] {
            assert!(stepped(0.0).parse(raw).is_err(), "{raw}");
        }
        assert!(by(i64::MIN).parse("-9223372036854775808").is_ok());
        assert!(by(i64::MIN).parse("5").is_err());
        assert!(by(i64::MAX).parse("9223372036854775807").is_ok());
    }

    /// A step has to be more than 0, and a float has to be a number as well.
    #[test]
    fn a_step_that_is_not_more_than_zero_cannot_run() {
        let integer = |step: i64| {
            Scalar::Integer(Bounds {
                multiple_of: Some(step),
                ..Bounds::default()
            })
        };
        for (scalar, raw) in [
            (integer(0), "0"),
            (integer(-1), "-1"),
            (integer(i64::MIN), "-9223372036854775808"),
            (stepped(0.0), "0"),
            (stepped(-0.0), "-0"),
            (stepped(-0.25), "-0.25"),
            (stepped(f64::NAN), "NaN"),
            (stepped(f64::INFINITY), "inf"),
        ] {
            assert_eq!(
                scalar.runnable(),
                Err(ScalarError::Rule {
                    raw: raw.to_owned(),
                    rule: "is not a step: `multipleOf` has to be more than 0".to_owned(),
                }),
                "{raw}"
            );
        }
        for scalar in [
            integer(1),
            integer(i64::MAX),
            stepped(0.01),
            stepped(5e-324),
        ] {
            assert_eq!(scalar.runnable(), Ok(()));
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
        assert_eq!(Scalar::Boolean(None).wire("true").as_deref(), Ok("true"));
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
            Some("at least 2 characters; at most 4 characters; a match for ^[A-Z]+$")
        );
        assert!(code.parse("A").is_err());
        assert!(code.parse("ABCDE").is_err());
        assert!(code.parse("ab").is_err());
        assert!(code.parse("AB").is_ok());

        let count = Scalar::Integer(Bounds {
            low: Some(Limit::Inclusive(1)),
            high: Some(Limit::Exclusive(10)),
            multiple_of: Some(3),
            ..Bounds::default()
        });
        assert_eq!(
            count.note().as_deref(),
            Some("at least 1; less than 10; a multiple of 3")
        );
        assert_eq!(Scalar::Boolean(None).note(), None);
        assert_eq!(Scalar::Text(Text::default()).note(), None);
    }
    fn integers(allowed: &[i64]) -> Scalar {
        Scalar::Integer(Bounds {
            allowed: allowed.to_vec(),
            ..Bounds::default()
        })
    }

    fn outside(raw: &str, allowed: &[&str]) -> ScalarError {
        ScalarError::Choice {
            raw: raw.to_owned(),
            allowed: allowed.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    /// `probes/A3b/scalars` (`schemas`): `--v 4` against `enum: [1, 2, 3]` is sent
    /// and `--help` lists no value, unless the enum is a rule of the value as a
    /// bound is.
    #[test]
    fn an_integer_enum_is_enforced_and_noted() {
        let one_of = integers(&[1, 2, 3]);
        for raw in ["1", "2", "3", "+2", "002"] {
            assert!(one_of.parse(raw).is_ok(), "{raw}");
        }
        assert_eq!(one_of.parse("4"), Err(outside("4", &["1", "2", "3"])));
        assert_eq!(
            refusal(&one_of, "4").to_string(),
            "`4` is not one of 1, 2, 3"
        );
        assert!(matches!(
            one_of.parse("2.0"),
            Err(ScalarError::Kind {
                wanted: "an integer",
                ..
            })
        ));
        assert_eq!(one_of.note().as_deref(), Some("one of 1, 2, 3"));
        // The text that is sent is the value that was admitted.
        assert_eq!(one_of.wire("+2").as_deref(), Ok("2"));
        assert_eq!(one_of.wire("4"), Err(outside("4", &["1", "2", "3"])));
    }

    #[test]
    fn a_number_enum_admits_the_value_and_not_the_spelling() {
        let ratio = Scalar::Number(Bounds {
            allowed: vec![0.5, 1.5, 100.0],
            ..Bounds::default()
        });
        for raw in ["0.5", "5e-1", ".5", "1.50", "1e2", "100"] {
            assert!(ratio.parse(raw).is_ok(), "{raw}");
        }
        assert_eq!(ratio.parse("1"), Err(outside("1", &["0.5", "1.5", "100"])));
        assert_eq!(ratio.note().as_deref(), Some("one of 0.5, 1.5, 100"));
    }

    /// An enum is one rule among the others. All of them are enforced, and each is
    /// in the note, the enum first.
    #[test]
    fn an_enum_beside_a_bound_keeps_both() {
        let both = Scalar::Integer(Bounds {
            low: Some(Limit::Inclusive(2)),
            allowed: vec![1, 2, 3],
            ..Bounds::default()
        });
        assert!(both.parse("2").is_ok());
        assert!(both.parse("1").is_err(), "inside the enum, below the bound");
        assert!(
            both.parse("5").is_err(),
            "above the bound, outside the enum"
        );
        assert_eq!(both.note().as_deref(), Some("one of 1, 2, 3; at least 2"));
    }

    /// The `enum: [true]` of `A7/docs/boolenum.yaml`: `--v false` is sent unless the
    /// enum is a rule.
    #[test]
    fn a_boolean_enum_admits_the_one_value_it_lists() {
        let only = Scalar::Boolean(Some(true));
        assert_eq!(only.parse("true"), Ok(serde_json::json!(true)));
        assert_eq!(only.parse("false"), Err(outside("false", &["true"])));
        assert_eq!(only.note().as_deref(), Some("one of true"));
        assert_eq!(only.wire("false"), Err(outside("false", &["true"])));
        // The kind is asked first, so a word that is no boolean is none.
        assert!(matches!(
            only.parse("yes"),
            Err(ScalarError::Kind {
                wanted: "`true` or `false`",
                ..
            })
        ));

        let either = Scalar::Boolean(None);
        assert_eq!(either.parse("false"), Ok(serde_json::json!(false)));
        assert_eq!(either.note(), None);
        for raw in ["yes", "TRUE", "1", "", "True"] {
            assert!(
                matches!(
                    either.parse(raw),
                    Err(ScalarError::Kind {
                        wanted: "`true` or `false`",
                        ..
                    })
                ),
                "{raw}"
            );
        }
    }

    /// `minItems`, `maxItems` and `uniqueItems` are one value with one check and
    /// one note. The refusal names the list as it was given.
    #[test]
    fn a_list_is_held_to_its_count_and_to_its_uniqueness() {
        let ids = Scalar::Integer(Bounds::default());
        let items = Items {
            min: Some(2),
            max: Some(3),
            unique: true,
        };
        let words = |raws: &[&str]| match items.check(&ids, raws) {
            Ok(()) => None,
            Err(ScalarError::Rule { rule, .. }) => Some(rule),
            Err(other) => panic!("{other:?}"),
        };
        assert_eq!(words(&["1", "2"]), None);
        assert_eq!(words(&["1", "2", "3"]), None);
        assert_eq!(
            words(&["1"]).as_deref(),
            Some("is not a list of at least 2 values")
        );
        assert_eq!(
            words(&["1", "2", "3", "4"]).as_deref(),
            Some("is not a list of at most 3 values")
        );
        assert_eq!(
            words(&["1", "2", "1"]).as_deref(),
            Some("is not a list of different values")
        );
        assert_eq!(
            items.check(&ids, &["1", "2", "1"]),
            Err(ScalarError::Rule {
                raw: "1, 2, 1".to_owned(),
                rule: "is not a list of different values".to_owned(),
            })
        );
        assert_eq!(
            items.note().as_deref(),
            Some("at least 2 values; at most 3 values; different values")
        );
    }

    /// Two values are the same when the rule reads them as the same value.
    #[test]
    fn two_spellings_of_one_value_are_one_value_to_a_list() {
        let unique = Items {
            unique: true,
            ..Items::default()
        };
        let integers = Scalar::Integer(Bounds::default());
        assert!(unique.check(&integers, &["1", "01"]).is_err());
        assert!(unique.check(&integers, &["1", "+1"]).is_err());
        assert!(unique.check(&integers, &["1", "2"]).is_ok());
        let numbers = Scalar::Number(Bounds::default());
        assert!(unique.check(&numbers, &["1", "1.0"]).is_err());
        assert!(unique.check(&numbers, &["0", "-0"]).is_err());
        assert!(unique.check(&numbers, &["0.1", "0.10"]).is_err());
        assert!(unique.check(&numbers, &["0.1", "0.2"]).is_ok());
        let text = Scalar::Text(Text::default());
        assert!(unique.check(&text, &["a", "A", " a"]).is_ok());
        assert!(unique.check(&text, &["a", "b", "a"]).is_err());
        assert!(
            unique
                .check(&Scalar::Boolean(None), &["true", "true"])
                .is_err()
        );
    }

    /// A list of one value is a list, and its note says "1 value".
    #[test]
    fn a_list_rule_counts_one_value_in_the_singular() {
        let items = Items {
            min: Some(1),
            max: Some(1),
            unique: false,
        };
        assert_eq!(
            items.note().as_deref(),
            Some("at least 1 value; at most 1 value")
        );
        let text = Scalar::Text(Text::default());
        assert!(items.check(&text, &["a"]).is_ok());
        assert!(items.check(&text, &["a", "b"]).is_err());
        assert!(Items::default().check(&text, &[]).is_ok());
        assert_eq!(Items::default().note(), None);
    }

    /// A `minItems` of 0 asks for nothing, so the help line carries no note for it
    /// and every list meets it.
    #[test]
    fn a_list_of_at_least_no_values_states_no_rule() {
        let items = Items {
            min: Some(0),
            ..Items::default()
        };
        assert_eq!(items.note(), None);
        assert!(items.check(&Scalar::Text(Text::default()), &[]).is_ok());
        let capped = Items {
            max: Some(2),
            ..items
        };
        assert_eq!(capped.note().as_deref(), Some("at most 2 values"));
    }

    /// A value that its own rule refuses is refused as that value, so a list
    /// checked on its own meets the same words that the value had alone.
    #[test]
    fn a_list_with_a_value_the_rule_refuses_is_refused_for_that_value() {
        let unique = Items {
            unique: true,
            ..Items::default()
        };
        assert_eq!(
            unique.check(&integers(&[1, 2]), &["1", "7"]),
            Err(outside("7", &["1", "2"]))
        );
    }

    /// An integer that states the one rule given: a low end, a high end, or a step.
    fn integer_with(
        low: Option<Limit<i64>>,
        high: Option<Limit<i64>>,
        multiple_of: Option<i64>,
    ) -> Scalar {
        Scalar::Integer(Bounds {
            low,
            high,
            multiple_of,
            ..Bounds::default()
        })
    }

    /// One scalar for each rule it can state, and a value that breaks it.
    fn one_rule_each() -> Vec<(Scalar, &'static str)> {
        let lengths = |min_length, max_length| {
            Scalar::Text(Text {
                min_length,
                max_length,
                ..Text::default()
            })
        };
        vec![
            (lengths(Some(3), None), "ab"),
            (lengths(None, Some(3)), "abcd"),
            (text("^[0-9]+$"), "x"),
            (integer_with(Some(Limit::Inclusive(1)), None, None), "0"),
            (integer_with(Some(Limit::Exclusive(1)), None, None), "1"),
            (integer_with(None, Some(Limit::Inclusive(9)), None), "10"),
            (integer_with(None, Some(Limit::Exclusive(9)), None), "9"),
            (integer_with(None, None, Some(5)), "7"),
            (stepped(0.25), "0.3"),
            (integers(&[100, 200]), "7"),
            (
                Scalar::Number(Bounds {
                    allowed: vec![0.5, 1.5],
                    ..Bounds::default()
                }),
                "1",
            ),
            (Scalar::Boolean(Some(false)), "true"),
        ]
    }

    /// F-82: a refusal is "is not" and the note of the rule it enforces, whichever
    /// the rule, so `--help` and the refusal cannot describe two rules.
    #[test]
    fn every_refusal_is_built_from_its_note() {
        for (scalar, raw) in one_rule_each() {
            let Some(note) = scalar.note() else {
                panic!("{scalar:?} states a rule and renders none");
            };
            assert_eq!(
                refusal(&scalar, raw).to_string(),
                format!("`{raw}` is not {note}"),
                "{scalar:?}"
            );
        }
    }

    /// A list is a list of what its note says.
    #[test]
    fn the_refusal_of_a_list_is_built_from_its_note() {
        let items = Items {
            min: Some(2),
            ..Items::default()
        };
        let Some(note) = items.note() else {
            panic!("{items:?} states a rule and renders none");
        };
        let Err(refused) = items.check(&Scalar::Text(Text::default()), &["a"]) else {
            panic!("one value is fewer than two");
        };
        assert_eq!(refused.to_string(), format!("`a` is not a list of {note}"));
    }

    /// `enum` on a string is the one rule that has no note: the flag lists its
    /// values, and they complete. Its refusal says the same list.
    #[test]
    fn a_string_enum_is_listed_by_the_flag_and_refused_with_the_list() {
        let status = Scalar::Choice(vec!["draft".to_owned(), "paid".to_owned()]);
        assert_eq!(status.note(), None);
        assert_eq!(
            refusal(&status, "void").to_string(),
            "`void` is not one of draft, paid"
        );
    }

    /// The scalars a document states with a rule that cannot run are told apart
    /// from those it states with a rule that does.
    #[test]
    fn a_pattern_with_an_escape_the_unicode_flag_changes_cannot_run() {
        for (pattern, escape) in [
            (r"^\p{L}+$", r"\p{"),
            (r"^\P{L}+$", r"\P{"),
            (r"^\u{41}$", r"\u{"),
            (r"[\p{L}\d]", r"\p{"),
            (r"\\\p{L}", r"\p{"),
        ] {
            assert_eq!(
                text(pattern).runnable(),
                Err(ScalarError::Rule {
                    raw: pattern.to_owned(),
                    rule: format!(
                        "holds `{escape}`, which means one thing with the Unicode flag and \
                         another without it, and a generated type cannot be given the flag"
                    ),
                }),
                "{pattern}"
            );
        }
        for pattern in [r"^\\p{3}$", r"^p\{L\}$", r"^\u0041$", r"^\d+\p$", r"^\\$"] {
            assert_eq!(text(pattern).runnable(), Ok(()), "{pattern}");
        }
        // A pattern the engine cannot read is that, whatever it holds.
        for pattern in [r"[\p{L}", r"\"] {
            assert!(
                matches!(text(pattern).runnable(), Err(ScalarError::Pattern { .. })),
                "{pattern}"
            );
        }
    }
}
