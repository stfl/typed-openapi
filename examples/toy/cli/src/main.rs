//! The toy accounting CLI.
//!
//! The whole command tree comes from `api`'s embedded document at startup —
//! the same bytes the generated types and wrappers were emitted from. Adding an
//! operation to the vendor's document or to the adopter's Overlay adds a
//! subcommand; nothing in this file changes.

use std::io::Write as _;
use std::process::ExitCode;

use clap_complete::CompleteEnv;
use cli::app;
use cli::client::Ureq;
use cli::output::Output;

fn main() -> ExitCode {
    match run() {
        Ok(output) => print(&output),
        Err(error) => {
            eprintln!("toy: {error}");
            // The cause is where the detail is — which field of a `--json-body`
            // file did not fit, which byte of a path was not a URL — and an
            // agent reading stderr cannot ask for it afterwards.
            let mut cause = error.source();
            while let Some(next) = cause {
                eprintln!("  caused by: {next}");
                cause = next.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Output, Box<dyn std::error::Error>> {
    let api = api::Api::new()?;

    // Dynamic completion: the shell asks this binary what completes, so what
    // the shell offers is always what the document describes — no regeneration
    // on the user's machine, and nothing to go stale.
    CompleteEnv::with_factory(|| app::root(&api)).complete();

    let matches = app::root(&api).get_matches();
    Ok(app::run(api, &Ureq::new(), &matches)?)
}

fn print(output: &Output) -> ExitCode {
    if !output.stdout.is_empty() {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(output.stdout.as_bytes());
    }
    if !output.stderr.is_empty() {
        eprint!("{}", output.stderr);
    }
    if output.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
