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
use typed_openapi::tree::{self, Asked, Outcome, Selection};
use typed_openapi::{Payload, SyncClient, render};

use crate::app::Error;
use crate::output::Output;

/// One operation, straight from the document.
pub fn run<C: SyncClient>(api: &Api, client: &C, matches: &ArgMatches) -> Result<Output, Error> {
    let selected = match tree::select(api.document(), matches)? {
        Asked::Run(selection) => selection,
        Asked::Template(template) => return Ok(shape(template)),
    };
    vet_body(&selected)?;
    Ok(report(selected.send(client, api.base())?))
}

/// The skeleton of a body, on stdout with the sentence about it on stderr.
///
/// Which stream each half goes to is the whole point of answering this as text
/// rather than as help: `toy raw contacts create --json-body-template >
/// body.json` leaves a file holding JSON and nothing else, ready to fill in and
/// hand back to `--json-body`.
fn shape(template: &str) -> Output {
    Output::note(
        format!("{template}\n"),
        "the shape of the body: nothing was sent. Fill it in and pass it to --json-body.\n",
    )
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
        // `select` answers this before a `Selection` exists, so nothing `send`
        // hands back carries one. The arm is here because `Outcome` is also
        // what `tree::dispatch` returns, where the same answer arrives without
        // a seam to catch it in — `examples/root.rs` is that CLI.
        Outcome::Template(template) => shape(&template),
    }
}
