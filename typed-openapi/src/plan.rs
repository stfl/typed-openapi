//! The gate: whether a built request may be sent, decided once.
//!
//! Every caller that has a request and a confirmation builds a [`Plan`] and
//! executes what it gets back, rather than testing the flag again. That is what
//! makes a dry run print the exact bytes a confirmed run sends — they are this
//! one value.
//!
//! Nothing here knows about command lines. A CLI reaches it through
//! the `tree` module (feature `clap`), and a Rust caller that wants the same gate over a typed
//! wrapper calls [`Plan::decide`] directly, which is what the `finalize-voucher`
//! verb in the example does.

use http::{Request, Uri};
use thiserror::Error;

use crate::model::{Effect, Operation};
use crate::request::{Invocation, ValueError};
use crate::values::Values;

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
    /// The gate: a read runs on sight, a write runs only once confirmed.
    #[must_use]
    pub fn decide(effect: Effect, confirmed: bool, request: Request<Vec<u8>>) -> Self {
        match (effect, confirmed) {
            (Effect::Read, _) | (Effect::Write, true) => Self::Send(request),
            (Effect::Write, false) => Self::DryRun(request),
        }
    }

    /// Values that satisfy an operation, rendered against a server and put to
    /// the gate — validation, rendering and the verdict in one step, so no
    /// caller can do two of the three and skip the last.
    pub fn build(
        op: &Operation,
        base: &Uri,
        values: Values,
        confirmed: bool,
    ) -> Result<Self, PlanError> {
        let invocation = Invocation::new(op, values)?;
        Ok(Self::decide(
            op.effect(),
            confirmed,
            invocation.request(base)?,
        ))
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

/// Why values that parsed did not turn into a request.
#[derive(Debug, Error)]
pub enum PlanError {
    #[error(transparent)]
    Value(#[from] ValueError),
    #[error("cannot build the request: {0}")]
    Request(#[from] http::Error),
}
