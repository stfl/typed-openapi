//! The gate: whether a request may be sent, decided once.
//!
//! Every caller that has an [`Invocation`] and what the user answered builds a
//! [`Plan`] and executes what it gets back, rather than testing the flags again.
//! That is what makes a dry run print the exact bytes a confirmed run sends —
//! they are this one value. The plan builds the request itself, from the
//! invocation, so the operation the gate reads and the request it carries are
//! one operation: no caller can put a write's request beside a read's verdict.
//!
//! What the user answered is an [`Answers`]: the write confirmation, plus one
//! answer per gate the operation names. Both halves are demanded together, so
//! an operation that stands behind `enshrine` needs the confirmation *and* that
//! word, and neither substitutes for the other.
//!
//! Nothing here knows about command lines. A CLI reaches it through
//! the `tree` module (feature `clap`), and a Rust caller that wants the same gate over a typed
//! wrapper calls [`Call::plan`], which is what the `finalize-voucher` verb in the
//! example does.
//!
//! [`Call::plan`]: crate::client::Call::plan

use std::collections::BTreeSet;

use http::{Request, Uri};
use thiserror::Error;

use crate::model::{Effect, Gate, Operation};
use crate::request::{Invocation, RequestError, ValueError};
use crate::values::Values;

/// What a caller answered at the gate: the write confirmation, and one answer
/// per named gate the operation carries.
///
/// Default-closed in both halves, because a question nobody was asked is a
/// question nobody answered: an `Answers` built and never spoken to holds every
/// write back, and answering a gate no operation names opens nothing. Adding an
/// answer can only ever let a request through — never take one back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answers {
    commit: bool,
    gates: BTreeSet<String>,
}

impl Answers {
    /// Nothing answered: what a read is put to the gate with, and what every
    /// write is refused by.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The write confirmation — `--commit`, on a command line.
    #[must_use]
    pub fn commit(mut self) -> Self {
        self.commit = true;
        self
    }

    /// One named gate, answered.
    ///
    /// A name is taken as it is given rather than checked against a document:
    /// which gates exist is the operation's say, so a name no operation carries
    /// opens nothing and is not an error.
    #[must_use]
    pub fn gate(mut self, name: impl Into<String>) -> Self {
        self.gates.insert(name.into());
        self
    }

    /// Was the write confirmed?
    #[must_use]
    pub fn committed(&self) -> bool {
        self.commit
    }

    /// Was this gate answered?
    #[must_use]
    pub fn answered(&self, gate: &Gate) -> bool {
        self.gates.contains(gate.as_str())
    }
}

/// What the gate decided, with the request already built.
///
/// This is the only place in the crate that decides whether something is sent.
#[derive(Debug, Clone)]
pub enum Plan {
    /// A read, or a write the user confirmed.
    Send(Request<Vec<u8>>),
    /// A write without confirmation. Print it; send nothing.
    DryRun(Request<Vec<u8>>),
}

impl Plan {
    /// The gate: a read runs on sight, a write runs once it is confirmed and
    /// every gate it names is answered.
    ///
    /// The request is built here, from the invocation that holds the operation
    /// the gate reads, so the verdict and the request are about one operation.
    /// A plan cannot be made of an operation and a request that came from
    /// another:
    ///
    /// ```compile_fail,E0308
    /// use typed_openapi::{Answers, Operation, Plan};
    ///
    /// fn pair(read: &Operation, write_request: http::Request<Vec<u8>>) -> Plan {
    ///     // A write's request with a read's verdict, which is `Send`.
    ///     Plan::decide(read, &Answers::new(), write_request)
    /// }
    /// ```
    ///
    /// The one failure is the request: a base that holds userinfo or a query, or
    /// a header name that the `http` crate refuses.
    pub fn decide(
        invocation: &Invocation<'_>,
        base: &Uri,
        answers: &Answers,
    ) -> Result<Self, RequestError> {
        let op = invocation.operation();
        let request = invocation.request(base)?;
        Ok(match (op.effect(), opened(op, answers)) {
            (Effect::Read, _) | (Effect::Write, true) => Self::Send(request),
            (Effect::Write, false) => Self::DryRun(request),
        })
    }

    /// Values that satisfy an operation, rendered against a server and put to
    /// the gate — validation, rendering and the verdict in one step, so no
    /// caller can do two of the three and skip the last.
    pub fn build(
        op: &Operation,
        base: &Uri,
        values: Values,
        answers: &Answers,
    ) -> Result<Self, PlanError> {
        let invocation = Invocation::new(op, values)?;
        Ok(Self::decide(&invocation, base, answers)?)
    }

    /// The request, whichever way the gate went — a dry run prints exactly the
    /// bytes a confirmed run sends, because they are this one value.
    #[must_use]
    pub fn request(&self) -> &Request<Vec<u8>> {
        match self {
            Self::Send(request) | Self::DryRun(request) => request,
        }
    }
}

/// Is every question this operation asks answered?
///
/// The confirmation *and* each gate the document names, all of them together: a
/// gate nobody answered can only hold a request back, never let one through.
fn opened(op: &Operation, answers: &Answers) -> bool {
    answers.committed() && op.gates().iter().all(|gate| answers.answered(gate))
}

/// Why values that parsed did not turn into a request.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PlanError {
    /// The values do not satisfy the operation. The message names the
    /// parameter, or says the body is the problem.
    #[error(transparent)]
    Value(#[from] ValueError),
    /// The values satisfied the operation and the request they render to could
    /// not be built: the base URL holds userinfo or a query, or the `http` crate
    /// refused what the document names, such as a header name.
    #[error(transparent)]
    Request(#[from] RequestError),
}
