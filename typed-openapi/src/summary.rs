//! What the reduction did, counted off the reduction itself.
//!
//! Every adoption counts something about its own API — how many operations
//! there are, how many of them write, which stand behind which named gate, how
//! many bodies a command line cannot take apart — and writes the number into a
//! doc comment, a README or a reference page. The number is true the day it is
//! written and silent every day after: nothing re-counts it when an Overlay
//! adds an operation, a vendor revision withdraws one, or a gate moves. A page
//! quoting a number fails nothing when the number moves, which is what makes
//! the mistake cheap to make and expensive to find.
//!
//! [`Summary`] is that count taken off the model instead of remembered beside
//! it. Nothing here is stored: every field is derived from the [`Document`]'s
//! own operations each time it is asked, so the count and the model it counts
//! cannot come apart — a stored one could. A bless step renders a summary to a
//! page beside the blob, under the same header every generated file carries, so
//! an adopter quotes a generated fact and a test asserts against the value the
//! page was rendered from.
//!
//! The two consumers want different things from one measurement. A test wants
//! values, and [`Summary`]'s accessors are those. A page wants prose, and
//! [`Summary`]'s [`Display`](std::fmt::Display) is that. Both come off one
//! `Summary`, so the sentence a reader is shown and the number a test asserts
//! are the same count.
//!
//! Deriving is also what keeps the blob the size it is. A summary is answerable
//! from the operations already in it, so storing one would add bytes to every
//! shipped binary to hold a number the binary can compute — and would introduce
//! the one thing this module exists to prevent, a written-down count that the
//! thing it counts can move out from under.

use std::collections::BTreeMap;
use std::fmt;

use crate::model::{Effect, Gate, Shape, Unsupported};
use crate::names::CommandName;
use crate::{Document, Operation};

/// A statement of what reducing one document produced.
///
/// Every number is measured off the operations the reduction holds. Take one
/// with [`Document::summary`], assert against its accessors, or render it with
/// `to_string` — the `Display` is the page a bless step commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    operations: usize,
    groups: Vec<CommandName>,
    reads: usize,
    writes: usize,
    gates: BTreeMap<Gate, usize>,
    bodiless: usize,
    whole_bodies: usize,
    unreachable: Vec<Unreachable>,
}

/// One parameter the reduction carried without a flag, and why.
///
/// A count of these would say how many values a command line cannot supply and
/// not which, which is the half an adopter acts on: the repair is an Overlay
/// naming that operation's parameter. So the exception list names its subjects
/// rather than tallying them — a reader can re-count it, which is what makes it
/// a measurement and not a claim — and [`Summary::unreachable`] is where its
/// length is a count like any other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreachable {
    operation: String,
    parameter: String,
    why: Unsupported,
}

impl Unreachable {
    /// The `operationId` the parameter is declared on, as the document spells
    /// it.
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// The parameter's wire name, as the document spells it.
    #[must_use]
    pub fn parameter(&self) -> &str {
        &self.parameter
    }

    /// The shape this crate has no spelling for. It renders as the sentence the
    /// subcommand's long help gives, so the page and the `--help` say one thing.
    #[must_use]
    pub fn why(&self) -> &Unsupported {
        &self.why
    }
}

impl Document {
    /// What this reduction did, counted off its own operations.
    ///
    /// Nothing is read out of the document a second time and nothing is stored:
    /// the answer is derived from the operations this `Document` holds, so it is
    /// the same answer whether the model was just loaded or came back off a
    /// blob, and it moves the moment the document does.
    #[must_use]
    pub fn summary(&self) -> Summary {
        Summary::of(self)
    }
}

impl Summary {
    fn of(document: &Document) -> Self {
        let ops = document.operations();
        // One pass with an exhaustive match rather than two filters: a third
        // `Effect` would be a compile error here, where two independent counts
        // would silently stop adding up to the operation count.
        let (reads, writes) = ops
            .iter()
            .fold((0, 0), |(reads, writes), op| match op.effect() {
                Effect::Read => (reads + 1, writes),
                Effect::Write => (reads, writes + 1),
            });
        Self {
            operations: ops.len(),
            groups: groups(ops),
            reads,
            writes,
            gates: gates(ops),
            bodiless: ops.iter().filter(|op| !op.body().present()).count(),
            whole_bodies: ops.iter().filter(|op| op.body().whole()).count(),
            unreachable: unreachable(ops),
        }
    }

    /// Every operation the document describes.
    #[must_use]
    pub fn operations(&self) -> usize {
        self.operations
    }

    /// The groups the operations are mounted under, once each and in the order
    /// a user meets them on a command line.
    #[must_use]
    pub fn groups(&self) -> &[CommandName] {
        &self.groups
    }

    /// Operations a CLI sends on sight.
    #[must_use]
    pub fn reads(&self) -> usize {
        self.reads
    }

    /// Operations held behind `--commit`. With [`Summary::reads`] this is every
    /// operation, because the gate is default-closed and an operation is one or
    /// the other.
    #[must_use]
    pub fn writes(&self) -> usize {
        self.writes
    }

    /// How many operations stand behind each named gate, by gate.
    ///
    /// A map rather than a count per name the caller has to know in advance: a
    /// gate an Overlay invents costs this crate no release, so the set of names
    /// is the document's and appears here the moment it is reduced.
    #[must_use]
    pub fn gates(&self) -> &BTreeMap<Gate, usize> {
        &self.gates
    }

