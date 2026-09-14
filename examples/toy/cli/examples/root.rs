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
//! cargo run -p cli --example root -- get-voucher --id 5
//! cargo run -p cli --example root -- create-voucher --total 12.50 --currency EUR
//! ```

use std::process::ExitCode;

use clap::Command;
use cli::client::Ureq;
use typed_openapi::render;
use typed_openapi::tree::{self, Outcome};

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

fn run() -> Result<String, Box<dyn std::error::Error>> {
    let api = api::Api::new()?;
    let matches = Command::new("toy")
        .about("Every operation in the document, mounted as the CLI itself")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommands(tree::commands(api.document()))
        .get_matches();

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
