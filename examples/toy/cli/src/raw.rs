//! `toy raw <operation>` — the generated surface, one subcommand per operation.
//!
//! The whole trip is [`typed_openapi::tree`]'s: the subcommand name becomes an
//! operation, the flags become values under the document's own names, the gate
//! answers, and the request is sent or printed. What is left in this file is
//! the one thing a library cannot do — hold the body to the generated type that
//! operation's wrapper takes, which is a type this crate can name and
//! `typed-openapi` cannot.
//!
//! An adopter with no such check writes [`tree::dispatch`] instead and has no
//! module here at all; `examples/root.rs` is that CLI in full.

use api::{Api, OperationId};
use clap::ArgMatches;
use typed_openapi::tree::{self, Outcome, Selection};
use typed_openapi::{Payload, SyncClient, render};

use crate::app::Error;
use crate::output::Output;

/// One operation, straight from the document.
pub fn run<C: SyncClient>(api: &Api, client: &C, matches: &ArgMatches) -> Result<Output, Error> {
    let selected = tree::select(api.document(), matches)?;
    vet_body(&selected)?;
    Ok(report(selected.send(client, api.base())?))
}

/// Hold a JSON body to the generated type the operation takes, before anything
/// is sent.
///
/// This is why the seam exists. `typed-openapi` built the body out of flags and
/// files and knows it is JSON; only this crate knows which `struct` that JSON
/// has to be, so the check belongs on this side of [`tree::select`].
fn vet_body(selected: &Selection<'_>) -> Result<(), Error> {
    let Some(Payload::Json(body)) = selected.values().payload() else {
        return Ok(());
    };
    let op = selected.operation();
    // The operation came out of the same document the wrappers were generated
    // from, so the inventory has it; saying so costs one line and beats an
    // unwrap that would be a panic if that ever stopped being true.
    let id = OperationId::from_command(op.group().as_str(), op.command().as_str())
        .ok_or_else(|| Error::Unknown(format!("{} {}", op.group(), op.command())))?;
    Ok(id.check_body(body)?)
}

/// Render what the gate did. Nothing here decides anything.
fn report(outcome: Outcome) -> Output {
    match outcome {
        Outcome::Sent(response) => Output::answered(&response),
        Outcome::DryRun(request) => Output::note(
            render(&request),
            "dry run: nothing was sent. Add --commit to send it.\n",
        ),
    }
}