    /// How many operations stand behind one named gate, and none for a word the
    /// document names no gate with.
    #[must_use]
    pub fn behind(&self, gate: &str) -> usize {
        self.gates
            .iter()
            .find(|(named, _)| named.as_str() == gate)
            .map_or(0, |(_, behind)| *behind)
    }

    /// Operations the document asks no request body of.
    #[must_use]
    pub fn bodiless(&self) -> usize {
        self.bodiless
    }

    /// Operations whose body goes out whole, with no flag per property — a
    /// nested JSON body, a multipart upload, a media type this CLI does not
    /// assemble. What an adopter hands one of these is a whole document.
    #[must_use]
    pub fn whole_bodies(&self) -> usize {
        self.whole_bodies
    }

    /// Every parameter carried without a flag, named. Its length is the count.
    #[must_use]
    pub fn unreachable(&self) -> &[Unreachable] {
        &self.unreachable
    }

    /// The counts that are one number each, in the words the rendered page
    /// states them in.
    ///
    /// One place decides how a count is worded, so the page and anything else
    /// that renders a summary describe the same measurement the same way.
    fn tally(&self) -> [(&'static str, usize); 7] {
        [
            ("operations", self.operations),
            ("groups they are mounted under", self.groups.len()),
            ("reads, sent on sight", self.reads),
            ("writes, held behind `--commit`", self.writes),
            ("operations the document asks no body of", self.bodiless),
            (
                "bodies that go out whole, with no flag per property",
                self.whole_bodies,
            ),
            ("parameters carried without a flag", self.unreachable.len()),
        ]
    }
}

/// The groups the operations are mounted under, once each and in document
/// order — which is the order a user meets them, because the command tree is
/// built from this same list.
fn groups(ops: &[Operation]) -> Vec<CommandName> {
    let mut named: Vec<CommandName> = Vec::new();
    for group in ops.iter().map(Operation::group) {
        if !named.contains(group) {
            named.push(group.clone());
        }
    }
    named
}

/// How many operations stand behind each gate any of them names.
fn gates(ops: &[Operation]) -> BTreeMap<Gate, usize> {
    let mut behind: BTreeMap<Gate, usize> = BTreeMap::new();
    for gate in ops.iter().flat_map(Operation::gates) {
        *behind.entry(gate.clone()).or_default() += 1;
    }
    behind
}

/// Every parameter the reduction carried without a flag, in document order.
fn unreachable(ops: &[Operation]) -> Vec<Unreachable> {
    ops.iter()
        .flat_map(|op| {
            op.params()
                .iter()
                .filter_map(move |param| match param.shape() {
                    Shape::Flag { .. } => None,
                    Shape::Unreachable(why) => Some(Unreachable {
                        operation: op.id().to_owned(),
                        parameter: param.name().to_owned(),
                        why: why.clone(),
                    }),
                })
        })
        .collect()
}

/// The page a bless step commits, and the one a test parses.
///
/// Markdown, because the reader it is written for is a human looking at a
/// `docs/` page or a README — and because a page that quotes this one can quote
/// a row rather than retyping a number. What it does not carry is where it came
/// from: a `Summary` in hand has the document beside it, and the detached file
/// gets that header from the generator, which is the half that knows which
/// documents were read and what command reads them again.
impl fmt::Display for Summary {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(out, "# What the reduction found\n")?;
        writeln!(
            out,
            "Every number here is counted off the reduced model itself — the same value \
             a shipped\nbinary loads — so a number that has moved is a document that \
             has moved, and a page\nquoting one is quoting a measurement rather than a \
             memory.\n"
        )?;
        writeln!(out, "| what | how many |")?;
        writeln!(out, "|---|---|")?;
        for (what, how_many) in self.tally() {
            writeln!(out, "| {what} | {how_many} |")?;
        }
        self.groups_section(out)?;
        self.gates_section(out)?;
        self.unreachable_section(out)
    }
}

impl Summary {
    fn groups_section(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(out, "\n## Groups\n")?;
        if self.groups.is_empty() {
            return writeln!(out, "The document describes no operations.");
        }
        let named: Vec<String> = self
            .groups
            .iter()
            .map(|group| format!("`{group}`"))
            .collect();
        writeln!(out, "{}", named.join(", "))
    }

    fn gates_section(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(out, "\n## Gates\n")?;
        if self.gates.is_empty() {
            return writeln!(
                out,
                "No operation names a gate, so `--commit` is the whole of the confirmation."
            );
        }
        writeln!(out, "| gate | operations behind it |")?;
        writeln!(out, "|---|---|")?;
        for (gate, behind) in &self.gates {
            writeln!(out, "| `--{gate}` | {behind} |")?;
        }
        Ok(())
    }

    fn unreachable_section(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(out, "\n## Parameters carried without a flag\n")?;
        if self.unreachable.is_empty() {
            return writeln!(
                out,
                "None: every parameter the document declares reaches the request."
            );
        }
        writeln!(out, "| operation | parameter | why |")?;
        writeln!(out, "|---|---|---|")?;
        for carried in &self.unreachable {
            writeln!(
                out,
                "| `{}` | `{}` | {} |",
                carried.operation, carried.parameter, carried.why
            )?;
        }
        Ok(())
    }
}
