//! The command tree, and which module runs each command in it.
//!
//! `run` is generic over the HTTP client and hands back an [`Output`] rather
//! than printing it, so every test in `tests/` drives the real thing with a
//! recording client and no socket.

use api::{Api, OperationId};
use clap::{Arg, ArgMatches, Command, value_parser};
use clap_complete::aot::Shell;
use http::Uri;
use thiserror::Error;
use typed_openapi::SyncClient;
use typed_openapi::tree::{self, DispatchError};

use crate::output::Output;
use crate::{finalize, raw};

pub const BASE_URL: &str = "base-url";
pub const RAW: &str = "raw";
pub const FINALIZE: &str = "finalize-voucher";
pub const COMPLETIONS: &str = "completions";
pub const ID: &str = "id";
pub const SHELL: &str = "shell";

/// Anything one invocation can fail with.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Api(#[from] api::Error),
    // Everything the generated surface can fail with, absorbed whole: the
    // messages and the cause chain are the library's, so what an agent reads on
    // stderr does not depend on which side of the seam the failure happened.
    #[error(transparent)]
    Dispatch(#[from] DispatchError),
    #[error(transparent)]
    Body(#[from] api::BodyError),
    #[error("cannot build the request: {0}")]
    Request(#[from] http::Error),
    #[error("no operation named `{0}` in the document")]
    Unknown(String),
    #[error("no command given")]
    NoCommand,
}

/// The whole command tree: every operation the document describes under `raw`,
/// plus the hand-written verbs beside it.
#[must_use]
pub fn root(api: &Api) -> Command {
    Command::new("toy")
        .about("Toy accounting CLI. Every operation comes from the OpenAPI document at startup.")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new(BASE_URL)
                .long(BASE_URL)
                .global(true)
                .value_name("URL")
                .help("Server to send to")
                // Parsed where it arrives, so what the rest of the CLI passes
                // around is a `Uri` and not a string that might be one. A
                // `--base-url` the document's own server could not be is
                // clap's error, naming the value the user typed — never a
                // quiet fall back to the server they were overriding.
                .value_parser(value_parser!(Uri))
                .default_value(api.base().to_string()),
        )
        .subcommand(
            Command::new(RAW)
                .about(
                    "One subcommand per resource in the document, and one per operation under it",
                )
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommands(tree::commands(api.document())),
        )
        .subcommand(finalize_voucher(api))
        .subcommand(
            Command::new(COMPLETIONS)
                .about("Print a completion script for SHELL")
                .arg(
                    Arg::new(SHELL)
                        .required(true)
                        .value_parser(value_parser!(Shell)),
                ),
        )
}

/// The hand-written verb, with the words its chain stands behind.
///
/// `--commit` is this verb's own, because the chain is this verb's own. The
/// gates are not: they belong to the operation the chain calls, so `tree::gates`
/// puts them here — the same call `tree::command` makes for a generated
/// subcommand, spelled once. An Overlay that renames the gate renames this flag
/// with it, and there is nothing to keep in step.
///
/// Every gate is `required`, exactly as it is under `raw`, and a voucher that
/// turns out not to need enshrining does not soften that: which operations the
/// chain reaches is the *server's* answer, arriving after the command line is
/// gone, so the word is asked for while there is still someone to ask.
fn finalize_voucher(api: &Api) -> Command {
    tree::gates(
        Command::new(FINALIZE)
            .about("Fetch a voucher, enshrine it if it is open, then render it")
            .long_about(
                "Fetch a voucher, enshrine it if it is open, then render it.\n\n\
                 The fetch is a read and always runs — the chain depends on the \
                 answer. Without --commit the writes that follow are printed and \
                 not sent.",
            )
            .arg(
                Arg::new(ID)
                    .long(ID)
                    .required(true)
                    .value_name("INT")
                    .value_parser(value_parser!(i64))
                    .help("The voucher to finalize"),
            )
            .arg(
                Arg::new(typed_openapi::COMMIT)
                    .long(typed_openapi::COMMIT)
                    .action(clap::ArgAction::SetTrue)
                    .help("Send the writes. Without it this is a dry run that prints them"),
            ),
        api.operation(OperationId::EnshrineVoucher),
    )
}

/// Run what the user typed, against the server they named.
///
/// The handle is taken by value and pointed at `--base-url` before anything is
/// dispatched, so there is no route from a command line to a request that can
/// miss the flag. Every verb — `raw` and the hand-written ones alike — builds
/// its requests from the handle this function hands on, and a verb added later
/// inherits the flag without knowing it exists.
pub fn run<C: SyncClient>(api: Api, client: &C, matches: &ArgMatches) -> Result<Output, Error> {
    let api = retarget(api, matches);
    match matches.subcommand() {
        Some((RAW, sub)) => raw::run(&api, client, sub),
        Some((FINALIZE, sub)) => finalize::run(
            &api,
            client,
            sub.get_one::<i64>(ID).copied().unwrap_or_default(),
            // Read the way the generated surface reads its own: the flags on
            // this verb are the operation's words, so the same function makes
            // the same answer out of them.
            &tree::answers(api.operation(OperationId::EnshrineVoucher), sub),
        ),
        Some((COMPLETIONS, sub)) => Ok(completions(&api, sub)),
        Some((name, _)) => Err(Error::Unknown(name.to_owned())),
        None => Err(Error::NoCommand),
    }
}

/// Point the handle at the server `--base-url` names.
///
/// The flag is global and carries a [`Uri`] that [`root`] has already parsed;
/// where it is absent the document's own server stands, untouched. This is the
/// only place in the CLI that reads it.
fn retarget(api: Api, matches: &ArgMatches) -> Api {
    match matches.get_one::<Uri>(BASE_URL) {
        Some(base) => api.with_base(base.clone()),
        None => api,
    }
}

/// The tree, printed as a script the shell can source.
fn completions(api: &Api, matches: &ArgMatches) -> Output {
    let shell = matches
        .get_one::<Shell>(SHELL)
        .copied()
        .unwrap_or(Shell::Bash);
    let mut script = Vec::new();
    clap_complete::aot::generate(shell, &mut root(api), "toy", &mut script);
    Output::note(String::from_utf8_lossy(&script).into_owned(), String::new())
}
