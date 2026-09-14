//! Every operation in the document, mounted as the CLI itself.
//!
//! There is no `raw` layer here, no hand-written verb and no dispatch of its
//! own: `tree::commands` builds the subcommands and `tree::dispatch` runs
//! whichever one the user typed. That is the whole CLI, and it is what an
//! adopter writes on the first day — the `toy` binary beside this file is the
//! same API once there is something to put *next to* the generated operations.
//!
//! ```text
//! cargo run -p cli --example root -- --help
//! cargo run -p cli --example root -- vouchers get --id 5
//! cargo run -p cli --example root -- vouchers create --total 12.50 --currency EUR --status open
//! ```

use std::process::ExitCode;

use clap::Command;
use clap_complete::CompleteEnv;
use cli::client::Ureq;
use typed_openapi::tree::{self, Outcome};
use typed_openapi::{Document, render};

fn main() -> ExitCode {
    match run() {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("toy: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The tree, built twice from one function: once for the shell to ask what
/// completes, once to parse what the user typed. Both see the same document,
/// so what the shell offers is always what the CLI accepts.
fn root(doc: &Document) -> Command {
    Command::new("toy")
        .about("Every operation in the document, mounted as the CLI itself")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommands(tree::commands(doc))
}

fn run() -> Result<String, Box<dyn std::error::Error>> {
    let api = api::Api::new()?;

    // Dynamic completion: the shell asks this binary what completes, so the
    // document's own enums arrive as choices and there is nothing to
    // regenerate on the user's machine. `complete()` returns at once unless
    // the shell set the environment variable that asks for a completion.
    CompleteEnv::with_factory(|| root(api.document())).complete();

    let matches = root(api.document()).get_matches();
    Ok(
        match tree::dispatch(api.document(), api.base(), &Ureq::new(), &matches)? {
            Outcome::Sent(response) => String::from_utf8_lossy(response.body()).into_owned(),
            Outcome::DryRun(request) => format!(
                "{}\ndry run: nothing was sent. Add --commit to send it.\n",
                render(&request)
            ),
        },
    )
}
